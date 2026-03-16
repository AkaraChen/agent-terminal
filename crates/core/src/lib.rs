pub mod buffer;
pub mod dsl;
pub mod ipc;
pub mod lock;
pub mod protocol;
pub mod session;
#[cfg(feature = "tcp")]
pub mod tcp;

/// Returns the default shell path for the current platform.
///
/// - macOS: `/bin/zsh` (zsh is the default shell since macOS Catalina)
/// - Linux: `/bin/bash` (most widely available)
pub fn default_shell() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "/bin/zsh"
    }
    #[cfg(target_os = "linux")]
    {
        "/bin/bash"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "/bin/sh" // fallback for other Unix-like systems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_shell_returns_non_empty() {
        let shell = default_shell();
        assert!(!shell.is_empty());
        assert!(shell.starts_with('/'));
    }

    #[test]
    fn test_default_shell_is_valid_path() {
        let shell = default_shell();
        // Should contain at least one slash and be an absolute path
        assert!(shell.contains('/'));
        assert!(shell.starts_with("/bin/"));
    }
}
