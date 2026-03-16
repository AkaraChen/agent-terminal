use agent_terminal_core::buffer::OutputBuffer;
use agent_terminal_core::ipc::{read_frame, write_frame};
use agent_terminal_core::lock::LockFile;
use agent_terminal_core::protocol::{Request, Response};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tokio::time::timeout;

mod common;

/// A mock session with streaming support
struct MockStreamingSession {
    buffer: Arc<Mutex<OutputBuffer>>,
    socket_path: String,
    session_id: String,
    broadcast_tx: broadcast::Sender<Vec<u8>>,
    shutdown_tx: Option<tokio::sync::watch::Sender<bool>>,
}

impl MockStreamingSession {
    async fn new() -> anyhow::Result<Self> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let socket_path = format!("/tmp/test-streaming-{}.sock", session_id);

        // Remove stale socket
        let _ = std::fs::remove_file(&socket_path);

        let buffer = Arc::new(Mutex::new(OutputBuffer::new(24, 80)));
        let (broadcast_tx, _) = broadcast::channel::<Vec<u8>>(1024);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // Create lock file
        let lock = LockFile::new(session_id.clone(), std::process::id());
        lock.write()?;

        // Start mock session server
        let listener = UnixListener::bind(&socket_path)?;
        let buf_for_server = Arc::clone(&buffer);
        let broadcast_tx_server = broadcast_tx.clone();

        tokio::spawn(async move {
            Self::run_server(listener, buf_for_server, broadcast_tx_server, shutdown_rx).await;
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        Ok(MockStreamingSession {
            buffer,
            socket_path,
            session_id,
            broadcast_tx,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    async fn run_server(
        listener: UnixListener,
        buffer: Arc<Mutex<OutputBuffer>>,
        broadcast_tx: broadcast::Sender<Vec<u8>>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    if let Ok((stream, _)) = accept {
                        let buf = Arc::clone(&buffer);
                        let tx = broadcast_tx.clone();
                        tokio::spawn(async move {
                            handle_streaming_client(stream, buf, tx).await;
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

async fn handle_streaming_client(
    mut stream: UnixStream,
    buffer: Arc<Mutex<OutputBuffer>>,
    broadcast_tx: broadcast::Sender<Vec<u8>>,
) {
    use base64::Engine;
    let mut rx: Option<broadcast::Receiver<Vec<u8>>> = None;

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
                            buffer.lock().unwrap().push(data.as_bytes());
                            let _ = broadcast_tx.send(data.into_bytes());
                            Some(Response::Ok)
                        }
                        Ok(Request::GetOutput) => {
                            let b = buffer.lock().unwrap();
                            Some(Response::Output {
                                raw_b64: b.raw_b64(),
                                screen: b.screen_contents(),
                            })
                        }
                        Ok(Request::Subscribe) => Some(Response::Error {
                            message: "already subscribed".into(),
                        }),
                        Err(_) => break,
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
                Request::WriteInput { data } => {
                    buffer.lock().unwrap().push(data.as_bytes());
                    let _ = broadcast_tx.send(data.into_bytes());
                    Some(Response::Ok)
                }
                Request::GetOutput => {
                    let b = buffer.lock().unwrap();
                    Some(Response::Output {
                        raw_b64: b.raw_b64(),
                        screen: b.screen_contents(),
                    })
                }
            }
        };

        if let Some(resp) = resp {
            if write_frame(&mut stream, &resp).await.is_err() {
                break;
            }
        }
    }
}

async fn connect_client(socket_path: &str) -> UnixStream {
    UnixStream::connect(socket_path).await.unwrap()
}

#[tokio::test]
async fn test_subscribe_receives_output_chunks() {
    let session = MockStreamingSession::new().await.unwrap();
    let mut stream = connect_client(&session.socket_path).await;

    // Subscribe to output
    write_frame(&mut stream, &Request::Subscribe).await.unwrap();
    let resp: Response = read_frame(&mut stream).await.unwrap();
    assert!(matches!(resp, Response::Ok), "Subscribe should return Ok");

    // Write some data (should trigger broadcast)
    write_frame(&mut stream, &Request::WriteInput { data: "hello".to_string() }).await.unwrap();
    let _: Response = read_frame(&mut stream).await.unwrap(); // Ok response

    // Read the OutputChunk response
    let chunk_resp = timeout(Duration::from_secs(1), read_frame::<Response>(&mut stream)).await;
    assert!(chunk_resp.is_ok(), "Should receive output chunk within timeout");

    let chunk = chunk_resp.unwrap().unwrap();
    match chunk {
        Response::OutputChunk { raw_b64 } => {
            assert!(!raw_b64.is_empty());
        }
        other => panic!("Expected OutputChunk, got {:?}", other),
    }

    session.shutdown().await;
}

#[tokio::test]
async fn test_unsubscribe_stops_receiving_chunks() {
    let session = MockStreamingSession::new().await.unwrap();
    let mut stream = connect_client(&session.socket_path).await;

    // Subscribe
    write_frame(&mut stream, &Request::Subscribe).await.unwrap();
    let resp: Response = read_frame(&mut stream).await.unwrap();
    assert!(matches!(resp, Response::Ok));

    // Write data
    write_frame(&mut stream, &Request::WriteInput { data: "test1".to_string() }).await.unwrap();
    let _: Response = read_frame(&mut stream).await.unwrap(); // Ok response

    // Receive chunk
    let chunk = timeout(Duration::from_millis(500), read_frame::<Response>(&mut stream))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(chunk, Response::OutputChunk { .. }));

    // Unsubscribe
    write_frame(&mut stream, &Request::Unsubscribe).await.unwrap();
    let resp: Response = read_frame(&mut stream).await.unwrap();
    assert!(matches!(resp, Response::Ok));

    // Write more data
    write_frame(&mut stream, &Request::WriteInput { data: "test2".to_string() }).await.unwrap();
    let resp: Response = read_frame(&mut stream).await.unwrap(); // Should be Ok, not OutputChunk
    assert!(matches!(resp, Response::Ok));

    session.shutdown().await;
}

#[tokio::test]
async fn test_multiple_clients_can_subscribe() {
    let session = MockStreamingSession::new().await.unwrap();

    // Create two clients
    let mut stream1 = connect_client(&session.socket_path).await;
    let mut stream2 = connect_client(&session.socket_path).await;

    // Both subscribe
    write_frame(&mut stream1, &Request::Subscribe).await.unwrap();
    write_frame(&mut stream2, &Request::Subscribe).await.unwrap();
    let _: Response = read_frame(&mut stream1).await.unwrap();
    let _: Response = read_frame(&mut stream2).await.unwrap();

    // Write data from client1
    write_frame(&mut stream1, &Request::WriteInput { data: "broadcast".to_string() }).await.unwrap();
    let _: Response = read_frame(&mut stream1).await.unwrap(); // Ok response

    // Both should receive the chunk
    let chunk1 = timeout(Duration::from_millis(500), read_frame::<Response>(&mut stream1))
        .await
        .unwrap()
        .unwrap();
    let chunk2 = timeout(Duration::from_millis(500), read_frame::<Response>(&mut stream2))
        .await
        .unwrap()
        .unwrap();

    assert!(matches!(chunk1, Response::OutputChunk { .. }));
    assert!(matches!(chunk2, Response::OutputChunk { .. }));

    session.shutdown().await;
}

#[tokio::test]
async fn test_subscribe_when_already_subscribed_returns_error() {
    let session = MockStreamingSession::new().await.unwrap();
    let mut stream = connect_client(&session.socket_path).await;

    // Subscribe once
    write_frame(&mut stream, &Request::Subscribe).await.unwrap();
    let resp: Response = read_frame(&mut stream).await.unwrap();
    assert!(matches!(resp, Response::Ok));

    // Subscribe again (should error)
    write_frame(&mut stream, &Request::Subscribe).await.unwrap();
    let resp: Response = read_frame(&mut stream).await.unwrap();
    match resp {
        Response::Error { message } => {
            assert!(message.contains("already subscribed"));
        }
        other => panic!("Expected Error, got {:?}", other),
    }

    session.shutdown().await;
}

#[tokio::test]
async fn test_subscribe_response_format() {
    // Verify Subscribe request serializes correctly
    let req = Request::Subscribe;
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"type\":\"subscribe\""));

    // Verify response format
    let resp = Response::OutputChunk {
        raw_b64: "aGVsbG8=".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"type\":\"output_chunk\""));
    assert!(json.contains("\"raw_b64\":\"aGVsbG8=\""));
}
