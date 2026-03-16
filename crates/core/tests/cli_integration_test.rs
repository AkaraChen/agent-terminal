use agent_terminal_core::ipc::{read_frame, write_frame};
use agent_terminal_core::lock::LockFile;
use agent_terminal_core::protocol::{Request, Response};
use base64::Engine;
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::oneshot;

mod common;

/// Spawn a mock session server that simulates CLI command handling
async fn spawn_mock_session(
    socket_path: &str,
    session_id: &str,
) -> (oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    // Remove stale socket
    let _ = std::fs::remove_file(socket_path);

    // Create lock file
    let lock = LockFile::new(session_id.to_string(), std::process::id());
    lock.write().unwrap();

    let listener = UnixListener::bind(socket_path).unwrap();
    let (tx, mut rx) = oneshot::channel();

    let path = socket_path.to_string();
    let id = session_id.to_string();

    let handle = tokio::spawn(async move {
        let mut output_buffer = String::new();

        loop {
            tokio::select! {
                accept = listener.accept() => {
                    if let Ok((mut stream, _)) = accept {
                        loop {
                            match read_frame::<Request>(&mut stream).await {
                                Ok(Request::WriteInput { data }) => {
                                    output_buffer.push_str(&data);
                                    let _ = write_frame(&mut stream, &Response::Ok).await;
                                }
                                Ok(Request::GetOutput) => {
                                    let resp = Response::Output {
                                        raw_b64: base64::engine::general_purpose::STANDARD.encode(output_buffer.as_bytes()),
                                        screen: output_buffer.clone(),
                                    };
                                    let _ = write_frame(&mut stream, &resp).await;
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
                _ = &mut rx => break,
            }
        }

        // Cleanup
        let _ = std::fs::remove_file(&path);
        let lock = LockFile::new(id, 0);
        lock.remove();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    (tx, handle)
}

#[test]
fn test_cli_list_empty() {
    // Clean up any existing sessions first
    let active = LockFile::scan_active();
    for lock in active {
        lock.remove();
    }

    // List should return empty
    let active = LockFile::scan_active();
    assert!(active.is_empty());
}

#[test]
fn test_cli_list_with_sessions() {
    let session_ids: Vec<String> = (0..3).map(|_| uuid::Uuid::new_v4().to_string()).collect();

    // Create multiple sessions
    for (i, id) in session_ids.iter().enumerate() {
        let lock = LockFile::new(id.clone(), 1000 + i as u32);
        lock.write().unwrap();
    }

    // List should find all active sessions
    let active = LockFile::scan_active();
    assert_eq!(active.len(), 3);

    for id in &session_ids {
        let found = active.iter().any(|l| l.session_id == *id);
        assert!(found, "Session {} should be found", id);
    }

    // Cleanup
    for id in &session_ids {
        let lock = LockFile::new(id.clone(), 0);
        lock.remove();
    }
}

#[tokio::test]
async fn test_cli_write_to_nonexistent_session() {
    let result = LockFile::find_active("nonexistent-prefix-xyz");
    assert!(result.is_none());

    // Attempting to connect should fail
    let result = tokio::net::UnixStream::connect("/tmp/nonexistent-socket.sock").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_cli_dump_from_nonexistent_session() {
    // Finding a nonexistent session should return None
    let result = LockFile::find_active("nonexistent-prefix-abc123");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_cli_full_workflow() {
    let session_id = uuid::Uuid::new_v4().to_string();
    let socket_path = format!("/tmp/test-cli-workflow-{}.sock", session_id);

    // Step 1: Start session (mock)
    let (shutdown_tx, handle) = spawn_mock_session(&socket_path, &session_id).await;

    // Step 2: List sessions - should find our session
    let active = LockFile::scan_active();
    let found = active.iter().any(|l| l.session_id == session_id);
    assert!(found, "Session should be listed");

    // Step 3: Write to session
    {
        use agent_terminal_core::ipc::IpcClient;
        let mut client = IpcClient::connect(&socket_path).await.unwrap();
        client.write_input("echo hello\n").await.unwrap();
    }

    // Step 4: Dump session output
    {
        use agent_terminal_core::ipc::IpcClient;
        let mut client = IpcClient::connect(&socket_path).await.unwrap();
        let (raw_b64, screen) = client.get_output().await.unwrap();

        assert!(!raw_b64.is_empty());
        assert!(screen.contains("echo hello"));
    }

    // Step 5: Write more commands
    {
        use agent_terminal_core::ipc::IpcClient;
        let mut client = IpcClient::connect(&socket_path).await.unwrap();
        client.write_input("ls -la\n").await.unwrap();
        client.write_input("pwd\n").await.unwrap();
    }

    // Step 6: Verify output contains all commands
    {
        use agent_terminal_core::ipc::IpcClient;
        let mut client = IpcClient::connect(&socket_path).await.unwrap();
        let (_, screen) = client.get_output().await.unwrap();

        assert!(screen.contains("echo hello"));
        assert!(screen.contains("ls -la"));
        assert!(screen.contains("pwd"));
    }

    // Cleanup
    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn test_cli_find_by_prefix() {
    let session_id = uuid::Uuid::new_v4().to_string();
    let socket_path = format!("/tmp/test-prefix-{}.sock", session_id);

    let (shutdown_tx, handle) = spawn_mock_session(&socket_path, &session_id).await;

    // Find by full ID
    let found = LockFile::find_active(&session_id);
    assert!(found.is_some());
    assert_eq!(found.unwrap().session_id, session_id);

    // Find by prefix (first 8 chars)
    let prefix = &session_id[..8];
    let found = LockFile::find_active(prefix);
    assert!(found.is_some());

    // Cleanup
    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn test_cli_multiple_writes_same_session() {
    let session_id = uuid::Uuid::new_v4().to_string();
    let socket_path = format!("/tmp/test-multi-write-{}.sock", session_id);

    let (shutdown_tx, handle) = spawn_mock_session(&socket_path, &session_id).await;

    use agent_terminal_core::ipc::IpcClient;

    // Multiple sequential writes
    let mut client = IpcClient::connect(&socket_path).await.unwrap();
    for i in 0..10 {
        client.write_input(&format!("command {}\n", i)).await.unwrap();
    }

    // Verify all commands were recorded
    let (_, screen) = client.get_output().await.unwrap();
    for i in 0..10 {
        assert!(screen.contains(&format!("command {}", i)));
    }

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}

#[tokio::test]
async fn test_cli_write_binary_data() {
    let session_id = uuid::Uuid::new_v4().to_string();
    let socket_path = format!("/tmp/test-binary-{}.sock", session_id);

    let (shutdown_tx, handle) = spawn_mock_session(&socket_path, &session_id).await;

    use agent_terminal_core::ipc::IpcClient;

    let mut client = IpcClient::connect(&socket_path).await.unwrap();

    // Write data with special characters
    client.write_input("data\x00\x01\x02").await.unwrap();

    let (raw_b64, _) = client.get_output().await.unwrap();
    assert!(!raw_b64.is_empty());

    let _ = shutdown_tx.send(());
    let _ = handle.await;
}
