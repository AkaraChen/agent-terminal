use anyhow::Result;

pub async fn run() -> Result<()> {
    core::session::run_session().await
}
