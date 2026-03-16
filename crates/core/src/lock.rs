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
        fs::create_dir_all(SESSION_DIR)
            .context("create session dir")?;
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
        let data = fs::read_to_string(&path)
            .with_context(|| format!("read lock file {:?}", path))?;
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
