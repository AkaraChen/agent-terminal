use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::net::UnixListener;
use tokio::sync::oneshot;

/// Generate a unique temporary socket path for testing.
pub fn temp_socket_path() -> String {
    let temp_dir = std::env::temp_dir();
    let uuid = uuid::Uuid::new_v4();
    temp_dir.join(format!("test-ipc-{}.sock", uuid)).to_string_lossy().to_string()
}

/// Generate a unique temporary session directory for testing.
pub fn temp_session_dir() -> PathBuf {
    let temp_dir = std::env::temp_dir();
    let uuid = uuid::Uuid::new_v4();
    temp_dir.join(format!("test-sessions-{}", uuid))
}

/// Create a temporary session directory and return its path.
pub fn setup_temp_session_dir() -> PathBuf {
    let dir = temp_session_dir();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A mock IPC server for testing.
pub struct MockServer {
    pub socket_path: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl MockServer {
    /// Create a new mock server that will bind to the given socket path.
    pub async fn new<F, Fut>(socket_path: &str, handler: F) -> anyhow::Result<Self>
    where
        F: Fn(tokio::net::UnixStream) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        // Remove stale socket file
        let _ = std::fs::remove_file(socket_path);

        let listener = UnixListener::bind(socket_path)?;
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let path = socket_path.to_string();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _)) => {
                                let h = handler.clone();
                                tokio::spawn(h(stream));
                            }
                            Err(_) => break,
                        }
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
            // Cleanup socket file
            let _ = std::fs::remove_file(&path);
        });

        Ok(MockServer {
            socket_path: socket_path.to_string(),
            shutdown_tx: Some(shutdown_tx),
            task_handle: Some(handle),
        })
    }

    /// Shut down the mock server gracefully.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.await;
        }
    }
}

/// Wait for a condition to become true, with a timeout.
pub async fn wait_for_condition<F>(mut condition: F, timeout_ms: u64) -> bool
where
    F: FnMut() -> bool,
{
    let start = std::time::Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}

/// Wait for a file to exist, with a timeout.
pub async fn wait_for_file(path: &Path, timeout_ms: u64) -> bool {
    wait_for_condition(|| path.exists(), timeout_ms).await
}

/// Generate test data of a specific size.
pub fn generate_test_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

/// RAII guard for temporary session directories.
pub struct TempSessionGuard {
    pub dir: PathBuf,
}

impl TempSessionGuard {
    /// Create a new temp session guard with a unique directory.
    pub fn new() -> Self {
        let dir = setup_temp_session_dir();
        TempSessionGuard { dir }
    }

    /// Get the path to a file within the session directory.
    pub fn file_path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Override the SESSION_DIR for testing by setting an environment variable.
    /// Note: This requires the library to support env-based session dir configuration.
    pub fn override_session_dir(&self) -> &Self {
        // This is a placeholder - the actual implementation depends on
        // whether the library supports env-based configuration
        std::env::set_var("AGENT_TERMINAL_SESSION_DIR", &self.dir);
        self
    }
}

impl Default for TempSessionGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TempSessionGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// RAII guard for a lock file that automatically cleans up on drop.
pub struct LockFileGuard {
    pub session_id: String,
    pub original_dir: Option<String>,
}

impl LockFileGuard {
    pub fn new(session_id: String) -> Self {
        LockFileGuard {
            session_id,
            original_dir: None,
        }
    }
}

impl Drop for LockFileGuard {
    fn drop(&mut self) {
        use agent_terminal_core::lock::LockFile;
        // Try to clean up the lock file if it exists
        let path = LockFile::path_for(&self.session_id);
        let _ = std::fs::remove_file(&path);

        // Also try to clean up the socket file
        let socket_path = LockFile::socket_path_for(&self.session_id);
        let _ = std::fs::remove_file(&socket_path);

        // Restore original session dir if it was overridden
        if let Some(ref dir) = self.original_dir {
            std::env::set_var("AGENT_TERMINAL_SESSION_DIR", dir);
        }
    }
}
