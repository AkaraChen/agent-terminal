use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Maximum raw bytes to retain before pruning the front of the buffer.
const MAX_RAW_BYTES: usize = 1024 * 1024; // 1 MB

/// Holds captured PTY output.
///
/// `raw` stores the byte stream for forensic access; trimmed when it
/// exceeds 1 MB.  `parser` maintains a VT100 screen state so callers
/// can retrieve the rendered "current screen" at any time.
pub struct OutputBuffer {
    raw: Vec<u8>,
    parser: vt100::Parser,
}

impl OutputBuffer {
    pub fn new(rows: u16, cols: u16) -> Self {
        OutputBuffer {
            raw: Vec::new(),
            parser: vt100::Parser::new(rows, cols, 0),
        }
    }

    /// Push new PTY bytes into both the raw buffer and the VT100 parser.
    pub fn push(&mut self, data: &[u8]) {
        self.raw.extend_from_slice(data);
        self.parser.process(data);

        // Trim front of raw buffer if over limit.
        if self.raw.len() > MAX_RAW_BYTES {
            let excess = self.raw.len() - MAX_RAW_BYTES;
            self.raw.drain(..excess);
        }
    }

    /// Return raw bytes as a base64-encoded string.
    pub fn raw_b64(&self) -> String {
        STANDARD.encode(&self.raw)
    }

    /// Resize the VT100 parser to new dimensions.
    /// This creates a new parser and replays the current raw buffer.
    /// Note: replaying raw buffer will lose some ANSI state (like previous clear-screen commands),
    /// but it's the best we can do since vt100 library doesn't support resize.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let mut new_parser = vt100::Parser::new(rows, cols, 0);
        // Replay the raw buffer into the new parser
        new_parser.process(&self.raw);
        self.parser = new_parser;
    }

    /// Return a plain-text rendering of the current VT100 screen state.
    /// Each row is separated by '\n'; trailing spaces on each row are trimmed.
    pub fn screen_contents(&self) -> String {
        let screen = self.parser.screen();
        let rows = screen.size().0;
        let cols = screen.size().1;
        let mut lines: Vec<String> = Vec::with_capacity(rows as usize);
        for row in 0..rows {
            let mut line = String::with_capacity(cols as usize);
            for col in 0..cols {
                let cell = screen.cell(row, col);
                let ch = cell
                    .map(|c| c.contents())
                    .unwrap_or_else(|| " ".to_string());
                if ch.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(&ch);
                }
            }
            lines.push(line.trim_end().to_string());
        }
        // Drop trailing empty lines.
        while lines.last().map(|l: &String| l.is_empty()).unwrap_or(false) {
            lines.pop();
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    #[test]
    fn test_new_empty_buffer() {
        let buf = OutputBuffer::new(24, 80);
        assert_eq!(buf.raw_b64(), STANDARD.encode(b""));
        assert_eq!(buf.screen_contents(), "");
    }

    #[test]
    fn test_push_raw_bytes_roundtrip() {
        let mut buf = OutputBuffer::new(24, 80);
        buf.push(b"hello");
        let decoded = STANDARD.decode(buf.raw_b64()).unwrap();
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn test_push_accumulates_raw_bytes() {
        let mut buf = OutputBuffer::new(24, 80);
        buf.push(b"foo");
        buf.push(b"bar");
        let decoded = STANDARD.decode(buf.raw_b64()).unwrap();
        assert_eq!(decoded, b"foobar");
    }

    #[test]
    fn test_push_empty_slice_is_noop() {
        let mut buf = OutputBuffer::new(24, 80);
        buf.push(b"");
        assert_eq!(buf.raw_b64(), STANDARD.encode(b""));
        assert_eq!(buf.screen_contents(), "");
    }

    #[test]
    fn test_screen_contents_plain_text() {
        let mut buf = OutputBuffer::new(24, 80);
        buf.push(b"hello world");
        let screen = buf.screen_contents();
        assert!(screen.contains("hello world"));
    }

    #[test]
    fn test_screen_contents_no_trailing_newline() {
        let mut buf = OutputBuffer::new(24, 80);
        buf.push(b"hi");
        let screen = buf.screen_contents();
        // Trailing empty rows should be dropped; result should not end with \n
        assert!(!screen.ends_with('\n'));
    }

    #[test]
    fn test_screen_contents_multiline() {
        let mut buf = OutputBuffer::new(24, 80);
        // Write two lines separated by CR+LF (PTY convention)
        buf.push(b"line1\r\nline2");
        let screen = buf.screen_contents();
        assert!(screen.contains("line1"));
        assert!(screen.contains("line2"));
    }

    #[test]
    fn test_screen_contents_trailing_spaces_trimmed() {
        let mut buf = OutputBuffer::new(24, 80);
        // vt100 fills unused cells with spaces; screen_contents must trim them.
        buf.push(b"x");
        let screen = buf.screen_contents();
        // The first (and only) row should be just "x", not "x" padded to 80 chars.
        let first_line = screen.lines().next().unwrap_or("");
        assert_eq!(first_line, "x");
    }

    #[test]
    fn test_1mb_trim_keeps_at_most_1mb() {
        let mut buf = OutputBuffer::new(24, 80);
        // Push 1.5 MB total
        let chunk = vec![b'A'; 512 * 1024]; // 512 KB
        buf.push(&chunk);
        buf.push(&chunk);
        buf.push(&chunk);
        let raw_bytes = STANDARD.decode(buf.raw_b64()).unwrap();
        assert!(
            raw_bytes.len() <= 1024 * 1024,
            "raw buffer must not exceed 1 MB"
        );
    }

    #[test]
    fn test_1mb_trim_retains_most_recent_data() {
        let mut buf = OutputBuffer::new(24, 80);
        // Push 1 MB of 'A' then another 512 KB of 'B'
        let ones = vec![b'A'; 1024 * 1024];
        let twos = vec![b'B'; 512 * 1024];
        buf.push(&ones);
        buf.push(&twos);
        let raw_bytes = STANDARD.decode(buf.raw_b64()).unwrap();
        // The tail should be the 'B' data
        let tail = &raw_bytes[raw_bytes.len() - 512 * 1024..];
        assert!(tail.iter().all(|&b| b == b'B'));
    }

    #[test]
    fn test_raw_b64_is_valid_base64() {
        let mut buf = OutputBuffer::new(24, 80);
        buf.push(b"\x00\x01\x02\xff\xfe");
        let b64 = buf.raw_b64();
        assert!(STANDARD.decode(&b64).is_ok());
    }

    #[test]
    fn test_resize_changes_dimensions() {
        let mut buf = OutputBuffer::new(24, 80);
        buf.push(b"hello world\r\nline 2");

        // Resize to smaller dimensions
        buf.resize(10, 40);

        // Content should still be accessible
        let screen = buf.screen_contents();
        assert!(screen.contains("hello") || screen.contains("world"));
    }

    #[test]
    fn test_resize_preserves_raw_bytes() {
        let mut buf = OutputBuffer::new(24, 80);
        buf.push(b"test data 123");

        let raw_before = buf.raw_b64();
        buf.resize(30, 100);
        let raw_after = buf.raw_b64();

        // Raw bytes should be preserved after resize
        assert_eq!(raw_before, raw_after);
    }

    #[test]
    fn test_resize_empty_buffer() {
        let mut buf = OutputBuffer::new(24, 80);
        // Resize without any data
        buf.resize(10, 40);
        assert_eq!(buf.screen_contents(), "");
        assert_eq!(buf.raw_b64(), STANDARD.encode(b""));
    }
}
