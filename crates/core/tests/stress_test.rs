use agent_terminal_core::ipc::{read_frame, write_frame, IpcClient};
use agent_terminal_core::lock::LockFile;
use agent_terminal_core::protocol::{Request, Response};
use base64::Engine;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::Mutex;

mod common;

#[tokio::test]
async fn stress_test_rapid_writes() {
    let session_id = uuid::Uuid::new_v4().to_string();
    let socket_path = format!("/tmp/stress-rapid-{}.sock", session_id);

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
                                buf.lock().await.push_str(&data);
                                let _ = write_frame(&mut stream, &Response::Ok).await;
                            }
                            Ok(Request::GetOutput) => {
                                let content = buf.lock().await.clone();
                                let resp = Response::Output {
                                    raw_b64: base64::engine::general_purpose::STANDARD.encode(content.as_bytes()),
                                    screen: content,
                                };
                                let _ = write_frame(&mut stream, &resp).await;
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut client = IpcClient::connect(&socket_path).await.unwrap();

    // Rapid fire writes - 1000 writes as fast as possible
    for i in 0..1000 {
        client.write_input(&format!("msg{}", i)).await.unwrap();
    }

    // Verify all data is there
    let (_, screen) = client.get_output().await.unwrap();
    assert!(screen.contains("msg0"));
    assert!(screen.contains("msg999"));

    lock.remove();
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn stress_test_many_sessions() {
    // Create many sessions (50)
    let sessions: Vec<_> = (0..50)
        .map(|i| {
            let id = format!("stress-session-{}", i);
            let lock = LockFile::new(id.clone(), std::process::id());
            lock.write().unwrap();
            lock
        })
        .collect();

    // Verify all are found
    let active = LockFile::scan_active();
    let found_count = sessions
        .iter()
        .filter(|s| active.iter().any(|a| a.session_id == s.session_id))
        .count();
    assert_eq!(found_count, 50);

    // Cleanup all
    for lock in sessions {
        lock.remove();
    }

    // Verify all cleaned up
    let active = LockFile::scan_active();
    for i in 0..50 {
        let id = format!("stress-session-{}", i);
        let found = active.iter().any(|l| l.session_id == id);
        assert!(!found);
    }
}

#[tokio::test]
async fn stress_test_high_frequency_heartbeats() {
    let id = uuid::Uuid::new_v4().to_string();
    let lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();

    // 100 rapid heartbeats
    for _ in 0..100 {
        let mut l = LockFile::read(&id).unwrap();
        l.heartbeat().unwrap();
    }

    // Should still be alive
    let lock = LockFile::read(&id).unwrap();
    assert!(lock.is_alive());

    lock.remove();
}

#[tokio::test]
async fn stress_test_large_output() {
    let session_id = uuid::Uuid::new_v4().to_string();
    let socket_path = format!("/tmp/stress-large-{}.sock", session_id);

    let lock = LockFile::new(session_id.clone(), std::process::id());
    lock.write().unwrap();

    // Create large output data (500KB)
    let large_data = Arc::new("x".repeat(500_000));

    let listener = UnixListener::bind(&socket_path).unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((mut stream, _)) = listener.accept().await {
                let data = Arc::clone(&large_data);
                tokio::spawn(async move {
                    loop {
                        match read_frame::<Request>(&mut stream).await {
                            Ok(Request::GetOutput) => {
                                let resp = Response::Output {
                                    raw_b64: base64::engine::general_purpose::STANDARD.encode(data.as_bytes()),
                                    screen: (*data).clone(),
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

    let mut client = IpcClient::connect(&socket_path).await.unwrap();

    // Request large output
    let (raw_b64, screen) = client.get_output().await.unwrap();
    assert_eq!(screen.len(), 500_000);
    assert!(!raw_b64.is_empty());

    lock.remove();
    let _ = std::fs::remove_file(&socket_path);
}
