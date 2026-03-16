pub mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agent-terminal", about = "CLI testing framework with PTY session management")]
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
    }
}
