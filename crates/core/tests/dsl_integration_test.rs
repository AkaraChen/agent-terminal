use agent_terminal_core::buffer::OutputBuffer;
use agent_terminal_core::ipc::{read_frame, write_frame};
use agent_terminal_core::lock::LockFile;
use agent_terminal_core::protocol::{Request, Response};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::time::sleep;

mod common;

/// A mock session that simulates shell-like behavior for DSL testing
struct MockDslSession {
    session_id: String,
    socket_path: String,
    output: Arc<Mutex<String>>,
    shutdown_tx: Option<tokio::sync::watch::Sender<bool>>,
}

impl MockDslSession {
    async fn new() -> anyhow::Result<Self> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let socket_path = format!("/tmp/test-dsl-{}.sock", session_id);

        let _ = std::fs::remove_file(&socket_path);

        let output = Arc::new(Mutex::new(String::new()));
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Create lock file
        let lock = LockFile::new(session_id.clone(), std::process::id());
        lock.write()?;

        // Start mock server
        let listener = UnixListener::bind(&socket_path)?;
        let output_for_server = Arc::clone(&output);

        tokio::spawn(async move {
            Self::run_server(listener, output_for_server, shutdown_rx).await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        Ok(MockDslSession {
            session_id,
            socket_path,
            output,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    async fn run_server(
        listener: UnixListener,
        output: Arc<Mutex<String>>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        use base64::Engine;

        loop {
            tokio::select! {
                accept = listener.accept() => {
                    if let Ok((mut stream, _)) = accept {
                        let out = Arc::clone(&output);
                        tokio::spawn(async move {
                            loop {
                                match read_frame::<Request>(&mut stream).await {
                                    Ok(Request::WriteInput { data }) => {
                                        out.lock().unwrap().push_str(&data);
                                        // Echo back the command output
                                        if data.trim() == "echo hello" {
                                            out.lock().unwrap().push_str("hello\n$ ");
                                        } else if data.contains("\n") {
                                            out.lock().unwrap().push_str("$ ");
                                        }
                                        let _ = write_frame(&mut stream, &Response::Ok).await;
                                    }
                                    Ok(Request::GetOutput) => {
                                        let screen = out.lock().unwrap().clone();
                                        let resp = Response::Output {
                                            raw_b64: base64::engine::general_purpose::STANDARD.encode(screen.as_bytes()),
                                            screen: screen.clone(),
                                        };
                                        let _ = write_frame(&mut stream, &resp).await;
                                    }
                                    Ok(Request::Subscribe) => {
                                        let _ = write_frame(&mut stream, &Response::Ok).await;
                                        // After subscribe, send initial output
                                        let screen = out.lock().unwrap().clone();
                                        if !screen.is_empty() {
                                            let chunk = Response::OutputChunk {
                                                raw_b64: base64::engine::general_purpose::STANDARD.encode(screen.as_bytes()),
                                            };
                                            let _ = write_frame(&mut stream, &chunk).await;
                                        }
                                    }
                                    Ok(Request::Unsubscribe) => {
                                        let _ = write_frame(&mut stream, &Response::Ok).await;
                                    }
                                    Ok(Request::Authenticate { .. }) => {
                                        let _ = write_frame(&mut stream, &Response::Error {
                                            message: "authentication not supported".into(),
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

    fn socket_path(&self) -> &str {
        &self.socket_path
    }

    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        let lock = LockFile::new(self.session_id.clone(), 0);
        lock.remove();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

#[tokio::test]
async fn test_dsl_wait_for_pattern() {
    use agent_terminal_core::dsl::TestRunner;

    let session = MockDslSession::new().await.unwrap();

    // Initialize session with prompt
    {
        let out = session.output.clone();
        out.lock().unwrap().push_str("$ ");
    }

    let mut runner = TestRunner::connect(session.socket_path()).await.unwrap();

    // Wait for the prompt
    runner
        .wait_for("$", Duration::from_secs(2))
        .await
        .expect("should find prompt");

    session.shutdown().await;
}

#[tokio::test]
async fn test_dsl_write_and_wait() {
    use agent_terminal_core::dsl::TestRunner;

    let session = MockDslSession::new().await.unwrap();

    // Initialize session with prompt
    {
        let out = session.output.clone();
        out.lock().unwrap().push_str("$ ");
    }

    let mut runner = TestRunner::connect(session.socket_path()).await.unwrap();

    // Wait for prompt, then write command
    runner.wait_for("$", Duration::from_secs(2)).await.unwrap();
    runner.write_input("echo hello\n").await.unwrap();

    // Wait for the output
    runner
        .wait_for("hello", Duration::from_secs(2))
        .await
        .expect("should find hello output");

    session.shutdown().await;
}

#[tokio::test]
async fn test_dsl_assert_screen_contains() {
    use agent_terminal_core::dsl::TestRunner;

    let session = MockDslSession::new().await.unwrap();

    // Initialize with some content
    {
        let out = session.output.clone();
        out.lock().unwrap().push_str("test output content\n$ ");
    }

    let mut runner = TestRunner::connect(session.socket_path()).await.unwrap();

    // Assert content is on screen
    runner
        .assert_screen_contains("test output")
        .await
        .expect("screen should contain 'test output'");

    session.shutdown().await;
}

#[tokio::test]
async fn test_dsl_assert_screen_fails_when_not_found() {
    use agent_terminal_core::dsl::TestRunner;

    let session = MockDslSession::new().await.unwrap();

    // Initialize with some content
    {
        let out = session.output.clone();
        out.lock().unwrap().push_str("some content\n$ ");
    }

    let mut runner = TestRunner::connect(session.socket_path()).await.unwrap();

    // Assert that missing content fails
    let result = runner.assert_screen_contains("not present").await;
    assert!(result.is_err(), "assertion should fail for missing content");

    session.shutdown().await;
}

#[tokio::test]
async fn test_dsl_wait_for_timeout() {
    use agent_terminal_core::dsl::TestRunner;

    let session = MockDslSession::new().await.unwrap();

    // Initialize with limited content
    {
        let out = session.output.clone();
        out.lock().unwrap().push_str("$ ");
    }

    let mut runner = TestRunner::connect(session.socket_path()).await.unwrap();

    // Try to wait for something that won't appear
    let result = runner
        .wait_for("NEVER_APPEAR", Duration::from_millis(100))
        .await;

    assert!(result.is_err(), "wait_for should timeout");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("timeout waiting for pattern"));

    session.shutdown().await;
}

#[tokio::test]
async fn test_dsl_output_buffer_accumulation() {
    use agent_terminal_core::dsl::TestRunner;

    let session = MockDslSession::new().await.unwrap();

    {
        let out = session.output.clone();
        out.lock().unwrap().push_str("line1\nline2\n$ ");
    }

    let mut runner = TestRunner::connect(session.socket_path()).await.unwrap();

    // Wait for first pattern
    runner
        .wait_for("line1", Duration::from_secs(1))
        .await
        .unwrap();

    // Buffer should still contain both lines
    assert!(runner.output_buffer().contains("line2"));

    session.shutdown().await;
}

#[tokio::test]
async fn test_dsl_clear_buffer() {
    use agent_terminal_core::dsl::TestRunner;

    let session = MockDslSession::new().await.unwrap();

    {
        let out = session.output.clone();
        out.lock().unwrap().push_str("old content\n$ ");
    }

    let mut runner = TestRunner::connect(session.socket_path()).await.unwrap();

    // Wait for and clear buffer
    runner
        .wait_for("old", Duration::from_secs(1))
        .await
        .unwrap();
    runner.clear_buffer();
    assert!(runner.output_buffer().is_empty());

    session.shutdown().await;
}
