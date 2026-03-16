use std::{
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use crossterm::terminal;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{broadcast, watch},
    time::{interval, Duration},
};

use crate::{
    buffer::OutputBuffer,
    ipc::{read_frame, write_frame},
    lock::LockFile,
    protocol::{Request, Response},
};
use base64::Engine;

/// Capacity for the broadcast channel (output streaming).
const BROADCAST_CAPACITY: usize = 1024;

/// Entry point: start a PTY session running a shell, then block until the
/// child exits (or the process is killed).
///
/// # Arguments
///
/// * `shell` - Path to the shell executable (e.g., "/bin/zsh", "/bin/bash")
pub async fn run_session(shell: &str) -> Result<()> {
    // ── 1. Determine terminal size ──────────────────────────────────────
    let (cols, rows) = terminal::size().unwrap_or((220, 50));
    let pty_size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    // ── 2. Open PTY and spawn shell ─────────────────────────────────────
    let pty_system = NativePtySystem::default();
    let pty_pair = pty_system
        .openpty(pty_size)
        .context("open PTY")?;

    let mut cmd = CommandBuilder::new(shell);
    // Give zsh a proper TERM so readline works.
    cmd.env("TERM", "xterm-256color");

    let mut child = pty_pair
        .slave
        .spawn_command(cmd)
        .context("spawn zsh")?;

    // After spawning we no longer need the slave side in this process.
    drop(pty_pair.slave);

    let pid = std::process::id();
    let session_id = uuid::Uuid::new_v4().to_string();

    // ── 3. Session directory & socket ──────────────────────────────────
    let lock = LockFile::new(session_id.clone(), pid);
    lock.write().context("write initial lock file")?;

    let socket_path = lock.socket_path.clone();
    // Remove stale socket file if present.
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).context("bind unix socket")?;
    // Restrict socket to owner only.
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;

    // ── 4. Shared output buffer ─────────────────────────────────────────
    let buffer = Arc::new(Mutex::new(OutputBuffer::new(rows, cols)));

    // ── 4b. Broadcast channel for output streaming ──────────────────────
    let (broadcast_tx, _) = broadcast::channel::<Vec<u8>>(BROADCAST_CAPACITY);

    // ── 5. PTY reader & writer handles ─────────────────────────────────
    let pty_reader = pty_pair
        .master
        .try_clone_reader()
        .context("clone PTY reader")?;
    let pty_writer = pty_pair
        .master
        .take_writer()
        .context("take PTY writer")?;
    let pty_writer = Arc::new(Mutex::new(pty_writer));

    // ── 6. Raw stdin ────────────────────────────────────────────────────
    terminal::enable_raw_mode().context("enable raw mode")?;

    // Cancellation channel: when the child exits we shut everything down.
    let (cancel_tx, cancel_rx) = watch::channel(false);

    // ── 7. Spawn tasks ──────────────────────────────────────────────────

    // 7a. Heartbeat: update lock.tick every 2 seconds.
    let session_id_hb = session_id.clone();
    let cancel_rx_hb = cancel_rx.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(2));
        let mut cancel = cancel_rx_hb;
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Ok(mut l) = LockFile::read(&session_id_hb) {
                        let _ = l.heartbeat();
                    }
                }
                _ = cancel.changed() => break,
            }
        }
    });

    // 7b. Unix socket server: accept IPC clients.
    let buf_for_ipc = Arc::clone(&buffer);
    let pty_writer_ipc = Arc::clone(&pty_writer);
    let cancel_rx_ipc = cancel_rx.clone();
    let broadcast_tx_ipc = broadcast_tx.clone();
    tokio::spawn(async move {
        let mut cancel = cancel_rx_ipc;
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _)) => {
                            let buf = Arc::clone(&buf_for_ipc);
                            let writer = Arc::clone(&pty_writer_ipc);
                            let broadcast_tx = broadcast_tx_ipc.clone();
                            tokio::spawn(handle_ipc_client(stream, buf, writer, broadcast_tx));
                        }
                        Err(_) => break,
                    }
                }
                _ = cancel.changed() => break,
            }
        }
    });

    // 7c. PTY reader: copy PTY output → stdout + buffer + broadcast.
    let buf_for_reader = Arc::clone(&buffer);
    let cancel_rx_reader = cancel_rx.clone();
    let broadcast_tx_reader = broadcast_tx.clone();
    let reader_handle = tokio::task::spawn_blocking(move || {
        let mut reader = pty_reader;
        let cancel = cancel_rx_reader;
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();
        let mut chunk = [0u8; 4096];
        loop {
            if *cancel.borrow() {
                break;
            }
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let data = &chunk[..n];
                    // Forward to real stdout.
                    let _ = stdout.write_all(data);
                    let _ = stdout.flush();
                    // Store in buffer.
                    if let Ok(mut buf) = buf_for_reader.lock() {
                        buf.push(data);
                    }
                    // Broadcast to subscribers.
                    let _ = broadcast_tx_reader.send(data.to_vec());
                }
                Err(_) => break,
            }
        }
    });

    // 7d. Stdin relay: copy raw stdin → PTY master writer.
    let writer_for_stdin = Arc::clone(&pty_writer);
    let cancel_rx_stdin = cancel_rx.clone();
    tokio::task::spawn_blocking(move || {
        let stdin = std::io::stdin();
        let mut stdin = stdin.lock();
        let cancel = cancel_rx_stdin;
        let mut byte = [0u8; 1];
        loop {
            if *cancel.borrow() {
                break;
            }
            match stdin.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    if let Ok(mut w) = writer_for_stdin.lock() {
                        let _ = w.write_all(&byte);
                    }
                }
                Err(_) => break,
            }
        }
    });

    // ── 8. Wait for zsh to exit ─────────────────────────────────────────
    // Run child-wait in a blocking thread so we don't block the async scheduler.
    tokio::task::spawn_blocking(move || {
        let _ = child.wait();
    })
    .await
    .ok();

    // Signal all tasks to stop.
    let _ = cancel_tx.send(true);

    // Wait for the PTY reader to drain.
    let _ = reader_handle.await;

    // ── 9. Cleanup ──────────────────────────────────────────────────────
    terminal::disable_raw_mode().ok();
    lock.remove();
    let _ = std::fs::remove_file(&socket_path);

    Ok(())
}

/// Handle a single IPC client connection.
async fn handle_ipc_client(
    mut stream: UnixStream,
    buffer: Arc<Mutex<OutputBuffer>>,
    pty_writer: Arc<Mutex<Box<dyn Write + Send>>>,
    broadcast_tx: broadcast::Sender<Vec<u8>>,
) {
    // Track if this client is subscribed for streaming
    let mut rx: Option<broadcast::Receiver<Vec<u8>>> = None;

    loop {
        let resp = if let Some(ref mut receiver) = rx {
            // Subscribed mode: handle both requests and broadcast messages
            tokio::select! {
                req_result = read_frame::<Request>(&mut stream) => {
                    match req_result {
                        Ok(Request::Unsubscribe) => {
                            rx = None;
                            Some(Response::Ok)
                        }
                        Ok(Request::WriteInput { data }) => {
                            match pty_writer.lock() {
                                Ok(mut w) => {
                                    if w.write_all(data.as_bytes()).is_ok() {
                                        Some(Response::Ok)
                                    } else {
                                        Some(Response::Error {
                                            message: "write to PTY failed".into(),
                                        })
                                    }
                                }
                                Err(_) => Some(Response::Error {
                                    message: "PTY writer lock poisoned".into(),
                                }),
                            }
                        }
                        Ok(Request::GetOutput) => match buffer.lock() {
                            Ok(buf) => Some(Response::Output {
                                raw_b64: buf.raw_b64(),
                                screen: buf.screen_contents(),
                            }),
                            Err(_) => Some(Response::Error {
                                message: "buffer lock poisoned".into(),
                            }),
                        },
                        Ok(Request::Subscribe) => Some(Response::Error {
                            message: "already subscribed".into(),
                        }),
                        Err(_) => break,
                    }
                }
                broadcast_result = receiver.recv() => {
                    match broadcast_result {
                        Ok(data) => {
                            Some(Response::OutputChunk {
                                raw_b64: base64::engine::general_purpose::STANDARD.encode(&data),
                            })
                        }
                        Err(_) => {
                            // Broadcast channel closed or lagged, unsubscribe
                            rx = None;
                            continue;
                        }
                    }
                }
            }
        } else {
            // Not subscribed mode: just handle requests
            let req: Request = match read_frame(&mut stream).await {
                Ok(r) => r,
                Err(_) => break,
            };

            match req {
                Request::Subscribe => {
                    rx = Some(broadcast_tx.subscribe());
                    Some(Response::Ok)
                }
                Request::Unsubscribe => Some(Response::Ok),
                Request::WriteInput { data } => {
                    match pty_writer.lock() {
                        Ok(mut w) => {
                            if w.write_all(data.as_bytes()).is_ok() {
                                Some(Response::Ok)
                            } else {
                                Some(Response::Error {
                                    message: "write to PTY failed".into(),
                                })
                            }
                        }
                        Err(_) => Some(Response::Error {
                            message: "PTY writer lock poisoned".into(),
                        }),
                    }
                }
                Request::GetOutput => match buffer.lock() {
                    Ok(buf) => Some(Response::Output {
                        raw_b64: buf.raw_b64(),
                        screen: buf.screen_contents(),
                    }),
                    Err(_) => Some(Response::Error {
                        message: "buffer lock poisoned".into(),
                    }),
                },
            }
        };

        if let Some(resp) = resp {
            if write_frame(&mut stream, &resp).await.is_err() {
                break;
            }
        }
    }
}
