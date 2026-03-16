use agent_terminal_core::{session, default_shell};
use anyhow::Result;

pub async fn run(shell: Option<&str>) -> Result<()> {
    let shell = shell.unwrap_or_else(|| default_shell());
    session::run_session(shell).await
}
