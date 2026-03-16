use agent_terminal_core::lock::LockFile;
use std::thread;
use std::time::Duration;

mod common;

/// Generate a unique session ID for testing
fn unique_id() -> String {
    format!("test-{}", uuid::Uuid::new_v4())
}

/// Wait for a specific amount of time
fn sleep_secs(secs: u64) {
    thread::sleep(Duration::from_secs(secs));
}

#[test]
fn test_lock_session_lifecycle() {
    let id = unique_id();

    // Create session
    let lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();
    assert!(LockFile::path_for(&id).exists());

    // Verify it's alive
    let read_lock = LockFile::read(&id).unwrap();
    assert!(read_lock.is_alive());

    // Update heartbeat
    let mut lock = LockFile::read(&id).unwrap();
    lock.heartbeat().unwrap();

    // Cleanup
    lock.remove();
    assert!(!LockFile::path_for(&id).exists());
}

#[test]
fn test_lock_multiple_sessions_isolation() {
    let ids: Vec<String> = (0..5).map(|_| unique_id()).collect();
    let mut locks = Vec::new();

    // Create multiple sessions
    for id in &ids {
        let lock = LockFile::new(id.clone(), std::process::id());
        lock.write().unwrap();
        locks.push(lock);
    }

    // Verify all sessions exist and are isolated
    for id in &ids {
        let lock = LockFile::read(id).unwrap();
        assert_eq!(lock.session_id, *id);
        assert!(lock.is_alive());
    }

    // Verify scan_active finds all
    let active = LockFile::scan_active();
    for id in &ids {
        let found = active.iter().any(|l| l.session_id == *id);
        assert!(found, "session {} should be found", id);
    }

    // Cleanup - remove only every other one first
    for (i, lock) in locks.iter().enumerate() {
        if i % 2 == 0 {
            lock.remove();
        }
    }

    // Verify removed ones are gone
    for (i, id) in ids.iter().enumerate() {
        if i % 2 == 0 {
            assert!(!LockFile::path_for(id).exists());
        } else {
            assert!(LockFile::path_for(id).exists());
        }
    }

    // Cleanup remaining
    for (i, lock) in locks.iter().enumerate() {
        if i % 2 != 0 {
            lock.remove();
        }
    }
}

#[test]
fn test_lock_heartbeat_keeps_session_alive() {
    // Note: This test manipulates tick values directly to avoid long waits
    let id = unique_id();

    let mut lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();

    // Manually set tick to be almost expired
    lock.tick = LockFile::read(&id).unwrap().tick - 4; // 4 seconds ago (TTL is 5)
    lock.write().unwrap();

    // Should still be alive
    let lock = LockFile::read(&id).unwrap();
    assert!(
        lock.is_alive(),
        "Session should still be alive at 4 seconds"
    );

    // Heartbeat to renew
    let mut lock = LockFile::read(&id).unwrap();
    lock.heartbeat().unwrap();

    // Should definitely be alive now
    let lock = LockFile::read(&id).unwrap();
    assert!(lock.is_alive());

    lock.remove();
}

#[test]
fn test_lock_stale_session_cleanup() {
    let id = unique_id();

    let mut lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();

    // Set tick to be expired (6 seconds ago, TTL is 5)
    lock.tick = lock.tick.saturating_sub(6);
    lock.write().unwrap();

    // Should be dead
    let lock = LockFile::read(&id).unwrap();
    assert!(!lock.is_alive(), "Session should be dead");

    // scan_active should not include it
    let active = LockFile::scan_active();
    let found = active.iter().any(|l| l.session_id == id);
    assert!(!found, "Dead session should not appear in scan_active");

    // Cleanup
    lock.remove();
}

#[test]
fn test_lock_prefix_matching_ambiguous() {
    let id1 = format!("test-prefix-{}", uuid::Uuid::new_v4());
    let id2 = format!("test-prefix-{}", uuid::Uuid::new_v4());

    let lock1 = LockFile::new(id1.clone(), 1001);
    let lock2 = LockFile::new(id2.clone(), 1002);

    lock1.write().unwrap();
    lock2.write().unwrap();

    // Find by full id1
    let found = LockFile::find_active(&id1);
    assert!(found.is_some());
    assert_eq!(found.unwrap().session_id, id1);

    // Find by unique prefix of id1
    let prefix1 = &id1[..20];
    let found = LockFile::find_active(prefix1);
    assert!(found.is_some());
    assert_eq!(found.unwrap().session_id, id1);

    // Find by full id2
    let found = LockFile::find_active(&id2);
    assert!(found.is_some());
    assert_eq!(found.unwrap().session_id, id2);

    // Both have same prefix "test-prefix-", find_active returns first match
    let found = LockFile::find_active("test-prefix-");
    assert!(found.is_some());
    // It should match one of them
    let session_id = found.unwrap().session_id;
    assert!(session_id == id1 || session_id == id2);

    lock1.remove();
    lock2.remove();
}

#[test]
fn test_lock_concurrent_heartbeats() {
    use std::sync::{Arc, Mutex};

    let id = unique_id();

    let lock = LockFile::new(id.clone(), std::process::id());
    lock.write().unwrap();

    let num_threads = 5;
    let iterations = 5;

    // Use a mutex to synchronize access to the lock file since file operations
    // are not atomic across read-modify-write cycles
    let sync = Arc::new(Mutex::new(()));

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let id = id.clone();
            let sync = Arc::clone(&sync);
            thread::spawn(move || {
                for _ in 0..iterations {
                    let _guard = sync.lock().unwrap();
                    if let Ok(mut lock) = LockFile::read(&id) {
                        let _ = lock.heartbeat();
                    }
                    drop(_guard);
                    thread::sleep(Duration::from_millis(5));
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // Verify session is still alive
    let lock = LockFile::read(&id).unwrap();
    assert!(lock.is_alive());

    lock.remove();
}

#[test]
fn test_lock_session_dir_permissions() {
    // This test checks that the session directory is created properly
    // and files can be written/read
    let id = unique_id();

    let lock = LockFile::new(id.clone(), std::process::id());

    // Should be able to write
    lock.write().unwrap();

    // Should be able to read back
    let read_lock = LockFile::read(&id).unwrap();
    assert_eq!(read_lock.session_id, id);

    // Cleanup
    lock.remove();
}

#[test]
fn test_lock_corrupted_file_recovery() {
    let id = unique_id();

    // Write a corrupted lock file
    let path = LockFile::path_for(&id);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not valid json {{{").unwrap();

    // scan_active should skip it
    let active = LockFile::scan_active();
    let found = active.iter().any(|l| l.session_id == id);
    assert!(!found);

    // read should fail
    let result = LockFile::read(&id);
    assert!(result.is_err());

    // Cleanup
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_lock_find_active_no_match() {
    let result = LockFile::find_active("nonexistent-prefix-xyz123");
    assert!(result.is_none());
}

#[test]
fn test_lock_scan_active_empty_dir() {
    // scan_active on empty/non-existent dir should return empty
    let _active = LockFile::scan_active();
    // We can't guarantee it's empty (other tests may have active sessions)
    // but it should not panic
}

#[test]
fn test_lock_started_at_preserved() {
    let id = unique_id();

    let lock = LockFile::new(id.clone(), std::process::id());
    let started_at = lock.started_at;
    lock.write().unwrap();

    // Heartbeat multiple times
    let mut lock = LockFile::read(&id).unwrap();
    let original_tick = lock.tick;

    thread::sleep(Duration::from_millis(100));
    lock.heartbeat().unwrap();

    // Verify started_at is preserved
    let lock = LockFile::read(&id).unwrap();
    assert_eq!(lock.started_at, started_at);
    // tick should be >= original (may be equal if within same second)
    assert!(lock.tick >= original_tick);

    lock.remove();
}
