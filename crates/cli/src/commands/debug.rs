use agent_terminal_core::{ipc::IpcClient, lock::LockFile};
use anyhow::{bail, Context, Result};
use base64::Engine;
use std::time::Duration;
use tokio::time::interval;

/// ANSI color codes for terminal output
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const MAGENTA: &str = "\x1b[35m";
const RESET: &str = "\x1b[0m";

pub async fn run(
    session_id: &str,
    raw: bool,
    watch: bool,
    analyze: bool,
    history: Option<usize>,
) -> Result<()> {
    let lock = match LockFile::find_active(session_id) {
        Some(l) => l,
        None => bail!("no active session found with id/prefix: {}", session_id),
    };

    let mut client = IpcClient::connect(&lock.socket_path)
        .await
        .context("connect to session")?;

    // Handle history mode first
    if let Some(count) = history {
        let snapshots = client.get_screen_history(count).await?;

        println!("{}=== Session {} | Screen History (last {} snapshots) ==={}\n",
            CYAN, session_id, snapshots.len(), RESET);

        for (i, snapshot) in snapshots.iter().enumerate() {
            let label_str = snapshot.label.as_ref()
                .map(|l| format!(" [{}]", l))
                .unwrap_or_default();

            println!("{}--- Snapshot {}{} @ {}ms ---{}",
                YELLOW, i + 1, label_str, snapshot.timestamp_ms, RESET);
            println!("{}", snapshot.screen);
            println!();

            if raw && !snapshot.raw_b64.is_empty() {
                println!("{}Raw bytes (base64):{} {}",
                    MAGENTA, RESET, &snapshot.raw_b64[..snapshot.raw_b64.len().min(80)]);
            }
        }

        return Ok(());
    }

    if watch {
        // Watch mode: continuously display screen
        let mut ticker = interval(Duration::from_millis(500));
        let mut counter = 0u32;

        println!("{}[Watching session {} - Press Ctrl+C to stop]{}\n", CYAN, session_id, RESET);

        loop {
            ticker.tick().await;
            counter += 1;

            let (_raw_b64, screen) = client.get_output().await?;

            // Clear screen and move cursor to top
            print!("\x1b[2J\x1b[H");
            println!("{}=== Frame {} | Session {} ==={}\n", CYAN, counter, session_id, RESET);
            println!("{}", screen);
            println!("\n{}[Press Ctrl+C to stop]{}\n", CYAN, RESET);

            // Also analyze if requested
            if analyze {
                analyze_screen(&screen);
            }
        }
    } else {
        // Single shot mode
        let (raw_b64, screen) = client.get_output().await?;

        println!("{}=== Session {} | Screen Contents ==={}\n", CYAN, session_id, RESET);
        println!("{}", screen);

        if raw {
            println!("\n{}=== Raw ANSI Bytes (base64) ==={}\n", YELLOW, RESET);
            println!("{}", raw_b64);

            // Decode and show hex/escaped version
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&raw_b64) {
                println!("\n{}=== Decoded Raw Bytes (last 2KB) ==={}\n", YELLOW, RESET);
                let start = decoded.len().saturating_sub(2048);
                show_escaped(&decoded[start..]);
            }
        }

        if analyze {
            analyze_screen(&screen);

            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&raw_b64) {
                analyze_ansi_sequences(&decoded);
            }
        }

        Ok(())
    }
}

/// Show bytes in escaped format
fn show_escaped(data: &[u8]) {
    let mut output = String::new();
    for &byte in data {
        match byte {
            0x1b => output.push_str(&format!("{}\\x1b{}", MAGENTA, RESET)),
            b'\n' => output.push_str(&format!("{}\\n{}\n", GREEN, RESET)),
            b'\r' => output.push_str(&format!("{}\\r{}", GREEN, RESET)),
            0x07 => output.push_str(&format!("{}\\a{}", RED, RESET)), // BEL
            0x08 => output.push_str(&format!("{}\\b{}", RED, RESET)), // BS
            0x09 => output.push_str(&format!("{}\\t{}", GREEN, RESET)), // TAB
            32..=126 => output.push(byte as char),
            _ => output.push_str(&format!("{}\\x{:02x}{}", RED, byte, RESET)),
        }
    }
    println!("{}", output);
}

/// Analyze screen content for debugging
fn analyze_screen(screen: &str) {
    println!("\n{}=== Screen Analysis ==={}\n", CYAN, RESET);

    let lines: Vec<&str> = screen.lines().collect();
    println!("Total lines: {}", lines.len());

    // Check for common patterns
    let patterns = [
        ("Vim", vec!["-- INSERT --", "-- NORMAL --", "-- VISUAL --", "~"]),
        ("Alternate Screen", vec!["[?1049h", "[?1049l"]),
        ("Shell Prompt", vec!["$", "#", "%", ">"]),
    ];

    for (name, indicators) in patterns {
        let found = indicators.iter().any(|&p| screen.contains(p));
        if found {
            println!("{}✓{} Detected: {}", GREEN, RESET, name);
        }
    }

    // Show non-empty lines
    println!("\n{}Non-empty lines:{}", YELLOW, RESET);
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            println!("  Line {}: {:?}", i + 1, &trimmed[..trimmed.len().min(60)]);
        }
    }
}

/// Analyze ANSI sequences in raw data
fn analyze_ansi_sequences(data: &[u8]) {
    println!("\n{}=== ANSI Sequence Analysis ==={}\n", CYAN, RESET);

    let mut i = 0;
    let mut sequences = Vec::new();

    while i < data.len() {
        if data[i] == 0x1b && i + 1 < data.len() {
            // Found escape sequence
            let start = i;
            i += 1;

            if data[i] == b'[' {
                // CSI sequence
                i += 1;
                let mut params = Vec::new();
                let mut current_param = String::new();

                while i < data.len() {
                    let c = data[i];
                    if c.is_ascii_digit() || c == b';' || c == b'?' || c == b'>' {
                        if c == b';' {
                            if !current_param.is_empty() {
                                params.push(current_param.clone());
                                current_param.clear();
                            }
                        } else {
                            current_param.push(c as char);
                        }
                        i += 1;
                    } else {
                        // Final byte
                        if !current_param.is_empty() {
                            params.push(current_param);
                        }

                        let seq_desc = describe_csi_sequence(&params, c);
                        sequences.push((start, i - start + 1, seq_desc));
                        i += 1;
                        break;
                    }
                }
            } else if data[i] == b']' {
                // OSC sequence
                i += 1;
                while i < data.len() && data[i] != 0x07 && data[i] != 0x1b {
                    i += 1;
                }
                if i < data.len() {
                    i += 1;
                }
            } else {
                // Other escape
                sequences.push((start, 2, format!("ESC {}", data[i] as char)));
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    // Show important sequences
    let important: Vec<_> = sequences.iter().filter(|(_, _, desc)| {
        desc.contains("1049") ||  // Alternate screen
        desc.contains("Clear") ||
        desc.contains("Cursor") ||
        desc.contains("Color")
    }).collect();

    if !important.is_empty() {
        println!("{}Important sequences found:{}", YELLOW, RESET);
        for (offset, len, desc) in important {
            println!("  @{}: {} ({} bytes)", offset, desc, len);
        }
    }

    // Summary
    println!("\n{}Total ANSI sequences: {}{}", CYAN, sequences.len(), RESET);

    // Check for alternate screen
    let has_enter_alt = sequences.iter().any(|(_, _, d)| d.contains("1049h"));
    let has_exit_alt = sequences.iter().any(|(_, _, d)| d.contains("1049l"));

    if has_enter_alt {
        println!("{}✓{} Enter alternate screen (1049h) detected", GREEN, RESET);
    }
    if has_exit_alt {
        println!("{}✓{} Exit alternate screen (1049l) detected", GREEN, RESET);
    }
    if has_enter_alt && !has_exit_alt {
        println!("{}⚠{} In alternate screen mode", YELLOW, RESET);
    }
}

/// Describe a CSI sequence
fn describe_csi_sequence(params: &[String], final_byte: u8) -> String {
    let param_str = params.join(";");

    match final_byte {
        b'h' => {
            if params.get(0).map(|s| s.starts_with('?')).unwrap_or(false) {
                format!("Set Mode {}", param_str)
            } else {
                format!("Set Mode {}", param_str)
            }
        }
        b'l' => {
            if params.get(0).map(|s| s.starts_with('?')).unwrap_or(false) {
                format!("Reset Mode {}", param_str)
            } else {
                format!("Reset Mode {}", param_str)
            }
        }
        b'm' => format!("SGR (Color/Style) {}", param_str),
        b'H' | b'f' => format!("Cursor Position {}", param_str),
        b'A' => format!("Cursor Up {}", param_str),
        b'B' => format!("Cursor Down {}", param_str),
        b'C' => format!("Cursor Forward {}", param_str),
        b'D' => format!("Cursor Back {}", param_str),
        b'J' => {
            match param_str.as_str() {
                "0" | "" => "Clear From Cursor to End".to_string(),
                "1" => "Clear From Start to Cursor".to_string(),
                "2" => "Clear Entire Screen".to_string(),
                "3" => "Clear Scrollback".to_string(),
                _ => format!("Erase in Display {}", param_str),
            }
        }
        b'K' => {
            match param_str.as_str() {
                "0" | "" => "Clear Line From Cursor".to_string(),
                "1" => "Clear Line To Cursor".to_string(),
                "2" => "Clear Entire Line".to_string(),
                _ => format!("Erase in Line {}", param_str),
            }
        }
        b'r' => format!("Set Scrolling Region {}", param_str),
        b's' => "Save Cursor Position".to_string(),
        b'u' => "Restore Cursor Position".to_string(),
        b'n' => format!("Device Status Report {}", param_str),
        _ => format!("CSI {} {}", param_str, final_byte as char),
    }
}
