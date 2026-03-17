//! Agent Terminal E2E Testing Framework
//!
//! This crate provides a high-level DSL for writing end-to-end tests
//! for terminal applications. It supports both pure text scripts (.atdsl)
//! and a fluent Rust API.
//!
//! # Pure Text DSL
//!
//! Create a `.atdsl` file:
//!
//! ```atdsl
//! # Wait for shell prompt
//! wait_for "$" 5s
//!
//! # Start vim and edit
//! write "vim test.txt\n"
//! wait_for "~" 5s
//! write "iHello World"
//! assert_screen "Hello World"
//! write "\x1b:wq\n"
//! wait 1s
//! ```
//!
//! Run it:
//!
//! ```rust,ignore
//! use agent_terminal_e2e::Session;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     Session::new().await?
//!         .run_script_file("test.atdsl").await?;
//!     Ok(())
//! }
//! ```
//!
//! # Fluent Rust API
//!
//! ```rust,ignore
//! use agent_terminal_e2e::Session;
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let session = Session::new().await?;
//!
//!     session.dsl()
//!         .wait_for("$", Duration::from_secs(5))
//!         .write("echo hello\n")
//!         .wait_for("hello", Duration::from_secs(2))
//!         .assert_screen_contains("hello")
//!         .run(&mut session.connect().await?)
//!         .await?;
//!
//!     Ok(())
//! }
//! ```

pub mod runner;
pub mod script;
pub mod session;

// Re-export commonly used items
pub use runner::DslBuilder;
pub use script::{Command, Script, SpecialKey};
pub use session::Session;
