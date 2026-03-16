use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const SESSION_DIR: &str = "/tmp/agent-terminal/sessions";
/// Seconds without a heartbeat before a session is considered dead.
const LOCK_TTL_SECS: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    pub session_id: String,
    pub pid: u32,
    pub socket_path: String,
    /// Unix timestamp (seconds) of the last heartbeat tick.
    pub tick: u64,
    /// Unix timestamp (seconds) when the session started.
    pub started_at: u64,
}

impl LockFile {
    /// Path to the lock file for a given session_id.
    pub fn path_for(session_id: &str) -> PathBuf {
        PathBuf::from(SESSION_DIR).join(format!("{}.lock", session_id))
    }

    /// Path to the Unix socket for a given session_id.
    pub fn socket_path_for(session_id: &str) -> String {
        format!("{}/{}.sock", SESSION_DIR, session_id)
    }

    /// Create a new LockFile struct (does not write it yet).
    pub fn new(session_id: String, pid: u32) -> Self {
        let now = now_secs();
        let socket_path = Self::socket_path_for(&session_id);
        LockFile {
            session_id,
            pid,
            socket_path,
            tick: now,
            started_at: now,
        }
    }

    /// Write (or overwrite) the lock file on disk.
    pub fn write(&self) -> Result<()> {
        fs::create_dir_all(SESSION_DIR).context("create session dir")?;
        let path = Self::path_for(&self.session_id);
        let json = serde_json::to_string(self)?;
        fs::write(path, json).context("write lock file")?;
        Ok(())
    }

    /// Update the heartbeat tick and rewrite the file.
    pub fn heartbeat(&mut self) -> Result<()> {
        self.tick = now_secs();
        self.write()
    }

    /// Read a lock file from disk.
    pub fn read(session_id: &str) -> Result<Self> {
        let path = Self::path_for(session_id);
        let data =
            fs::read_to_string(&path).with_context(|| format!("read lock file {:?}", path))?;
        let lock: LockFile = serde_json::from_str(&data)?;
        Ok(lock)
    }

    /// Returns true if this session is considered alive.
    pub fn is_alive(&self) -> bool {
        let now = now_secs();
        now.saturating_sub(self.tick) <= LOCK_TTL_SECS
    }

    /// Remove the lock file from disk.
    pub fn remove(&self) {
        let _ = fs::remove_file(Self::path_for(&self.session_id));
    }

    /// Scan the session directory and return all active (alive) sessions.
    pub fn scan_active() -> Vec<LockFile> {
        let dir = Path::new(SESSION_DIR);
        let mut active = Vec::new();
        let Ok(entries) = fs::read_dir(dir) else {
            return active;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("lock") {
                continue;
            }
            let Ok(data) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(lock) = serde_json::from_str::<LockFile>(&data) else {
                continue;
            };
            if lock.is_alive() {
                active.push(lock);
            }
        }
        active
    }

    /// Find an active session by (prefix of) session_id.
    pub fn find_active(session_id_prefix: &str) -> Option<LockFile> {
        Self::scan_active()
            .into_iter()
            .find(|l| l.session_id.starts_with(session_id_prefix))
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a unique session ID for each test to avoid cross-test interference.
    fn unique_id() -> String {
        format!("test-{}", uuid::Uuid::new_v4())
    }

    // ── Path helpers ─────────────────────────────────────────────────────

    #[test]
    fn test_path_for_has_lock_extension() {
        let path = LockFile::path_for("abc123");
        assert!(path.to_str().unwrap().ends_with("abc123.lock"));
    }

    #[test]
    fn test_path_for_is_under_session_dir() {
        let path = LockFile::path_for("abc123");
        assert!(path.to_str().unwrap().contains(SESSION_DIR));
    }

    #[test]
    fn test_socket_path_for_has_sock_extension() {
        let p = LockFile::socket_path_for("abc123");
        assert!(p.ends_with("abc123.sock"));
    }

    #[test]
    fn test_socket_path_for_is_under_session_dir() {
        let p = LockFile::socket_path_for("abc123");
        assert!(p.contains(SESSION_DIR));
    }

    // ── LockFile::new ────────────────────────────────────────────────────

    #[test]
    fn test_new_sets_session_id_and_pid() {
        let id = unique_id();
        let lock = LockFile::new(id.clone(), 42000);
        assert_eq!(lock.session_id, id);
        assert_eq!(lock.pid, 42000);
    }

    #[test]
    fn test_new_socket_path_contains_session_id() {
        let id = unique_id();
        let lock = LockFile::new(id.clone(), 1);
        assert!(lock.socket_path.contains(&id));
    }

    #[test]
    fn test_new_tick_is_recent() {
        let before = now_secs();
        let lock = LockFile::new("x".to_string(), 1);
        let after = now_secs();
        assert!(lock.tick >= before && lock.tick <= after + 1);
    }

    #[test]
    fn test_new_started_at_equals_tick() {
        let lock = LockFile::new("x".to_string(), 1);
        // Both are set from the same now_secs() call, so they must be equal.
        assert_eq!(lock.tick, lock.started_at);
    }

    // ── write / read roundtrip ───────────────────────────────────────────

    #[test]
    fn test_write_creates_file() {
        let id = unique_id();
        let lock = LockFile::new(id.clone(), 1);
        lock.write().unwrap();
        assert!(LockFile::path_for(&id).exists());
        lock.remove();
    }

    #[test]
    fn test_write_and_read_roundtrip() {
        let id = unique_id();
        let lock = LockFile::new(id.clone(), 77777);
        lock.write().unwrap();

        let back = LockFile::read(&id).unwrap();
        assert_eq!(back.session_id, id);
        assert_eq!(back.pid, 77777);
        assert_eq!(back.tick, lock.tick);

        lock.remove();
    }

    #[test]
    fn test_read_nonexistent_returns_error() {
        let result = LockFile::read("nonexistent-session-does-not-exist-xyz");
        assert!(result.is_err());
    }

    // ── is_alive ─────────────────────────────────────────────────────────

    #[test]
    fn test_is_alive_fresh_lock_returns_true() {
        let lock = LockFile::new(unique_id(), 1);
        assert!(lock.is_alive());
    }

    #[test]
    fn test_is_alive_stale_tick_returns_false() {
        let mut lock = LockFile::new(unique_id(), 1);
        lock.tick = 0; // Unix epoch — definitely expired
        assert!(!lock.is_alive());
    }

    // ── heartbeat ────────────────────────────────────────────────────────

    #[test]
    fn test_heartbeat_updates_in_memory_tick() {
        let id = unique_id();
        let mut lock = LockFile::new(id.clone(), 1);
        lock.write().unwrap(); // Ensure the file exists first
        let old_tick = lock.tick;

        // Artificially age the tick then beat.
        lock.tick = old_tick.saturating_sub(10);
        lock.heartbeat().unwrap();

        assert!(lock.tick >= old_tick.saturating_sub(10));

        lock.remove();
    }

    #[test]
    fn test_heartbeat_writes_updated_tick_to_disk() {
        let id = unique_id();
        let mut lock = LockFile::new(id.clone(), 1);
        lock.write().unwrap();

        lock.tick = 1; // force a stale value on disk too
        lock.heartbeat().unwrap();

        let from_disk = LockFile::read(&id).unwrap();
        assert!(from_disk.tick > 1, "tick on disk should have been updated");

        lock.remove();
    }

    // ── remove ───────────────────────────────────────────────────────────

    #[test]
    fn test_remove_deletes_lock_file() {
        let id = unique_id();
        let lock = LockFile::new(id.clone(), 1);
        lock.write().unwrap();
        assert!(LockFile::path_for(&id).exists());

        lock.remove();
        assert!(!LockFile::path_for(&id).exists());
    }

    #[test]
    fn test_remove_on_nonexistent_file_does_not_panic() {
        let lock = LockFile::new(unique_id(), 1);
        // Never written; remove should silently succeed.
        lock.remove();
    }

    // ── scan_active ──────────────────────────────────────────────────────

    #[test]
    fn test_scan_active_includes_alive_session() {
        let id = unique_id();
        let lock = LockFile::new(id.clone(), 1);
        lock.write().unwrap();

        let active = LockFile::scan_active();
        let found = active.iter().any(|l| l.session_id == id);
        assert!(
            found,
            "freshly written session should appear in scan_active"
        );

        lock.remove();
    }

    #[test]
    fn test_scan_active_excludes_dead_session() {
        let id = unique_id();
        let mut lock = LockFile::new(id.clone(), 1);
        lock.tick = 0; // Dead
        lock.write().unwrap();

        let active = LockFile::scan_active();
        let found = active.iter().any(|l| l.session_id == id);
        assert!(!found, "dead session must not appear in scan_active");

        lock.remove();
    }

    #[test]
    fn test_scan_active_ignores_non_lock_files() {
        // Ensure the session dir exists then write a stray file
        std::fs::create_dir_all(SESSION_DIR).unwrap();
        let stray = format!("{}/stray.txt", SESSION_DIR);
        let _ = std::fs::write(&stray, "not a lock");

        // scan_active must not panic; stray file is simply ignored.
        let _ = LockFile::scan_active();

        let _ = std::fs::remove_file(&stray);
    }

    #[test]
    fn test_scan_active_ignores_malformed_json_lock_file() {
        std::fs::create_dir_all(SESSION_DIR).unwrap();
        // A .lock file with invalid JSON triggers the serde error branch.
        let bad_path = format!(
            "{}/malformed-test-{}.lock",
            SESSION_DIR,
            uuid::Uuid::new_v4()
        );
        std::fs::write(&bad_path, "not valid json {{{").unwrap();

        // Must not panic; malformed entry is skipped.
        let active = LockFile::scan_active();
        let found = active
            .iter()
            .any(|l| l.socket_path.contains("malformed-test"));
        assert!(!found);

        let _ = std::fs::remove_file(&bad_path);
    }

    #[test]
    fn test_scan_active_ignores_unreadable_entry() {
        std::fs::create_dir_all(SESSION_DIR).unwrap();
        // A *directory* with a .lock extension causes read_to_string to fail,
        // exercising the read-error branch in scan_active.
        let dir_lock = format!("{}/unreadable-{}.lock", SESSION_DIR, uuid::Uuid::new_v4());
        std::fs::create_dir_all(&dir_lock).unwrap();

        // Must not panic; unreadable entry is skipped.
        let _ = LockFile::scan_active();

        let _ = std::fs::remove_dir(&dir_lock);
    }

    // ── find_active ──────────────────────────────────────────────────────

    #[test]
    fn test_find_active_full_id() {
        let id = unique_id();
        let lock = LockFile::new(id.clone(), 1);
        lock.write().unwrap();

        let found = LockFile::find_active(&id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().session_id, id);

        lock.remove();
    }

    #[test]
    fn test_find_active_id_prefix() {
        let id = unique_id();
        let lock = LockFile::new(id.clone(), 1);
        lock.write().unwrap();

        let prefix = &id[..10];
        let found = LockFile::find_active(prefix);
        assert!(found.is_some(), "prefix search should find the session");

        lock.remove();
    }

    #[test]
    fn test_find_active_no_match_returns_none() {
        let found = LockFile::find_active("no-such-session-prefix-xyzxyz");
        assert!(found.is_none());
    }
}
