//! Integration test for vim rendering in PTY
//!
//! This test spawns vim in a PTY and verifies that it renders content correctly.

use base64::Engine;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_terminal_core::buffer::OutputBuffer;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

/// Test that vim renders content in alternate screen buffer
#[test]
#[ignore = "Requires vim to be installed"]
fn test_vim_renders_in_pty() {
    // Create PTY
    let pty_system = NativePtySystem::default();
    let pty_size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pty_pair = pty_system.openpty(pty_size).expect("open PTY");

    // Create test file
    let test_content = "Hello from vim test\nLine 2\nLine 3\n";
    let test_file = "/tmp/agent_terminal_vim_test.txt";
    std::fs::write(test_file, test_content).expect("write test file");

    // Spawn vim
    let mut cmd = CommandBuilder::new("vim");
    cmd.arg(test_file);
    cmd.env("TERM", "xterm-256color");

    let mut child = pty_pair.slave.spawn_command(cmd).expect("spawn vim");
    drop(pty_pair.slave);

    // Read output
    let mut reader = pty_pair.master.try_clone_reader().unwrap();
    let buffer = Arc::new(Mutex::new(OutputBuffer::new(24, 80)));
    let buf_clone = Arc::clone(&buffer);

    // Spawn reader thread
    let reader_handle = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        let start = std::time::Instant::now();
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let data = &chunk[..n];
                    if let Ok(mut buf) = buf_clone.lock() {
                        buf.push(data);
                    }
                    // Check if we have vim content
                    if let Ok(buf) = buf_clone.lock() {
                        let raw = buf.raw_b64();
                        let decoded = base64::engine::general_purpose::STANDARD
                            .decode(&raw)
                            .unwrap();
                        if decoded.windows(6).any(|w| w == b"Hello ") {
                            println!("Found 'Hello' in output!");
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
            if start.elapsed() > Duration::from_secs(5) {
                println!("Timeout waiting for vim output");
                break;
            }
        }
    });

    // Wait for vim to start
    std::thread::sleep(Duration::from_secs(3));

    // Get writer
    let mut writer = pty_pair.master.take_writer().unwrap();

    // Try to quit vim
    writer.write_all(b"\x1b:q!\n").unwrap();
    writer.flush().unwrap();

    // Wait for reader to finish
    reader_handle.join().unwrap();

    // Get final output
    let final_buffer = buffer.lock().unwrap();
    let raw = final_buffer.raw_b64();
    let decoded = base64::engine::general_purpose::STANDARD.decode(&raw).unwrap();

    println!("Total bytes captured: {}", decoded.len());

    let decoded_str = String::from_utf8_lossy(&decoded);
    println!("Raw output (last 500 chars): {}", &decoded_str[decoded_str.len().saturating_sub(500)..]);

    // Check for alternate screen
    let has_1049h = decoded_str.contains("\x1b[?1049h");
    println!("Has 1049h (alternate screen): {}", has_1049h);

    // Check for content
    let has_hello = decoded_str.contains("Hello");
    println!("Has 'Hello': {}", has_hello);

    // Cleanup
    let _ = child.wait();
    let _ = std::fs::remove_file(test_file);

    // Assertions
    assert!(has_1049h, "vim should enter alternate screen");
    assert!(has_hello, "vim should render file content");
}

/// Test that we can capture vim's alternate screen buffer correctly
#[test]
fn test_vim_alternate_screen_simulation() {
    use agent_terminal_core::buffer::OutputBuffer;

    let mut buf = OutputBuffer::new(24, 80);

    // Simulate vim startup sequence more accurately
    // 1. Enter alternate screen first
    buf.push(b"\x1b[?1049h");

    // 2. Clear screen and position cursor at home
    buf.push(b"\x1b[H");
    buf.push(b"\x1b[2J");
    buf.push(b"\x1b[H");

    // 3. Draw line 1 with proper cursor positioning
    buf.push(b"\x1b[H"); // Row 1, Col 1
    buf.push(b"  1 Hello from vim test");

    // 4. Move to row 2 and draw
    buf.push(b"\x1b[2;1H"); // Row 2, Col 1
    buf.push(b"  2 Line 2");

    // 5. Move to row 3 and draw
    buf.push(b"\x1b[3;1H"); // Row 3, Col 1
    buf.push(b"  3 Line 3");

    // 6. Draw ~ for empty lines
    for row in 4..23 {
        buf.push(format!("\x1b[{};1H~", row).as_bytes());
    }

    // 7. Draw status line at bottom
    buf.push(b"\x1b[24;1H\"agent_terminal_vim_test.txt\" 3L, 34B");

    // 8. Position cursor at the end
    buf.push(b"\x1b[1;25H");

    // 9. Get screen contents
    let screen = buf.screen_contents();
    println!("Screen contents:\n{}", screen);

    // Verify content is visible
    assert!(screen.contains("Hello from vim test"), "Screen should show file content, got:\n{}", screen);
    assert!(screen.contains("Line 2"), "Screen should show line 2");
    assert!(screen.contains("Line 3"), "Screen should show line 3");
}
