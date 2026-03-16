use agent_terminal_core::dsl::TestRunner;
use agent_terminal_core::lock::LockFile;
use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::TestAction;

pub async fn run(session_id: &str, action: TestAction) -> Result<()> {
    let lock = match LockFile::find_active(session_id) {
        Some(l) => l,
        None => bail!("no active session found with id/prefix: {}", session_id),
    };

    let mut runner = TestRunner::connect(&lock.socket_path)
        .await
        .context("connect to session")?;

    match action {
        TestAction::WaitFor { pattern, timeout } => {
            let timeout_secs = timeout.unwrap_or(5);
            println!("Waiting for '{}' (timeout: {}s)...", pattern, timeout_secs);
            match runner
                .wait_for(&pattern, Duration::from_secs(timeout_secs))
                .await
            {
                Ok(_) => {
                    println!("✓ Pattern found: {}", pattern);
                    Ok(())
                }
                Err(_e) => {
                    bail!("✗ Timeout waiting for pattern: {}", pattern);
                }
            }
        }
        TestAction::AssertContains { text } => {
            println!("Asserting screen contains '{}'...", text);
            match runner.assert_screen_contains(&text).await {
                Ok(_) => {
                    println!("✓ Assertion passed: '{}' found", text);
                    Ok(())
                }
                Err(e) => {
                    bail!("✗ Assertion failed: {}", e);
                }
            }
        }
    }
}
