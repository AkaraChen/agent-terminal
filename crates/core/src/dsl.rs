//! Test case DSL support.
//!
//! Provides functionality to write test scripts that interact with PTY sessions:
//! - wait_for(pattern, timeout): Wait for a pattern to appear in output
//! - assert_screen: Assert on screen contents
//!
//! Example usage:
//! ```ignore
//! let mut runner = TestRunner::connect("/tmp/session.sock").await?;
//! runner.wait_for("$ ", Duration::from_secs(5)).await?;
//! runner.write_input("echo hello\n").await?;
//! runner.wait_for("hello", Duration::from_secs(2)).await?;
//! runner.assert_screen_contains("hello")?;
//! ```

use crate::ipc::{read_frame, IpcClient};
use crate::protocol::{Request, Response};
use anyhow::{bail, Context, Result};
use std::time::Duration;
use tokio::time::timeout;

/// A test runner that can execute DSL operations against a session.
pub struct TestRunner {
    client: IpcClient,
    /// Accumulated output for pattern matching
    output_buffer: String,
}

impl TestRunner {
    /// Connect to a session and start a test runner.
    pub async fn connect(socket_path: &str) -> Result<Self> {
        let client = IpcClient::connect(socket_path).await?;
        Ok(TestRunner {
            client,
            output_buffer: String::new(),
        })
    }

    /// Subscribe to output stream for real-time pattern matching.
    pub async fn subscribe(&mut self) -> Result<()> {
        let resp = self
            .client
            .send(&Request::Subscribe)
            .await
            .context("subscribe to output")?;
        match resp {
            Response::Ok => Ok(()),
            Response::Error { message } => bail!("subscribe failed: {}", message),
            _ => bail!("unexpected response to subscribe"),
        }
    }

    /// Unsubscribe from output stream.
    pub async fn unsubscribe(&mut self) -> Result<()> {
        let resp = self
            .client
            .send(&Request::Unsubscribe)
            .await
            .context("unsubscribe from output")?;
        match resp {
            Response::Ok => Ok(()),
            Response::Error { message } => bail!("unsubscribe failed: {}", message),
            _ => bail!("unexpected response to unsubscribe"),
        }
    }

    /// Write input to the session.
    pub async fn write_input(&mut self, data: &str) -> Result<()> {
        self.client.write_input(data).await
    }

    /// Wait for a pattern to appear in the output stream.
    ///
    /// This method subscribes to output, accumulates chunks, and checks
    /// for the pattern. Returns when the pattern is found or timeout.
    ///
    /// # Arguments
    ///
    /// * `pattern` - The string pattern to wait for
    /// * `timeout_duration` - Maximum time to wait
    pub async fn wait_for(&mut self, pattern: &str, timeout_duration: Duration) -> Result<()> {
        // First check if pattern is already in accumulated buffer
        if self.output_buffer.contains(pattern) {
            return Ok(());
        }

        // Get current output as baseline
        let (_raw_b64, screen) = self.client.get_output().await?;
        self.output_buffer = screen;

        if self.output_buffer.contains(pattern) {
            return Ok(());
        }

        // Subscribe to streaming output
        self.subscribe().await?;

        // Wait for pattern with timeout
        let result = timeout(timeout_duration, async {
            loop {
                match read_frame::<Response>(&mut self.client.stream).await {
                    Ok(Response::OutputChunk { raw_b64 }) => {
                        // Decode and append to buffer
                        use base64::Engine;
                        if let Ok(bytes) =
                            base64::engine::general_purpose::STANDARD.decode(&raw_b64)
                        {
                            if let Ok(text) = String::from_utf8(bytes) {
                                self.output_buffer.push_str(&text);
                                if self.output_buffer.contains(pattern) {
                                    return Ok(());
                                }
                            }
                        }
                    }
                    Ok(Response::Ok) => continue, // Response to our write
                    Ok(Response::Error { message }) => {
                        return Err(anyhow::anyhow!("stream error: {}", message))
                    }
                    Ok(_) => continue,
                    Err(e) => return Err(e.into()),
                }
            }
        })
        .await;

        // Always unsubscribe before returning
        let _ = self.unsubscribe().await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => bail!(
                "timeout waiting for pattern '{}' after {:?}",
                pattern,
                timeout_duration
            ),
        }
    }

    /// Get the current screen contents.
    pub async fn get_screen(&mut self) -> Result<String> {
        let (_, screen) = self.client.get_output().await?;
        Ok(screen)
    }

    /// Assert that the screen contains the given substring.
    pub async fn assert_screen_contains(&mut self, expected: &str) -> Result<()> {
        let screen = self.get_screen().await?;
        if !screen.contains(expected) {
            bail!(
                "assert_screen_contains failed: expected '{}' not found in screen:\n{}",
                expected,
                screen
            );
        }
        Ok(())
    }

    /// Get the accumulated output buffer content.
    pub fn output_buffer(&self) -> &str {
        &self.output_buffer
    }

    /// Clear the output buffer.
    pub fn clear_buffer(&mut self) {
        self.output_buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_buffer_accumulation() {
        // This is a basic unit test for the buffer logic
        // Full integration tests would require a mock session
        let mut buffer = String::new();
        buffer.push_str("hello world");
        assert!(buffer.contains("world"));
        assert!(!buffer.contains("foo"));
    }

    #[test]
    fn test_buffer_clear() {
        let mut buffer = String::new();
        buffer.push_str("content");
        buffer.clear();
        assert!(buffer.is_empty());
    }
}
