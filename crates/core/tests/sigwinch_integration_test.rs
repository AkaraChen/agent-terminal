use agent_terminal_core::buffer::OutputBuffer;

mod common;

/// Test that OutputBuffer resize works correctly
#[test]
fn test_buffer_resize_basic() {
    let mut buf = OutputBuffer::new(24, 80);
    buf.push(b"line 1\r\nline 2\r\nline 3");

    // Resize to larger dimensions
    buf.resize(30, 100);

    // Content should still be accessible
    let screen = buf.screen_contents();
    assert!(screen.contains("line 1") || screen.contains("line 2") || screen.contains("line 3"));
}

/// Test that resize preserves all raw bytes
#[test]
fn test_buffer_resize_preserves_raw() {
    use base64::Engine;

    let mut buf = OutputBuffer::new(24, 80);
    let test_data = b"test data with escape \x1b[31mred\x1b[0m codes";
    buf.push(test_data);

    let raw_before = buf.raw_b64();
    buf.resize(10, 40);
    let raw_after = buf.raw_b64();

    // Raw bytes must be preserved exactly
    assert_eq!(raw_before, raw_after);

    // Verify we can decode it back
    let decoded = base64::engine::general_purpose::STANDARD.decode(&raw_after).unwrap();
    assert!(decoded.windows(test_data.len()).any(|w| w == test_data));
}

/// Test multiple consecutive resizes
#[test]
fn test_buffer_multiple_resizes() {
    let mut buf = OutputBuffer::new(24, 80);
    buf.push(b"content before resize");

    // Resize multiple times (avoid very small dimensions that may cause vt100 panics)
    buf.resize(10, 40);
    buf.resize(50, 200);
    buf.resize(5, 20);

    // Raw data should still be there
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD.decode(buf.raw_b64()).unwrap();
    assert!(decoded.windows(7).any(|w| w == b"content"));
}

/// Test resize with ANSI sequences
#[test]
fn test_buffer_resize_with_ansi() {
    let mut buf = OutputBuffer::new(24, 80);

    // Add content with ANSI escape sequences
    buf.push(b"\x1b[2J\x1b[H"); // Clear screen and move to home
    buf.push(b"\x1b[32mgreen text\x1b[0m");

    buf.resize(30, 100);

    // Raw data should be preserved - check for the escape sequence bytes
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD.decode(buf.raw_b64()).unwrap();
    // Check for ESC (0x1b) followed by '['
    assert!(decoded.windows(2).any(|w| w == b"\x1b["));
}

/// Test resize to smaller dimensions
#[test]
fn test_buffer_resize_to_smaller() {
    let mut buf = OutputBuffer::new(24, 80);
    buf.push(b"this is a test line that is longer than 20 columns");

    // Resize to smaller dimensions (avoid 1-row which may cause vt100 issues)
    buf.resize(5, 20);

    // The parser should handle it without panicking
    let _screen = buf.screen_contents();
    // We don't assert specific content because the screen will be truncated
}

/// Test resize of empty buffer
#[test]
fn test_buffer_resize_empty() {
    let mut buf = OutputBuffer::new(24, 80);

    // Resize empty buffer
    buf.resize(10, 40);

    // Should still be empty
    assert_eq!(buf.screen_contents(), "");
    use base64::Engine;
    assert_eq!(buf.raw_b64(), base64::engine::general_purpose::STANDARD.encode(b""));
}

/// Test that screen contents after resize reflects new dimensions
#[test]
fn test_buffer_resize_screen_dimensions() {
    let mut buf = OutputBuffer::new(24, 80);

    // Fill with enough content to span multiple lines
    for i in 0..50 {
        buf.push(format!("line {}\r\n", i).as_bytes());
    }

    // Resize to much smaller
    buf.resize(5, 20);

    // Screen should reflect new dimensions (only 5 rows visible)
    let screen = buf.screen_contents();
    let lines: Vec<_> = screen.lines().collect();

    // Should have at most 5 lines (the visible rows)
    assert!(lines.len() <= 5, "Screen should have at most 5 lines after resize, got {}", lines.len());
}
