use agent_terminal_core::ipc::{read_frame, write_frame, IpcClient};
use agent_terminal_core::protocol::{Request, Response};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;
use tokio::sync::oneshot;

mod common;

/// Generate a unique temp socket path
fn temp_socket() -> String {
    common::temp_socket_path()
}

/// Spawn a simple one-shot mock server
async fn spawn_mock_server<F, Fut>(socket_path: &str, handler: F)
where
    F: FnOnce(tokio::net::UnixStream) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path).unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        handler(stream).await;
    });
    // Give the server a moment to start
    tokio::time::sleep(Duration::from_millis(10)).await;
}

/// Spawn a persistent mock server that can handle multiple connections
async fn spawn_persistent_mock_server<F, Fut>(
    socket_path: &str,
    handler: F,
) -> oneshot::Sender<()>
where
    F: Fn(tokio::net::UnixStream) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path).unwrap();
    let (tx, mut rx) = oneshot::channel();

    let path = socket_path.to_string();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _)) => {
                            tokio::spawn(handler(stream));
                        }
                        Err(_) => break,
                    }
                }
                _ = &mut rx => break,
            }
        }
        let _ = std::fs::remove_file(&path);
    });

    tokio::time::sleep(Duration::from_millis(10)).await;
    tx
}

#[tokio::test]
async fn test_ipc_client_server_roundtrip() {
    let path = temp_socket();

    spawn_mock_server(&path, |mut stream| async move {
        let req: Request = read_frame(&mut stream).await.unwrap();
        match req {
            Request::WriteInput { data } => {
                assert_eq!(data, "hello world");
                write_frame(&mut stream, &Response::Ok).await.unwrap();
            }
            _ => panic!("unexpected request"),
        }
    })
    .await;

    let mut client = IpcClient::connect(&path).await.unwrap();
    let response = client.send(&Request::WriteInput {
        data: "hello world".to_string(),
    })
    .await
    .unwrap();

    assert!(matches!(response, Response::Ok));
}

#[tokio::test]
async fn test_ipc_multiple_clients_sequential() {
    let path = temp_socket();
    let (tx, mut rx) = oneshot::channel::<()>();

    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    if let Ok((mut stream, _)) = accept {
                        tokio::spawn(async move {
                            let req: Request = read_frame(&mut stream).await.unwrap();
                            match req {
                                Request::GetOutput => {
                                    write_frame(&mut stream, &Response::Output {
                                        raw_b64: "test".to_string(),
                                        screen: "test".to_string(),
                                    })
                                    .await
                                    .unwrap();
                                }
                                _ => {}
                            }
                        });
                    }
                }
                _ = &mut rx => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Connect multiple clients sequentially
    for _i in 0..5 {
        let mut client = IpcClient::connect(&path).await.unwrap();
        let response = client.send(&Request::GetOutput).await.unwrap();
        match response {
            Response::Output { screen, .. } => {
                assert!(screen.contains("client") || screen == "test");
            }
            _ => panic!("unexpected response"),
        }
    }

    let _ = tx.send(());
}

#[tokio::test]
async fn test_ipc_multiple_clients_concurrent() {
    let path = temp_socket();
    let shutdown_tx = spawn_persistent_mock_server(&path, |mut stream| async move {
        loop {
            let req: Request = match read_frame(&mut stream).await {
                Ok(r) => r,
                Err(_) => break,
            };

            let resp = match req {
                Request::WriteInput { data: _ } => Response::Ok,
                Request::GetOutput => Response::Output {
                    raw_b64: "test".to_string(),
                    screen: "concurrent".to_string(),
                },
                _ => Response::Error { message: "unexpected request".into() },
            };

            if write_frame(&mut stream, &resp).await.is_err() {
                break;
            }
        }
    })
    .await;

    // Spawn multiple concurrent clients
    let mut handles = vec![];
    for i in 0..10 {
        let path = path.clone();
        let handle = tokio::spawn(async move {
            let mut client = IpcClient::connect(&path).await.unwrap();
            let req = if i % 2 == 0 {
                Request::WriteInput {
                    data: format!("client {}", i),
                }
            } else {
                Request::GetOutput
            };
            client.send(&req).await.unwrap()
        });
        handles.push(handle);
    }

    // Wait for all clients to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(matches!(result, Response::Ok | Response::Output { .. }));
    }

    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn test_ipc_reconnect_after_disconnect() {
    let path = temp_socket();

    // First server
    spawn_mock_server(&path, |mut stream| async move {
        let _req: Request = read_frame(&mut stream).await.unwrap();
        write_frame(&mut stream, &Response::Ok).await.unwrap();
    })
    .await;

    // First connection
    {
        let mut client = IpcClient::connect(&path).await.unwrap();
        let resp = client
            .send(&Request::WriteInput {
                data: "first".to_string(),
            })
            .await
            .unwrap();
        assert!(matches!(resp, Response::Ok));
    }

    // Wait a bit and start new server (simulating server restart)
    tokio::time::sleep(Duration::from_millis(50)).await;

    spawn_mock_server(&path, |mut stream| async move {
        let req: Request = read_frame(&mut stream).await.unwrap();
        match req {
            Request::WriteInput { data } => {
                assert_eq!(data, "second");
                write_frame(&mut stream, &Response::Ok).await.unwrap();
            }
            _ => {}
        }
    })
    .await;

    // Reconnect
    let mut client = IpcClient::connect(&path).await.unwrap();
    let resp = client
        .send(&Request::WriteInput {
            data: "second".to_string(),
        })
        .await
        .unwrap();
    assert!(matches!(resp, Response::Ok));
}

#[tokio::test]
async fn test_ipc_large_payload() {
    let path = temp_socket();

    // Generate a large payload (~3.9 MB, close to but under 4MB limit)
    let large_data = "x".repeat(3_900_000);

    spawn_mock_server(&path, |mut stream| async move {
        let req: Request = read_frame(&mut stream).await.unwrap();
        match req {
            Request::WriteInput { data } => {
                assert_eq!(data.len(), 3_900_000);
                write_frame(&mut stream, &Response::Ok).await.unwrap();
            }
            _ => panic!("unexpected request"),
        }
    })
    .await;

    let mut client = IpcClient::connect(&path).await.unwrap();
    let resp = client
        .send(&Request::WriteInput { data: large_data })
        .await
        .unwrap();
    assert!(matches!(resp, Response::Ok));
}

#[tokio::test]
async fn test_ipc_malformed_frame_handling() {
    let path = temp_socket();

    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        // Send malformed frame: claim length is 100 but only send 10 bytes
        stream.write_all(&100u32.to_le_bytes()).await.unwrap();
        stream.write_all(b"short data").await.unwrap();
        // Keep connection open but don't send more
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    tokio::time::sleep(Duration::from_millis(10)).await;

    let mut client = IpcClient::connect(&path).await.unwrap();
    let result = client.send(&Request::GetOutput).await;
    // Should fail because server won't respond properly
    assert!(result.is_err());
}

#[tokio::test]
async fn test_ipc_server_shutdown_graceful() {
    let path = temp_socket();

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    if let Ok((mut stream, _)) = accept {
                        tokio::spawn(async move {
                            loop {
                                let _req: Request = match read_frame(&mut stream).await {
                                    Ok(r) => r,
                                    Err(_) => break,
                                };
                                let resp = Response::Ok;
                                if write_frame(&mut stream, &resp).await.is_err() {
                                    break;
                                }
                            }
                        });
                    }
                }
                _ = &mut shutdown_rx => break,
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Connect and communicate
    let mut client = IpcClient::connect(&path).await.unwrap();
    let resp = client
        .send(&Request::WriteInput {
            data: "test".to_string(),
        })
        .await
        .unwrap();
    assert!(matches!(resp, Response::Ok));

    // Shutdown server
    let _ = shutdown_tx.send(());
    let _ = handle.await;

    // Try to connect again - should fail
    let result = IpcClient::connect(&path).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_ipc_empty_data_write() {
    let path = temp_socket();

    spawn_mock_server(&path, |mut stream| async move {
        let req: Request = read_frame(&mut stream).await.unwrap();
        match req {
            Request::WriteInput { data } => {
                assert!(data.is_empty());
                write_frame(&mut stream, &Response::Ok).await.unwrap();
            }
            _ => panic!("unexpected request"),
        }
    })
    .await;

    let mut client = IpcClient::connect(&path).await.unwrap();
    let resp = client
        .send(&Request::WriteInput {
            data: "".to_string(),
        })
        .await
        .unwrap();
    assert!(matches!(resp, Response::Ok));
}

#[tokio::test]
async fn test_ipc_unicode_data() {
    let path = temp_socket();

    let unicode_data = "Hello 世界 🌍 ñoño café 日本語 العربية";

    spawn_mock_server(&path, |mut stream| async move {
        let req: Request = read_frame(&mut stream).await.unwrap();
        match req {
            Request::WriteInput { data } => {
                assert_eq!(data, "Hello 世界 🌍 ñoño café 日本語 العربية");
                write_frame(&mut stream, &Response::Ok).await.unwrap();
            }
            _ => panic!("unexpected request"),
        }
    })
    .await;

    let mut client = IpcClient::connect(&path).await.unwrap();
    let resp = client
        .send(&Request::WriteInput {
            data: unicode_data.to_string(),
        })
        .await
        .unwrap();
    assert!(matches!(resp, Response::Ok));
}

#[tokio::test]
async fn test_ipc_error_response_propagation() {
    let path = temp_socket();

    spawn_mock_server(&path, |mut stream| async move {
        let _req: Request = read_frame(&mut stream).await.unwrap();
        write_frame(
            &mut stream,
            &Response::Error {
                message: "custom error message".to_string(),
            },
        )
        .await
        .unwrap();
    })
    .await;

    let mut client = IpcClient::connect(&path).await.unwrap();
    let result = client.write_input("test").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("custom error message"));
}
