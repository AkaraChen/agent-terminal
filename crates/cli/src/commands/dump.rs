use agent_terminal_core::{ipc::IpcClient, lock::LockFile};
use anyhow::Result;

pub async fn run(session_id_prefix: &str) -> Result<()> {
    let lock = LockFile::find_active(session_id_prefix)
        .ok_or_else(|| anyhow::anyhow!("no active session matching '{}'", session_id_prefix))?;

    let mut client = IpcClient::connect(&lock.socket_path).await?;
    let (_raw_b64, screen) = client.get_output().await?;

    println!("=== Session {} | Screen ===", lock.session_id);
    println!("{}", screen);
    Ok(())
}
