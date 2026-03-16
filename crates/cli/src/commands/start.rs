use agent_terminal_core::{default_shell, session};
use anyhow::Result;

pub async fn run(shell: Option<&str>) -> Result<()> {
    let shell = shell.unwrap_or_else(|| default_shell());
    session::run_session(shell).await
}
