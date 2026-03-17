//! DSL command runner for executing scripts.

use agent_terminal_core::dsl::TestRunner;
use anyhow::{Context, Result};
use std::time::Duration;

use crate::script::{Command, SpecialKey};

/// Builder for constructing DSL command sequences fluently.
pub struct DslBuilder {
    commands: Vec<Command>,
}

impl DslBuilder {
    /// Create a new DSL builder.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Add a wait command.
    pub fn wait(mut self, duration: Duration) -> Self {
        self.commands.push(Command::Wait { duration });
        self
    }

    /// Add a write command.
    pub fn write<S: Into<String>>(mut self, content: S) -> Self {
        self.commands.push(Command::Write {
            content: content.into(),
        });
        self
    }

    /// Add a wait_for command.
    pub fn wait_for<S: Into<String>>(mut self, pattern: S, timeout: Duration) -> Self {
        self.commands.push(Command::WaitFor {
            pattern: pattern.into(),
            timeout,
        });
        self
    }

    /// Add an assert_screen command.
    pub fn assert_screen_contains<S: Into<String>>(mut self, expected: S) -> Self {
        self.commands.push(Command::AssertScreen {
            expected: expected.into(),
        });
        self
    }

    /// Add a clear_buffer command.
    pub fn clear_buffer(mut self) -> Self {
        self.commands.push(Command::ClearBuffer);
        self
    }

    /// Add a key press command.
    pub fn key(mut self, key: SpecialKey) -> Self {
        self.commands.push(Command::Key { key });
        self
    }

    /// Convenience method to press Enter.
    pub fn enter(self) -> Self {
        self.key(SpecialKey::Enter)
    }

    /// Convenience method to press Escape.
    pub fn esc(self) -> Self {
        self.key(SpecialKey::Escape)
    }

    /// Convenience method to wait for shell prompt.
    pub fn wait_for_shell(self) -> Self {
        self.wait_for("$", Duration::from_secs(5))
    }

    /// Get the collected commands.
    pub fn into_commands(self) -> Vec<Command> {
        self.commands
    }

    /// Run all commands against a TestRunner.
    pub async fn run(self, runner: &mut TestRunner) -> Result<()> {
        execute_commands(runner, &self.commands).await
    }
}

impl Default for DslBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute a list of commands against a TestRunner.
pub async fn execute_commands(runner: &mut TestRunner, commands: &[Command]) -> Result<()> {
    let total = commands.len();
    for (i, cmd) in commands.iter().enumerate() {
        print_command(i + 1, total, cmd);
        execute_command(runner, cmd)
            .await
            .with_context(|| format!("Command {} failed: {:?}", i + 1, cmd))?;
        println!(" ✓");
    }
    Ok(())
}

/// Print command information to stdout.
fn print_command(current: usize, total: usize, cmd: &Command) {
    match cmd {
        Command::Wait { duration } => {
            let millis = duration.as_millis();
            if millis >= 1000 {
                print!("[{}/{}] wait {}s", current, total, duration.as_secs());
            } else {
                print!("[{}/{}] wait {}ms", current, total, millis);
            }
        }
        Command::Write { content } => {
            let display = format_content(content);
            print!("[{}/{}] write {}", current, total, display);
        }
        Command::WaitFor { pattern, timeout } => {
            print!(
                "[{}/{}] wait for '{}' (timeout: {}s)",
                current,
                total,
                format_content(pattern),
                timeout.as_secs()
            );
        }
        Command::AssertScreen { expected } => {
            print!(
                "[{}/{}] assert screen contains '{}'",
                current,
                total,
                format_content(expected)
            );
        }
        Command::ClearBuffer => {
            print!("[{}/{}] clear buffer", current, total);
        }
        Command::Key { key } => {
            let key_name = match key {
                SpecialKey::Enter => "Enter",
                SpecialKey::Escape => "Escape",
                SpecialKey::Tab => "Tab",
                SpecialKey::Backspace => "Backspace",
                SpecialKey::CtrlC => "Ctrl+C",
                SpecialKey::CtrlD => "Ctrl+D",
            };
            print!("[{}/{}] key {}", current, total, key_name);
        }
    }
    std::io::Write::flush(&mut std::io::stdout()).ok();
}

/// Format content for display, truncating if too long and showing special chars.
fn format_content(s: &str) -> String {
    let max_len = 40;
    let escaped = s
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('\x1b', "\\x1b")
        .replace('\x7f', "\\x7f")
        .replace('\x03', "\\x03")
        .replace('\x04', "\\x04");

    if escaped.len() > max_len {
        format!("'{}...'", &escaped[..max_len])
    } else {
        format!("'{}'", escaped)
    }
}

/// Execute a single command.
async fn execute_command(runner: &mut TestRunner, cmd: &Command) -> Result<()> {
    use tokio::time::sleep;

    match cmd {
        Command::Wait { duration } => {
            sleep(*duration).await;
        }

        Command::Write { content } => {
            runner.write_input(content).await?;
        }

        Command::WaitFor { pattern, timeout } => {
            runner.wait_for(pattern, *timeout).await?;
        }

        Command::AssertScreen { expected } => {
            runner.assert_screen_contains(expected).await?;
        }

        Command::ClearBuffer => {
            runner.clear_buffer();
        }

        Command::Key { key } => {
            let key_seq = match key {
                SpecialKey::Enter => "\n",
                SpecialKey::Escape => "\x1b",
                SpecialKey::Tab => "\t",
                SpecialKey::Backspace => "\x7f",
                SpecialKey::CtrlC => "\x03",
                SpecialKey::CtrlD => "\x04",
            };
            runner.write_input(key_seq).await?;
        }
    }

    Ok(())
}
