use agent_terminal_core::{ipc::IpcClient, lock::LockFile};
use anyhow::Result;

pub async fn run(session_id_prefix: &str, data: &str) -> Result<()> {
    let lock = LockFile::find_active(session_id_prefix)
        .ok_or_else(|| anyhow::anyhow!("no active session matching '{}'", session_id_prefix))?;

    let mut client = IpcClient::connect(&lock.socket_path).await?;
    client.write_input(data).await?;
    println!("Written {} byte(s) to session {}", data.len(), lock.session_id);
    Ok(())
}
