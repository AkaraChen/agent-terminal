//! Example: Testing vim using the E2E DSL
//!
//! This example demonstrates both approaches:
//! 1. Using a pure text script
//! 2. Using the fluent Rust API

use agent_terminal_e2e::Session;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let test_file = "/tmp/e2e_vim_test.txt";

    // Clean up any existing test file
    let _ = std::fs::remove_file(test_file);

    println!("=== Vim E2E Test ===\n");

    // Approach 1: Using pure text script
    println!("Approach 1: Using pure text script");
    {
        let session = Session::new().await?;

        let script = format!(
            r#"
# Wait for shell prompt
wait 500ms

# Start vim
write "vim {}\n"
wait 2s

# Enter insert mode and type
write "iHello from E2E DSL"
wait 500ms

# Save and exit
write "\x1b:wq\n"
wait 1s
"#,
            test_file
        );

        session.run_script_str(&script).await?;
        println!("✓ Pure text script completed\n");
    }

    // Verify file was created
    let content = std::fs::read_to_string(test_file)?;
    assert!(content.contains("Hello from E2E DSL"), "File should contain expected content");
    println!("✓ File content verified: {}", content.trim());

    // Clean up
    std::fs::remove_file(test_file)?;

    // Approach 2: Using fluent Rust API
    println!("\nApproach 2: Using fluent Rust API");
    {
        let session = Session::new().await?;

        session
            .run_dsl(|dsl| {
                dsl.wait(Duration::from_millis(500))
                    .write(format!("vim {}\n", test_file))
                    .wait(Duration::from_secs(2))
                    .write("iHello from Rust API")
                    .wait(Duration::from_millis(500))
                    .write("\x1b:wq\n")
                    .wait(Duration::from_secs(1))
            })
            .await?;

        println!("✓ Fluent API completed\n");
    }

    // Verify file again
    let content = std::fs::read_to_string(test_file)?;
    assert!(content.contains("Hello from Rust API"));
    println!("✓ File content verified: {}", content.trim());

    // Clean up
    std::fs::remove_file(test_file)?;

    println!("\n=== All tests passed! ===");
    Ok(())
}
