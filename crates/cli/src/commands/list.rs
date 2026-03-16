use agent_terminal_core::lock::LockFile;
use anyhow::Result;
use std::time::{Duration, UNIX_EPOCH};

pub fn run() -> Result<()> {
    let sessions = LockFile::scan_active();

    if sessions.is_empty() {
        println!("No active sessions.");
        return Ok(());
    }

    println!(
        "{:<38}  {:>7}  {:<30}  {}",
        "SESSION ID", "PID", "SOCKET", "STARTED AT"
    );
    println!("{}", "-".repeat(95));

    for s in sessions {
        let started = UNIX_EPOCH + Duration::from_secs(s.started_at);
        let datetime: chrono::DateTime<chrono::Local> = started.into();
        println!(
            "{:<38}  {:>7}  {:<30}  {}",
            s.session_id,
            s.pid,
            s.socket_path,
            datetime.format("%Y-%m-%d %H:%M:%S"),
        );
    }

    Ok(())
}
