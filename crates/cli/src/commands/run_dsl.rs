use agent_terminal_e2e::Session;
use anyhow::{Context, Result};
use std::path::Path;

/// Run an ATDSL script file.
pub async fn run(script_path: &str) -> Result<()> {
    // Check if file exists and has correct extension
    let path = Path::new(script_path);

    if !path.exists() {
        anyhow::bail!("Script file not found: {}", script_path);
    }

    if let Some(ext) = path.extension() {
        if ext != "atdsl" {
            eprintln!("Warning: File does not have .atdsl extension");
        }
    }

    println!("Starting ATDSL script execution...");
    println!("Script: {}", script_path);

    // Create a new session
    let session = Session::new()
        .await
        .context("Failed to create session")?;

    // Run the script
    session
        .run_script_file(path)
        .await
        .context("Script execution failed")?;

    println!("\n✓ Script executed successfully");

    Ok(())
}
