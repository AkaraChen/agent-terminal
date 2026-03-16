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
                let ch = cell.map(|c| c.contents()).unwrap_or_else(|| " ".to_string());
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
