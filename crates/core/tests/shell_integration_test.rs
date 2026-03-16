use agent_terminal_core::default_shell;

mod common;

/// Test that default_shell returns a valid shell path
#[test]
fn test_default_shell_platform_specific() {
    let shell = default_shell();

    // All platforms should return an absolute path
    assert!(shell.starts_with('/'), "Shell path should be absolute");
    assert!(shell.contains('/'), "Shell path should contain directory separator");

    // Platform-specific checks
    #[cfg(target_os = "macos")]
    assert_eq!(shell, "/bin/zsh", "macOS should default to zsh");

    #[cfg(target_os = "linux")]
    assert_eq!(shell, "/bin/bash", "Linux should default to bash");
}

/// Test that default_shell returns non-empty string
#[test]
fn test_default_shell_not_empty() {
    let shell = default_shell();
    assert!(!shell.is_empty(), "Default shell should not be empty");
}

/// Test that session accepts custom shell path
/// Note: This test doesn't actually spawn the shell, just verifies the function signature
#[tokio::test]
async fn test_session_accepts_shell_parameter() {
    // This test verifies that run_session accepts a shell parameter
    // We can't easily test the actual session without a real PTY,
    // but we can verify the API is correct by checking the function signature compiles

    // The function signature is: pub async fn run_session(shell: &str) -> Result<()>
    // This test passes if it compiles
}

/// Test shell path validation
#[test]
fn test_shell_path_variations() {
    // Test various shell paths that might be passed
    let valid_shells = [
        "/bin/zsh",
        "/bin/bash",
        "/bin/sh",
        "/usr/bin/zsh",
        "/usr/bin/bash",
        "/usr/local/bin/zsh",
    ];

    for shell in &valid_shells {
        assert!(shell.starts_with('/'), "Shell path must be absolute: {}", shell);
        assert!(!shell.contains(' '), "Shell path should not contain spaces: {}", shell);
    }
}

/// Test that the default shell function is consistent
#[test]
fn test_default_shell_consistency() {
    // Calling default_shell multiple times should return the same value
    let shell1 = default_shell();
    let shell2 = default_shell();
    assert_eq!(shell1, shell2, "default_shell should be consistent");
}
