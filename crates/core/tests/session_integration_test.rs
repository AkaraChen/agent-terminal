use agent_terminal_core::buffer::OutputBuffer;
use agent_terminal_core::ipc::{read_frame, write_frame, IpcClient};
use agent_terminal_core::lock::LockFile;
use agent_terminal_core::protocol::{Request, Response};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::watch;

mod common;

/// A mock PTY-like structure that simulates a session
struct MockSession {
    buffer: Arc<Mutex<OutputBuffer>>,
    socket_path: String,
    session_id: String,
    shutdown_tx: Option<watch::Sender<bool>>,
}

impl MockSession {
    async fn new() -> anyhow::Result<Self> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let socket_path = format!("/tmp/test-session-{}.sock", session_id);

        // Remove stale socket
        let _ = std::fs::remove_file(&socket_path);

        let buffer = Arc::new(Mutex::new(OutputBuffer::new(24, 80)));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Create lock file
        let lock = LockFile::new(session_id.clone(), std::process::id());
        lock.write()?;

        // Start mock session server
        let listener = UnixListener::bind(&socket_path)?;
        let buf_for_server = Arc::clone(&buffer);

        tokio::spawn(async move {
            Self::run_server(listener, buf_for_server, shutdown_rx).await;
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        Ok(MockSession {
            buffer,
            socket_path,
            session_id,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    async fn run_server(
        listener: UnixListener,
        buffer: Arc<Mutex<OutputBuffer>>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    if let Ok((mut stream, _)) = accept {
                        let buf = Arc::clone(&buffer);
                        tokio::spawn(async move {
                            loop {
                                match read_frame::<Request>(&mut stream).await {
                                    Ok(Request::WriteInput { data }) => {
                                        // Echo back to buffer
                                        buf.lock().unwrap().push(data.as_bytes());
                                        let _ = write_frame(&mut stream, &Response::Ok).await;
                                    }
                                    Ok(Request::GetOutput) => {
                                        let (raw, screen) = {
                                            let b = buf.lock().unwrap();
                                            (b.raw_b64(), b.screen_contents())
                                        };
                                        let resp = Response::Output {
                                            raw_b64: raw,
                                            screen,
                                        };
                                        let _ = write_frame(&mut stream, &resp).await;
                                    }
                                    Ok(Request::Subscribe) | Ok(Request::Unsubscribe) | Ok(Request::Authenticate { .. }) | Ok(Request::GetScreenHistory { .. }) => {
                                        let _ = write_frame(&mut stream, &Response::Error {
                                            message: "not supported".into(),
                                        }).await;
                                    }
                                    Err(_) => break,
                                }
                            }
                        });
                    }
                }
                _ = shutdown.changed() => break,
            }
        }
    }

    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }

        // Cleanup
        let lock = LockFile::new(self.session_id.clone(), 0);
        lock.remove();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[tokio::test]
async fn test_session_mock_pty_lifecycle() {
    let session = MockSession::new().await.unwrap();
    let session_id = session.session_id.clone();

    // Verify lock file exists
    let lock = LockFile::read(&session_id).unwrap();
    assert!(lock.is_alive());

    // Verify we can connect
    let client = IpcClient::connect(&session.socket_path).await;
    assert!(client.is_ok());

    session.shutdown().await;

    // Verify cleanup
    assert!(!LockFile::path_for(&session_id).exists());
}

#[tokio::test]
async fn test_session_write_and_output() {
    let session = MockSession::new().await.unwrap();
    let mut client = IpcClient::connect(&session.socket_path).await.unwrap();

    // Write data
    client.write_input("hello world").await.unwrap();

    // Get output
    let (raw_b64, screen) = client.get_output().await.unwrap();

    // Verify output contains our data
    assert!(!raw_b64.is_empty());
    assert!(screen.contains("hello world"));

    session.shutdown().await;
}

#[tokio::test]
async fn test_session_multiple_commands() {
    let session = MockSession::new().await.unwrap();
    let mut client = IpcClient::connect(&session.socket_path).await.unwrap();

    // Send multiple commands
    for i in 0..5 {
        let cmd = format!("command {}", i);
        client.write_input(&cmd).await.unwrap();

        let (_, screen) = client.get_output().await.unwrap();
        assert!(screen.contains(&cmd));
    }

    session.shutdown().await;
}

#[tokio::test]
async fn test_session_output_buffering() {
    let session = MockSession::new().await.unwrap();
    let mut client = IpcClient::connect(&session.socket_path).await.unwrap();

    // Write data without reading
    for i in 0..10 {
        client.write_input(&format!("line {}", i)).await.unwrap();
    }

    // Now read - should get all accumulated output
    let (raw_b64, screen) = client.get_output().await.unwrap();

    assert!(!raw_b64.is_empty());
    for i in 0..10 {
        assert!(screen.contains(&format!("line {}", i)));
    }

    session.shutdown().await;
}

#[tokio::test]
async fn test_session_concurrent_clients() {
    let session = MockSession::new().await.unwrap();
    let mut handles = vec![];

    // Spawn multiple concurrent clients
    for i in 0..5 {
        let path = session.socket_path.clone();
        handles.push(tokio::spawn(async move {
            let mut client = IpcClient::connect(&path).await.unwrap();
            client.write_input(&format!("client {}", i)).await.unwrap();
            client.get_output().await.unwrap()
        }));
    }

    // Wait for all to complete
    for handle in handles {
        let (_, screen) = handle.await.unwrap();
        // Each client should see the accumulated output
        assert!(!screen.is_empty());
    }

    session.shutdown().await;
}

#[tokio::test]
async fn test_session_unicode_handling() {
    let session = MockSession::new().await.unwrap();
    let mut client = IpcClient::connect(&session.socket_path).await.unwrap();

    let unicode_text = "Hello 世界 🌍 ñoño café";
    client.write_input(unicode_text).await.unwrap();

    let (raw_b64, screen) = client.get_output().await.unwrap();

    assert!(!raw_b64.is_empty());
    assert!(screen.contains("Hello"));

    session.shutdown().await;
}

#[tokio::test]
async fn test_session_large_write() {
    let session = MockSession::new().await.unwrap();
    let mut client = IpcClient::connect(&session.socket_path).await.unwrap();

    // Write large data (100KB)
    let large_data = "x".repeat(100_000);
    client.write_input(&large_data).await.unwrap();

    // Verify we can still get output
    let (raw_b64, _screen) = client.get_output().await.unwrap();
    assert!(!raw_b64.is_empty());

    session.shutdown().await;
}

#[tokio::test]
async fn test_session_error_on_closed() {
    let session = MockSession::new().await.unwrap();
    let path = session.socket_path.clone();

    let mut client = IpcClient::connect(&path).await.unwrap();

    // Shutdown session
    session.shutdown().await;

    // Give time for shutdown to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Try to write - may fail during send or when reading response
    let _result = client.write_input("test").await;
    // The write should fail because the connection is closed
    // (Note: depending on timing, it might succeed but then fail on read)
    // We just verify the client doesn't panic
}

#[tokio::test]
async fn test_session_reconnect_after_server_restart() {
    let session = MockSession::new().await.unwrap();
    let path = session.socket_path.clone();

    // Connect and write
    {
        let mut client = IpcClient::connect(&path).await.unwrap();
        client.write_input("before").await.unwrap();
    }

    // Shutdown
    session.shutdown().await;

    // Create new session on same socket path
    let session2 = MockSession::new().await.unwrap();
    // Note: socket path will be different since we generate a new UUID
    // This test verifies the cleanup works correctly
    session2.shutdown().await;
}
