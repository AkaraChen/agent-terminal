use agent_terminal_core::{ipc::IpcClient, lock::LockFile};
use anyhow::{bail, Context, Result};
use std::io::{self, Write};
use std::time::Duration;
use tokio::time::sleep;

const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const MAGENTA: &str = "\x1b[35m";
const RESET: &str = "\x1b[0m";

pub async fn run(session_id: &str, command: Option<String>) -> Result<()> {
    let lock = match LockFile::find_active(session_id) {
        Some(l) => l,
        None => bail!("no active session found with id/prefix: {}", session_id),
    };

    let mut client = IpcClient::connect(&lock.socket_path)
        .await
        .context("connect to session")?;

    println!("{}╔════════════════════════════════════════════════════════════╗{}", CYAN, RESET);
    println!("{}║       Interactive Step Debugger for Terminal             ║{}", CYAN, RESET);
    println!("{}╚════════════════════════════════════════════════════════════╝{}", CYAN, RESET);
    println!();
    println!("Commands:");
    println!("  {}s{} - Show current screen state", YELLOW, RESET);
    println!("  {}w <text>{} - Write text to session", YELLOW, RESET);
    println!("  {}r{} - Show raw ANSI bytes", YELLOW, RESET);
    println!("  {}h{} - Show screen history", YELLOW, RESET);
    println!("  {}d <ms>{} - Delay for milliseconds", YELLOW, RESET);
    println!("  {}q{} - Quit", YELLOW, RESET);
    println!();

    // If initial command provided, execute it
    if let Some(cmd) = command {
        println!("{}[Executing initial command: {}]{}\n", MAGENTA, cmd, RESET);
        execute_command(&mut client, &cmd).await?;
        sleep(Duration::from_millis(500)).await;
    }

    loop {
        print!("{}debug> {}", CYAN, RESET);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).map(|s| *s);

        match cmd {
            "q" | "quit" => {
                println!("{}Goodbye!{}", GREEN, RESET);
                break;
            }
            "s" | "screen" => {
                show_screen(&mut client).await?;
            }
            "w" | "write" => {
                if let Some(text) = arg {
                    execute_command(&mut client, text).await?;
                } else {
                    println!("{}Usage: w <text>{}", YELLOW, RESET);
                }
            }
            "r" | "raw" => {
                show_raw(&mut client).await?;
            }
            "h" | "history" => {
                let count = arg.and_then(|a| a.parse().ok()).unwrap_or(5);
                show_history(&mut client, count).await?;
            }
            "d" | "delay" => {
                let ms = arg.and_then(|a| a.parse().ok()).unwrap_or(1000);
                println!("{}Delaying for {}ms...{}", MAGENTA, ms, RESET);
                sleep(Duration::from_millis(ms)).await;
                println!("{}Done!{}", GREEN, RESET);
            }
            "a" | "analyze" => {
                analyze_screen(&mut client).await?;
            }
            "help" => {
                print_help();
            }
            _ => {
                println!("{}Unknown command: {}. Type 'help' for help.{}", YELLOW, cmd, RESET);
            }
        }
    }

    Ok(())
}

async fn execute_command(client: &mut IpcClient, text: &str) -> Result<()> {
    print!("{}Writing: {:?}...{}", MAGENTA, text, RESET);
    io::stdout().flush()?;

    client.write_input(text).await?;
    client.write_input("\n").await?;

    println!("{} ✓{}", GREEN, RESET);
    Ok(())
}

async fn show_screen(client: &mut IpcClient) -> Result<()> {
    let (_raw, screen) = client.get_output().await?;

    println!("{}┌─────────────────── Screen Contents ───────────────────┐{}", CYAN, RESET);
    for line in screen.lines() {
        let truncated = if line.len() > 58 {
            format!("{}...", &line[..55])
        } else {
            line.to_string()
        };
        println!("{}│{:<58}│{}", CYAN, truncated, RESET);
    }
    println!("{}└───────────────────────────────────────────────────────┘{}", CYAN, RESET);

    // Show line count
    let lines: Vec<&str> = screen.lines().collect();
    println!("{}Total lines: {}{}", YELLOW, lines.len(), RESET);

    Ok(())
}

async fn show_raw(client: &mut IpcClient) -> Result<()> {
    let (raw_b64, _screen) = client.get_output().await?;

    println!("{}Raw ANSI (base64, first 200 chars):{}", MAGENTA, RESET);
    println!("{}", &raw_b64[..raw_b64.len().min(200)]);
    println!();

    Ok(())
}

async fn show_history(client: &mut IpcClient, count: usize) -> Result<()> {
    let snapshots = client.get_screen_history(count).await?;

    println!("{}Screen History (last {} snapshots):{}", CYAN, snapshots.len(), RESET);

    for (i, snap) in snapshots.iter().enumerate() {
        let label = snap.label.as_ref().map(|l| format!(" [{}]", l)).unwrap_or_default();
        println!(
            "{}  Snapshot {}{} @ {}ms{}",
            YELLOW, i + 1, label, snap.timestamp_ms, RESET
        );

        // Show first few lines of each snapshot
        let lines: Vec<&str> = snap.screen.lines().take(3).collect();
        for line in &lines {
            if !line.trim().is_empty() {
                let preview: String = line.chars().take(50).collect();
                println!("    {}", preview);
            }
        }
        if snap.screen.lines().count() > 3 {
            println!("    ... ({} more lines)", snap.screen.lines().count() - 3);
        }
    }

    Ok(())
}

async fn analyze_screen(client: &mut IpcClient) -> Result<()> {
    let (_raw, screen) = client.get_output().await?;

    println!("{}Screen Analysis:{}", CYAN, RESET);

    let lines: Vec<&str> = screen.lines().collect();
    println!("  Total lines: {}", lines.len());

    // Detect patterns
    let patterns = [
        ("Vim", vec!["-- INSERT --", "-- NORMAL --", "-- VISUAL --", "~"]),
        ("Alternate Screen", vec!["[?1049h", "[?1049l"]),
        ("Shell", vec!["$", "#", "%", ">"]),
    ];

    for (name, indicators) in patterns {
        let found = indicators.iter().any(|&p| screen.contains(p));
        if found {
            println!("  {}✓{} {}", GREEN, RESET, name);
        }
    }

    // Count non-empty lines
    let non_empty = lines.iter().filter(|l| !l.trim().is_empty()).count();
    println!("  Non-empty lines: {}", non_empty);

    Ok(())
}

fn print_help() {
    println!("{}Available commands:{}", CYAN, RESET);
    println!("  {}s, screen{}    - Show current screen contents", YELLOW, RESET);
    println!("  {}w, write{}     - Write text to session (e.g., 'w vim file.txt')", YELLOW, RESET);
    println!("  {}r, raw{}       - Show raw ANSI bytes", YELLOW, RESET);
    println!("  {}h, history{}   - Show screen history (e.g., 'h 10')", YELLOW, RESET);
    println!("  {}a, analyze{}   - Analyze screen patterns", YELLOW, RESET);
    println!("  {}d, delay{}     - Delay in milliseconds (e.g., 'd 1000')", YELLOW, RESET);
    println!("  {}q, quit{}      - Exit debugger", YELLOW, RESET);
    println!("  {}help{}         - Show this help", YELLOW, RESET);
}
