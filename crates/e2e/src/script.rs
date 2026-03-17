//! Pure text DSL script parser for agent-terminal e2e tests.
//!
//! Script files use `.atdsl` extension and contain simple commands:
//!
//! ```atdsl
//! # Wait for shell prompt
//! wait_for "$" 5s
//!
//! # Start vim
//! write "vim test.txt\\n"
//! wait_for "~" 5s
//!
//! # Edit and save
//! write "iHello World"
//! assert_screen "Hello World"
//! write "\\x1b:wq\\n"
//! wait 1s
//! ```

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::time::Duration;

/// A parsed DSL script containing a sequence of commands.
#[derive(Debug, Clone)]
pub struct Script {
    pub commands: Vec<Command>,
}

/// Individual DSL commands.
#[derive(Debug, Clone)]
pub enum Command {
    /// Wait for a specific duration
    Wait { duration: Duration },
    /// Write input to the session
    Write { content: String },
    /// Wait for a pattern to appear in output
    WaitFor { pattern: String, timeout: Duration },
    /// Assert that screen contains expected content
    AssertScreen { expected: String },
    /// Clear the output buffer
    ClearBuffer,
    /// Press a special key
    Key { key: SpecialKey },
}

/// Special keys that can be sent.
#[derive(Debug, Clone)]
pub enum SpecialKey {
    Enter,
    Escape,
    Tab,
    Backspace,
    CtrlC,
    CtrlD,
}

impl Script {
    /// Load a script from a file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read script file: {:?}", path.as_ref()))?;
        Self::parse(&content)
    }

    /// Parse a script from a string.
    pub fn parse(input: &str) -> Result<Self> {
        let mut commands = Vec::new();

        for (line_num, line) in input.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let cmd = Self::parse_line(line)
                .with_context(|| format!("Parse error at line {}: {}", line_num + 1, line))?;
            commands.push(cmd);
        }

        Ok(Script { commands })
    }

    fn parse_line(line: &str) -> Result<Command> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            bail!("Empty command");
        }

        match parts[0] {
            "wait" => {
                if parts.len() < 2 {
                    bail!("wait requires a duration argument");
                }
                let duration = parse_duration(parts[1])?;
                Ok(Command::Wait { duration })
            }

            "write" => {
                if parts.len() < 2 {
                    bail!("write requires a content argument");
                }
                // Join remaining parts to handle quoted strings
                let rest = &line["write".len()..].trim();
                let content = parse_quoted_string(rest)?;
                let content = unescape(&content);
                Ok(Command::Write { content })
            }

            "wait_for" => {
                if parts.len() < 2 {
                    bail!("wait_for requires a pattern argument");
                }
                let (pattern, timeout) = if let Some(timeout_idx) = find_timeout(&parts) {
                    let pattern_str = parts[1..timeout_idx].join(" ");
                    let pattern = parse_quoted_string(&pattern_str)?;
                    let timeout = parse_duration(parts[timeout_idx])?;
                    (pattern, timeout)
                } else {
                    let pattern_str = parts[1..].join(" ");
                    let pattern = parse_quoted_string(&pattern_str)?;
                    (pattern, Duration::from_secs(5)) // default timeout
                };
                let pattern = unescape(&pattern);
                Ok(Command::WaitFor { pattern, timeout })
            }

            "assert_screen" => {
                if parts.len() < 2 {
                    bail!("assert_screen requires an expected content argument");
                }
                let rest = &line["assert_screen".len()..].trim();
                let expected = parse_quoted_string(rest)?;
                let expected = unescape(&expected);
                Ok(Command::AssertScreen { expected })
            }

            "clear_buffer" => Ok(Command::ClearBuffer),

            "key" => {
                if parts.len() < 2 {
                    bail!("key requires a key name argument");
                }
                let key = match parts[1].to_lowercase().as_str() {
                    "enter" | "return" => SpecialKey::Enter,
                    "esc" | "escape" => SpecialKey::Escape,
                    "tab" => SpecialKey::Tab,
                    "backspace" | "bs" => SpecialKey::Backspace,
                    "ctrl-c" | "ctrlc" => SpecialKey::CtrlC,
                    "ctrl-d" | "ctrld" => SpecialKey::CtrlD,
                    _ => bail!("Unknown key: {}", parts[1]),
                };
                Ok(Command::Key { key })
            }

            _ => bail!("Unknown command: {}", parts[0]),
        }
    }
}

/// Parse a duration string like "5s", "500ms", "1m"
fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();

    if let Some(millis) = s.strip_suffix("ms") {
        let millis: u64 = millis.parse().context("Invalid milliseconds")?;
        return Ok(Duration::from_millis(millis));
    }

    if let Some(secs) = s.strip_suffix('s') {
        let secs: u64 = secs.parse().context("Invalid seconds")?;
        return Ok(Duration::from_secs(secs));
    }

    if let Some(mins) = s.strip_suffix('m') {
        let mins: u64 = mins.parse().context("Invalid minutes")?;
        return Ok(Duration::from_secs(mins * 60));
    }

    // Try parsing as plain milliseconds
    let millis: u64 = s.parse().context("Invalid duration format, expected: <number>[s|ms|m]")?;
    Ok(Duration::from_millis(millis))
}

/// Parse a quoted or unquoted string.
fn parse_quoted_string(s: &str) -> Result<String> {
    let s = s.trim();

    if s.starts_with('"') && s.ends_with('"') && s.len() > 1 {
        // Double-quoted string
        Ok(s[1..s.len() - 1].to_string())
    } else if s.starts_with('\'') && s.ends_with('\'') && s.len() > 1 {
        // Single-quoted string
        Ok(s[1..s.len() - 1].to_string())
    } else {
        // Unquoted string (take the whole thing)
        Ok(s.to_string())
    }
}

/// Unescape special characters like \n, \t, \x1b, etc.
fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('\'') => result.push('\''),
                Some('x') => {
                    // Hex escape: \xNN
                    let hex: String = chars.by_ref().take(2).collect();
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                    }
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Find the index of a timeout argument in parts.
fn find_timeout(parts: &[&str]) -> Option<usize> {
    for (i, part) in parts.iter().enumerate().skip(2) {
        if part.ends_with('s') || part.ends_with("ms") || part.ends_with('m') {
            if parse_duration(part).is_ok() {
                return Some(i);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("1m").unwrap(), Duration::from_secs(60));
    }

    #[test]
    fn test_unescape() {
        assert_eq!(unescape("hello\\nworld"), "hello\nworld");
        assert_eq!(unescape("tab\\there"), "tab\there");
        assert_eq!(unescape("\\x1b"), "\x1b");
    }

    #[test]
    fn test_parse_script() {
        let script = r#"
# This is a comment
wait 500ms
write "hello world"
wait_for "$" 5s
assert_screen "prompt"
clear_buffer
"#;

        let parsed = Script::parse(script).unwrap();
        assert_eq!(parsed.commands.len(), 5);
    }

    #[test]
    fn test_parse_quoted_string() {
        assert_eq!(parse_quoted_string("\"hello world\"").unwrap(), "hello world");
        assert_eq!(parse_quoted_string("'hello world'").unwrap(), "hello world");
        assert_eq!(parse_quoted_string("hello").unwrap(), "hello");
    }
}
