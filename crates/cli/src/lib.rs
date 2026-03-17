pub mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "agent-terminal",
    about = "CLI testing framework with PTY session management"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an interactive PTY session that can be controlled remotely.
    Start {
        /// Shell to use (default: platform default - /bin/zsh on macOS, /bin/bash on Linux)
        #[arg(short, long)]
        shell: Option<String>,
    },
    /// List all active PTY sessions.
    List,
    /// Write input to a running session.
    Write {
        /// Session ID (or unique prefix) to target.
        session_id: String,
        /// Data to send (e.g. "ls\n").
        data: String,
    },
    /// Dump the current screen content of a running session.
    Dump {
        /// Session ID (or unique prefix) to target.
        session_id: String,
    },
    /// Debug a running session with advanced analysis options.
    Debug {
        /// Session ID (or unique prefix) to target.
        session_id: String,
        /// Show raw ANSI bytes (base64 encoded).
        #[arg(short, long)]
        raw: bool,
        /// Watch mode - continuously update.
        #[arg(short, long)]
        watch: bool,
        /// Analyze ANSI sequences and screen state.
        #[arg(short, long)]
        analyze: bool,
    },
    /// Run a DSL test command against a session.
    Test {
        /// Session ID (or unique prefix) to target.
        session_id: String,
        #[command(subcommand)]
        action: TestAction,
    },
    /// Connect to a remote session over TCP.
    Remote {
        /// TCP address (e.g., "192.168.1.100:8080").
        addr: String,
        /// Authentication token.
        #[arg(short, long)]
        token: String,
        #[command(subcommand)]
        action: RemoteAction,
    },
}

#[derive(Subcommand)]
pub enum RemoteAction {
    /// Write input to the remote session.
    Write { data: String },
    /// Get current output from the remote session.
    Dump,
}

#[derive(Subcommand)]
pub enum TestAction {
    /// Wait for a pattern to appear in the output.
    WaitFor {
        /// Pattern to wait for.
        pattern: String,
        /// Timeout in seconds (default: 5).
        #[arg(short, long)]
        timeout: Option<u64>,
    },
    /// Assert that the screen contains the given text.
    AssertContains {
        /// Text that should be present on screen.
        text: String,
    },
    /// Write input to the session.
    Write {
        /// Data to send (e.g. "ls\n").
        data: String,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    match cli.command {
        Commands::Start { shell } => rt.block_on(commands::start::run(shell.as_deref())),
        Commands::List => commands::list::run(),
        Commands::Write { session_id, data } => {
            rt.block_on(commands::write::run(&session_id, &data))
        }
        Commands::Dump { session_id } => rt.block_on(commands::dump::run(&session_id)),
        Commands::Debug {
            session_id,
            raw,
            watch,
            analyze,
        } => rt.block_on(commands::debug::run(&session_id, raw, watch, analyze)),
        Commands::Test { session_id, action } => {
            rt.block_on(commands::test::run(&session_id, action))
        }
        Commands::Remote {
            addr,
            token,
            action,
        } => rt.block_on(commands::remote::run(&addr, &token, action)),
    }
}
