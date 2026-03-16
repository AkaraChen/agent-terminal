use agent_terminal_core::ipc::IpcClient;
use agent_terminal_core::lock::LockFile;
use std::time::Duration;

mod common;

#[tokio::test]
async fn test_error_socket_not_found() {
    // Try to connect to a non-existent socket
    let result = IpcClient::connect("/tmp/nonexistent-socket-xyz123.sock").await;
    assert!(result.is_err());
}

#[test]
fn test_error_corrupted_lock_file() {
    let id = format!("test-corrupted-{}", uuid::Uuid::new_v4());
    let path = LockFile::path_for(&id);

    // Create directory if needed
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    // Write invalid JSON to the lock file
    std::fs::write(&path, "not valid json {{{").unwrap();

    // Read should fail
    let result = LockFile::read(&id);
    assert!(result.is_err());

    // scan_active should skip it (not panic)
    let active = LockFile::scan_active();
    let found = active.iter().any(|l| l.session_id == id);
    assert!(!found);

    // Cleanup
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_error_session_expired_during_request() {
    // This simulates a session that expires between listing and using
    let id = format!("test-expired-{}", uuid::Uuid::new_v4());

    let mut lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();

    // Verify it's alive
    assert!(lock.is_alive());

    // Manually expire it
    lock.tick = 0; // Unix epoch - definitely expired
    lock.write().unwrap();

    // Now it should be considered dead
    assert!(!lock.is_alive());

    // find_active should not find it
    let result = LockFile::find_active(&id);
    assert!(result.is_none());

    // Cleanup
    lock.remove();
}

#[test]
fn test_error_permission_denied_simulation() {
    // We can't easily test actual permission denied without changing file permissions,
    // but we can test that errors are handled gracefully

    // Try to read a non-existent session
    let result = LockFile::read("nonexistent-session-xyz");
    assert!(result.is_err());
}

#[test]
fn test_error_malformed_session_id() {
    // Test with empty session ID
    let path = LockFile::path_for("");
    assert!(path.to_str().unwrap().ends_with(".lock"));

    // Test with special characters
    let path = LockFile::path_for("test/with/slashes");
    assert!(path.to_str().unwrap().contains("test"));
}

#[tokio::test]
async fn test_error_connection_refused() {
    // Create a lock file pointing to a non-existent socket
    let id = format!("test-no-socket-{}", uuid::Uuid::new_v4());
    let lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();

    // Try to connect - should fail since socket doesn't exist
    let result = IpcClient::connect(&lock.socket_path).await;
    assert!(result.is_err());

    // Cleanup
    lock.remove();
}

#[test]
fn test_error_directory_as_lock_file() {
    // Create a directory with .lock extension (causes read_to_string to fail)
    let id = format!("test-dir-{}", uuid::Uuid::new_v4());
    let path = LockFile::path_for(&id);

    std::fs::create_dir_all(&path).unwrap();

    // scan_active should handle this gracefully
    let _active = LockFile::scan_active();
    // Should not panic

    // Cleanup
    let _ = std::fs::remove_dir(&path);
}

#[test]
fn test_error_network_partition_simulation() {
    // This simulates what happens when the server disappears mid-session
    let id = format!("test-partition-{}", uuid::Uuid::new_v4());
    let lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();

    // Session is recorded but socket doesn't exist
    assert!(LockFile::path_for(&id).exists());

    // find_active should still find it (it's not expired)
    let found = LockFile::find_active(&id);
    assert!(found.is_some());

    // Cleanup
    lock.remove();
}

#[tokio::test]
async fn test_error_disconnect_mid_request() {
    use agent_terminal_core::ipc::read_frame;
    use agent_terminal_core::protocol::Request;
    use tokio::net::UnixListener;
    use tokio::sync::oneshot;

    let socket_path = format!("/tmp/test-disconnect-{}.sock", uuid::Uuid::new_v4());

    // Create a server that will disconnect after receiving a request
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).unwrap();
    let (tx, _rx) = oneshot::channel();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            // Read but don't respond - just close
            let _req: Request = read_frame(&mut stream).await.unwrap();
            // Stream drops here, closing connection
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Try to send request
    let mut client = IpcClient::connect(&socket_path).await.unwrap();
    let result = client.write_input("test").await;
    // Should get an error because server closed without responding
    assert!(result.is_err());

    let _ = tx.send(());
}

#[test]
fn test_error_lock_file_race_condition() {
    // Test rapid create/delete cycles
    let id = format!("test-race-{}", uuid::Uuid::new_v4());

    for i in 0..10 {
        let lock = LockFile::new(format!("{}-{}", id, i), std::process::id());
        lock.write().unwrap();

        // Immediately try to read
        let read_result = LockFile::read(&format!("{}-{}", id, i));
        assert!(read_result.is_ok());

        // Immediately remove
        lock.remove();
    }

    // All should be cleaned up
    let active = LockFile::scan_active();
    for i in 0..10 {
        let found = active.iter().any(|l| l.session_id == format!("{}-{}", id, i));
        assert!(!found);
    }
}

#[test]
fn test_error_invalid_socket_path() {
    // Create lock with invalid socket path
    let id = format!("test-invalid-path-{}", uuid::Uuid::new_v4());
    let mut lock = LockFile::new(id.clone(), std::process::id());

    // Modify socket path to be invalid
    lock.socket_path = "/nonexistent/directory/socket.sock".to_string();
    lock.write().unwrap();

    // Read should still work
    let read_lock = LockFile::read(&id).unwrap();
    assert_eq!(read_lock.socket_path, "/nonexistent/directory/socket.sock");

    lock.remove();
}
