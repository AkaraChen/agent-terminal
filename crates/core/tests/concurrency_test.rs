use agent_terminal_core::ipc::{read_frame, write_frame};
use agent_terminal_core::lock::LockFile;
use agent_terminal_core::protocol::{Request, Response};
use base64::Engine;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::net::UnixListener;

mod common;

#[test]
fn test_concurrent_writes_same_session() {
    use agent_terminal_core::ipc::IpcClient;
    use tokio::runtime::Runtime;

    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        let session_id = uuid::Uuid::new_v4().to_string();
        let socket_path = format!("/tmp/test-concurrent-writes-{}.sock", session_id);

        // Create lock file
        let lock = LockFile::new(session_id.clone(), std::process::id());
        lock.write().unwrap();

        // Start mock server
        let listener = UnixListener::bind(&socket_path).unwrap();
        let buffer = Arc::new(Mutex::new(String::new()));
        let buf_clone = Arc::clone(&buffer);

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let buf = Arc::clone(&buf_clone);
                    tokio::spawn(async move {
                        loop {
                            match read_frame::<Request>(&mut stream).await {
                                Ok(Request::WriteInput { data }) => {
                                    buf.lock().unwrap().push_str(&data);
                                    let _ = write_frame(&mut stream, &Response::Ok).await;
                                }
                                Ok(Request::GetOutput) => {
                                    let content = buf.lock().unwrap().clone();
                                    let resp = Response::Output {
                                        raw_b64: base64::engine::general_purpose::STANDARD
                                            .encode(content.as_bytes()),
                                        screen: content,
                                    };
                                    let _ = write_frame(&mut stream, &resp).await;
                                }
                                Ok(Request::Subscribe)
                                | Ok(Request::Unsubscribe)
                                | Ok(Request::Authenticate { .. }) => {
                                    let _ = write_frame(
                                        &mut stream,
                                        &Response::Error {
                                            message: "not supported".into(),
                                        },
                                    )
                                    .await;
                                }
                                Err(_) => break,
                            }
                        }
                    });
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Spawn multiple concurrent clients
        let mut handles = vec![];
        for i in 0..5 {
            let path = socket_path.clone();
            handles.push(tokio::spawn(async move {
                let mut client = IpcClient::connect(&path).await.unwrap();
                for j in 0..10 {
                    client
                        .write_input(&format!("client {} msg {}\n", i, j))
                        .await
                        .unwrap();
                }
            }));
        }

        // Wait for all to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all data was written
        let mut client = IpcClient::connect(&socket_path).await.unwrap();
        let (_, screen) = client.get_output().await.unwrap();

        for i in 0..5 {
            for j in 0..10 {
                assert!(screen.contains(&format!("client {} msg {}", i, j)));
            }
        }

        // Cleanup
        lock.remove();
        let _ = std::fs::remove_file(&socket_path);
    });
}

#[test]
fn test_concurrent_reads_same_session() {
    use agent_terminal_core::ipc::IpcClient;
    use tokio::runtime::Runtime;

    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        let session_id = uuid::Uuid::new_v4().to_string();
        let socket_path = format!("/tmp/test-concurrent-reads-{}.sock", session_id);

        let lock = LockFile::new(session_id.clone(), std::process::id());
        lock.write().unwrap();

        let listener = UnixListener::bind(&socket_path).unwrap();
        let buffer = Arc::new(Mutex::new("test data".to_string()));
        let buf_clone = Arc::clone(&buffer);

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let buf = Arc::clone(&buf_clone);
                    tokio::spawn(async move {
                        loop {
                            match read_frame::<Request>(&mut stream).await {
                                Ok(Request::GetOutput) => {
                                    let content = buf.lock().unwrap().clone();
                                    let resp = Response::Output {
                                        raw_b64: base64::engine::general_purpose::STANDARD
                                            .encode(content.as_bytes()),
                                        screen: content,
                                    };
                                    let _ = write_frame(&mut stream, &resp).await;
                                }
                                Ok(_) => {
                                    let _ = write_frame(&mut stream, &Response::Ok).await;
                                }
                                Err(_) => break,
                            }
                        }
                    });
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Spawn multiple concurrent readers
        let mut handles = vec![];
        for _ in 0..10 {
            let path = socket_path.clone();
            handles.push(tokio::spawn(async move {
                let mut client = IpcClient::connect(&path).await.unwrap();
                let (_, screen) = client.get_output().await.unwrap();
                assert!(screen.contains("test data"));
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        lock.remove();
        let _ = std::fs::remove_file(&socket_path);
    });
}

#[test]
fn test_concurrent_session_creation() {
    let mut handles = vec![];

    for _ in 0..10 {
        handles.push(thread::spawn(|| {
            let id = uuid::Uuid::new_v4().to_string();
            let lock = LockFile::new(id.clone(), std::process::id());
            lock.write().unwrap();

            // Verify it was created
            assert!(LockFile::path_for(&id).exists());

            // Cleanup
            lock.remove();
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_concurrent_write_and_read() {
    use agent_terminal_core::ipc::IpcClient;
    use tokio::runtime::Runtime;

    let rt = Runtime::new().unwrap();

    rt.block_on(async {
        let session_id = uuid::Uuid::new_v4().to_string();
        let socket_path = format!("/tmp/test-concurrent-write-read-{}.sock", session_id);

        let lock = LockFile::new(session_id.clone(), std::process::id());
        lock.write().unwrap();

        let listener = UnixListener::bind(&socket_path).unwrap();
        let buffer = Arc::new(Mutex::new(String::new()));
        let buf_clone = Arc::clone(&buffer);

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let buf = Arc::clone(&buf_clone);
                    tokio::spawn(async move {
                        loop {
                            match read_frame::<Request>(&mut stream).await {
                                Ok(Request::WriteInput { data }) => {
                                    buf.lock().unwrap().push_str(&data);
                                    let _ = write_frame(&mut stream, &Response::Ok).await;
                                }
                                Ok(Request::GetOutput) => {
                                    let content = buf.lock().unwrap().clone();
                                    let resp = Response::Output {
                                        raw_b64: base64::engine::general_purpose::STANDARD
                                            .encode(content.as_bytes()),
                                        screen: content,
                                    };
                                    let _ = write_frame(&mut stream, &resp).await;
                                }
                                Ok(Request::Subscribe)
                                | Ok(Request::Unsubscribe)
                                | Ok(Request::Authenticate { .. }) => {
                                    let _ = write_frame(
                                        &mut stream,
                                        &Response::Error {
                                            message: "not supported".into(),
                                        },
                                    )
                                    .await;
                                }
                                Err(_) => break,
                            }
                        }
                    });
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Spawn writers and readers concurrently
        let mut handles = vec![];

        // Writers
        for i in 0..3 {
            let path = socket_path.clone();
            handles.push(tokio::spawn(async move {
                let mut client = IpcClient::connect(&path).await.unwrap();
                for j in 0..5 {
                    client
                        .write_input(&format!("write {}-{}", i, j))
                        .await
                        .unwrap();
                }
            }));
        }

        // Readers
        for _ in 0..3 {
            let path = socket_path.clone();
            handles.push(tokio::spawn(async move {
                let mut client = IpcClient::connect(&path).await.unwrap();
                for _ in 0..5 {
                    let _ = client.get_output().await;
                }
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        lock.remove();
        let _ = std::fs::remove_file(&socket_path);
    });
}

#[test]
fn test_concurrent_heartbeat_and_cleanup() {
    let id = uuid::Uuid::new_v4().to_string();
    let lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();

    let mut handles = vec![];

    // Heartbeat threads
    for _ in 0..3 {
        let id = id.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..10 {
                if let Ok(mut l) = LockFile::read(&id) {
                    let _ = l.heartbeat();
                }
                thread::sleep(Duration::from_millis(5));
            }
        }));
    }

    // Cleanup check thread
    handles.push(thread::spawn({
        let id = id.clone();
        move || {
            for _ in 0..5 {
                let active = LockFile::scan_active();
                let found = active.iter().any(|l| l.session_id == id);
                assert!(found, "Session should still be active");
                thread::sleep(Duration::from_millis(20));
            }
        }
    }));

    for handle in handles {
        handle.join().unwrap();
    }

    lock.remove();
}
