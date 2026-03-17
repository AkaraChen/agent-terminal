//! High-level session management that hides all implementation details.
//!
//! This module provides a clean API for managing terminal sessions without
//! exposing PTY, IPC, lock files, or other low-level details.

use agent_terminal_core::buffer::OutputBuffer;
use agent_terminal_core::default_shell;
use agent_terminal_core::dsl::TestRunner;
use agent_terminal_core::ipc::{read_frame, write_frame};
use agent_terminal_core::lock::LockFile;
use agent_terminal_core::protocol::{Request, Response};
use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::watch;

use crate::runner::{execute_commands, DslBuilder};
use crate::script::Script;

/// A high-level terminal session that hides all implementation details.
///
/// # Example
///
/// ```rust,ignore
/// let session = Session::new().await?;
/// session.run_script_str(r#"
///     wait_for "$" 5s
///     write "echo hello\n"
///     assert_screen "hello"
/// "#).await?;
/// ```
pub struct Session {
    socket_path: String,
    cancel_tx: watch::Sender<bool>,
    _child: Box<dyn portable_pty::Child + Send>,
}

impl Session {
    /// Create a new session with default settings.
    ///
    /// This automatically:
    /// - Creates a PTY
    /// - Spawns the default shell
    /// - Sets up IPC communication
    /// - Starts all background tasks
    pub async fn new() -> Result<Self> {
        Self::with_shell(default_shell()).await
    }

    /// Create a new session with a specific shell.
    pub async fn with_shell(shell: &str) -> Result<Self> {
        // Generate unique session ID
        let session_id = uuid::Uuid::new_v4().to_string();
        let socket_path = format!("/tmp/agent-terminal-e2e-{}.sock", session_id);

        // Clean up any stale socket
        let _ = std::fs::remove_file(&socket_path);

        // Create lock file
        let lock = LockFile::new(session_id.clone(), std::process::id());
        lock.write().context("Failed to write lock file")?;

        // Create PTY
        let pty_system = NativePtySystem::default();
        let pty_size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pty_pair = pty_system.openpty(pty_size).context("Failed to open PTY")?;

        // Spawn shell
        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "xterm-256color");
        let child = pty_pair.slave.spawn_command(cmd).context("Failed to spawn shell")?;
        drop(pty_pair.slave);

        // Create shared buffer
        let buffer = Arc::new(Mutex::new(OutputBuffer::new(24, 80)));
        let (broadcast_tx, _) = tokio::sync::broadcast::channel::<Vec<u8>>(1024);

        // Start IPC server
        let listener = UnixListener::bind(&socket_path).context("Failed to bind socket")?;
        let buf_for_ipc = Arc::clone(&buffer);
        let pty_writer_ipc = Arc::new(Mutex::new(pty_pair.master.take_writer()?));
        let broadcast_tx_ipc = broadcast_tx.clone();
        let (cancel_tx, cancel_rx) = watch::channel(false);

        tokio::spawn(run_ipc_server(
            listener,
            buf_for_ipc,
            pty_writer_ipc,
            broadcast_tx_ipc,
            cancel_rx,
        ));

        // Start PTY reader
        let buf_for_reader = Arc::clone(&buffer);
        let cancel_rx_reader = cancel_tx.subscribe();
        let broadcast_tx_reader = broadcast_tx.clone();
        let mut reader = pty_pair.master.try_clone_reader()?;

        tokio::task::spawn_blocking(move || {
            let mut chunk = [0u8; 4096];
            loop {
                if *cancel_rx_reader.borrow() {
                    break;
                }
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = &chunk[..n];
                        if let Ok(mut buf) = buf_for_reader.lock() {
                            buf.push(data);
                        }
                        let _ = broadcast_tx_reader.send(data.to_vec());
                    }
                    Err(_) => break,
                }
            }
        });

        // Wait for server to be ready
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(Session {
            socket_path,
            cancel_tx,
            _child: child,
        })
    }

    /// Get the socket path for this session.
    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    /// Run a DSL script from a string.
    pub async fn run_script_str(&self, script_str: &str) -> Result<()> {
        let script = Script::parse(script_str)?;
        self.run_script(&script).await
    }

    /// Run a DSL script from a file.
    pub async fn run_script_file<P: AsRef<std::path::Path>>(&self, path: P) -> Result<()> {
        let script = Script::load(path)?;
        self.run_script(&script).await
    }

    /// Run a parsed DSL script.
    pub async fn run_script(&self, script: &Script) -> Result<()> {
        let mut runner = TestRunner::connect(&self.socket_path).await?;
        execute_commands(&mut runner, &script.commands).await
    }

    /// Get a DSL builder for fluent API.
    pub fn dsl(&self) -> DslBuilder {
        DslBuilder::new()
    }

    /// Run DSL commands using the fluent API.
    pub async fn run_dsl<F>(&self, build: F) -> Result<()>
    where
        F: FnOnce(DslBuilder) -> DslBuilder,
    {
        let builder = build(DslBuilder::new());
        let mut runner = TestRunner::connect(&self.socket_path).await?;
        builder.run(&mut runner).await
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Signal all tasks to stop
        let _ = self.cancel_tx.send(true);
        // Clean up socket
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Run the IPC server in the background.
async fn run_ipc_server(
    listener: UnixListener,
    buffer: Arc<Mutex<OutputBuffer>>,
    pty_writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    broadcast_tx: tokio::sync::broadcast::Sender<Vec<u8>>,
    mut cancel: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            accept = listener.accept() => {
                if let Ok((stream, _)) = accept {
                    let buf = Arc::clone(&buffer);
                    let writer = Arc::clone(&pty_writer);
                    let broadcast_tx = broadcast_tx.clone();
                    tokio::spawn(handle_client(stream, buf, writer, broadcast_tx));
                }
            }
            _ = cancel.changed() => break,
        }
    }
}

/// Handle a single IPC client connection.
async fn handle_client(
    mut stream: tokio::net::UnixStream,
    buffer: Arc<Mutex<OutputBuffer>>,
    pty_writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    broadcast_tx: tokio::sync::broadcast::Sender<Vec<u8>>,
) {
    use base64::Engine;

    let mut rx: Option<tokio::sync::broadcast::Receiver<Vec<u8>>> = None;

    loop {
        let resp = if let Some(ref mut receiver) = rx {
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
                            Ok(buf) => {
                                let raw_b64: String = buf.raw_b64();
                                Some(Response::Output {
                                    raw_b64,
                                    screen: buf.screen_contents(),
                                })
                            }
                            Err(_) => Some(Response::Error {
                                message: "buffer lock poisoned".into(),
                            }),
                        },
                        Ok(Request::Subscribe) => Some(Response::Error {
                            message: "already subscribed".into(),
                        }),
                        _ => break,
                    }
                }
                broadcast_result = receiver.recv() => {
                    match broadcast_result {
                        Ok(data) => Some(Response::OutputChunk {
                            raw_b64: base64::engine::general_purpose::STANDARD.encode(&data),
                        }),
                        Err(_) => {
                            rx = None;
                            continue;
                        }
                    }
                }
            }
        } else {
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
                Request::WriteInput { data } => match pty_writer.lock() {
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
                },
                Request::GetOutput => match buffer.lock() {
                    Ok(buf) => {
                        let raw_b64: String = buf.raw_b64();
                        Some(Response::Output {
                            raw_b64,
                            screen: buf.screen_contents(),
                        })
                    }
                    Err(_) => Some(Response::Error {
                        message: "buffer lock poisoned".into(),
                    }),
                },
                _ => Some(Response::Error {
                    message: "not supported".into(),
                }),
            }
        };

        if let Some(resp) = resp {
            if write_frame(&mut stream, &resp).await.is_err() {
                break;
            }
        }
    }
}
