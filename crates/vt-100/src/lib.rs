//! VT100 terminal emulator wrapper using wezterm-term
//!
//! This crate provides a simplified interface to wezterm-term
//! for capturing and rendering terminal screen state.

use std::io::{self, Write};
use std::sync::Arc;
use termwiz::surface::Line;
use wezterm_term::color::ColorPalette;
use wezterm_term::terminal::{Clipboard, ClipboardSelection, Terminal};
use wezterm_term::{TerminalConfiguration, TerminalSize};

/// Configuration for the terminal emulator
#[derive(Debug)]
struct Config;

impl TerminalConfiguration for Config {
    fn generation(&self) -> usize {
        0
    }

    fn scrollback_size(&self) -> usize {
        10_000
    }

    fn enable_csi_u_key_encoding(&self) -> bool {
        true
    }

    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

/// Dummy writer that discards all output
struct DummyWriter;

impl Write for DummyWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Dummy clipboard implementation
#[derive(Debug)]
struct DummyClipboard;

impl Clipboard for DummyClipboard {
    fn set_contents(
        &self,
        _selection: ClipboardSelection,
        _data: Option<String>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Holds captured PTY output and maintains terminal screen state
pub struct Parser {
    terminal: Terminal,
    rows: usize,
    cols: usize,
}

impl Parser {
    /// Create a new parser with the given screen dimensions
    pub fn new(rows: u16, cols: u16) -> Self {
        let size = TerminalSize {
            rows: rows as usize,
            cols: cols as usize,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 96,
        };

        let config: Arc<dyn TerminalConfiguration + Send + Sync> = Arc::new(Config);
        let terminal = Terminal::new(
            size,
            config,
            "vt-100",
            "0.1.0",
            Box::new(DummyWriter),
        );

        Self {
            terminal,
            rows: rows as usize,
            cols: cols as usize,
        }
    }

    /// Process input bytes and update terminal state
    pub fn process(&mut self, data: &[u8]) {
        self.terminal.advance_bytes(data);
    }

    /// Get the current screen contents as plain text
    pub fn screen_contents(&self) -> String {
        let screen = self.terminal.screen();
        let mut lines: Vec<String> = Vec::new();

        // Get all lines in the visible screen
        for line in screen.lines_in_phys_range(0..screen.physical_rows) {
            let text = line_to_string(&line);
            lines.push(text);
        }

        // Trim trailing empty lines
        while let Some(last) = lines.last() {
            if last.trim().is_empty() {
                lines.pop();
            } else {
                break;
            }
        }

        lines.join("\n")
    }

    /// Get the number of rows
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Get the number of columns
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Resize the terminal to new dimensions
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.rows = rows as usize;
        self.cols = cols as usize;

        let size = TerminalSize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
            dpi: 96,
        };

        self.terminal.resize(size);
    }
}

/// Convert a Line to a String, extracting text content
fn line_to_string(line: &Line) -> String {
    let mut result = String::new();

    // Iterate through visible cells in the line
    for cell in line.visible_cells() {
        result.push_str(cell.str());
    }

    // Trim trailing whitespace
    result.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_parser() {
        let parser = Parser::new(24, 80);
        assert_eq!(parser.rows(), 24);
        assert_eq!(parser.cols(), 80);
    }

    #[test]
    fn test_process_simple_text() {
        let mut parser = Parser::new(24, 80);
        parser.process(b"hello world");
        let screen = parser.screen_contents();
        assert!(screen.contains("hello world"));
    }

    #[test]
    fn test_process_with_newline() {
        let mut parser = Parser::new(24, 80);
        parser.process(b"line1\r\nline2");
        let screen = parser.screen_contents();
        assert!(screen.contains("line1"));
        assert!(screen.contains("line2"));
    }

    #[test]
    fn test_trailing_empty_lines_trimmed() {
        let mut parser = Parser::new(10, 40);
        parser.process(b"hello");
        let screen = parser.screen_contents();
        assert!(!screen.ends_with('\n'));
    }

    #[test]
    fn test_resize() {
        let mut parser = Parser::new(24, 80);
        parser.process(b"test content");
        parser.resize(30, 100);
        assert_eq!(parser.rows(), 30);
        assert_eq!(parser.cols(), 100);
    }

    #[test]
    fn test_alternate_screen() {
        let mut parser = Parser::new(24, 80);

        // 写入普通内容
        parser.process(b"primary screen");
        let screen = parser.screen_contents();
        assert!(screen.contains("primary screen"), "Primary screen should contain text");

        // 切换到 alternate screen
        parser.process(b"\x1b[?1049h");
        parser.process(b"alternate content");
        let screen = parser.screen_contents();
        assert!(screen.contains("alternate content"), "Alternate screen should contain text, got: {}", screen);

        // 退出 alternate screen
        parser.process(b"\x1b[?1049l");
        let screen = parser.screen_contents();
        assert!(screen.contains("primary screen"), "Should return to primary screen, got: {}", screen);
    }

    #[test]
    fn test_echo_hello_sequence() {
        // Test the exact sequence seen in Python tests
        let mut parser = Parser::new(24, 80);

        // Simulate bash prompt and echo command
        parser.process(b"\x1b[?1034h"); // Enable 8-bit meta mode
        parser.process(b"bash-3.2$  ");
        parser.process(b"\r"); // CR
        parser.process(b"echo HELLO ");
        parser.process(b"\r"); // CR
        parser.process(b"\x1b[A"); // Cursor up
        parser.process(b"\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C\x1b[C"); // Cursor forward
        parser.process(b"\x1b[K"); // Clear to end of line
        parser.process(b"\r\n"); // Newline
        parser.process(b"HELLO\r\n");

        let screen = parser.screen_contents();
        println!("Screen: {:?}", screen);

        // The screen should contain HELLO somewhere
        assert!(screen.contains("HELLO"), "Screen should contain HELLO, got: {:?}", screen);
    }

    #[test]
    fn test_scrollback_behavior() {
        // Test that content stays visible after processing
        let mut parser = Parser::new(24, 80);

        // Fill screen with lines
        for i in 0..30 {
            parser.process(format!("Line {}\r\n", i).as_bytes());
        }

        let screen = parser.screen_contents();

        // Should see the last lines (screen holds 24 rows)
        assert!(screen.contains("Line 29"), "Should see last line, got: {}", screen);
    }
}
