use agent_terminal_core::session;
use anyhow::Result;

pub async fn run() -> Result<()> {
    session::run_session().await
}
