//! Plan 070 M1 (S-2): safe "open path in OS" used by both transports.
//!
//! The old implementation concatenated the request-supplied path into
//! `cmd /C start "" <path>` — a `{"path": "x & calc"}` POST meant arbitrary
//! command execution. This module never touches a shell: the path is
//! canonicalized (which requires it to exist, rejecting metacharacter
//! payloads) and passed as a single argv element to the OS opener.

use std::path::Path;

/// Canonicalize `raw`. Fails for nonexistent paths, so metacharacter
/// payloads like `"x & calc"` (no such file) are refused before anything is
/// spawned. Also resolves `..` and symlinks.
pub fn resolve(raw: &str) -> Result<std::path::PathBuf, String> {
    Path::new(raw)
        .canonicalize()
        .map_err(|e| format!("open_path: cannot resolve '{raw}': {e}"))
}

/// Open an already-resolved path with the OS file manager / default handler.
/// No shell is involved — the path travels as a single argv element.
fn spawn_opener(canonical: &std::path::Path) -> Result<(), String> {
    let spawn = if cfg!(windows) {
        if canonical.is_dir() {
            std::process::Command::new("explorer.exe").arg(canonical).spawn()
        } else {
            // Default file handler WITHOUT cmd.exe — rundll32 takes the path
            // as a single argv element, no shell metacharacters involved.
            std::process::Command::new("rundll32")
                .arg("url.dll,FileProtocolHandler")
                .arg(canonical)
                .spawn()
        }
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(canonical).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(canonical).spawn()
    };

    spawn.map(|_| ()).map_err(|e| format!("open_path: {e}"))
}

/// Canonicalize `raw`, then open it. `Err` = refused (nonexistent path) or
/// spawn failure; nothing is executed in either case.
pub fn open_in_os(raw: &str) -> Result<(), String> {
    spawn_opener(&resolve(raw)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metacharacter_path_is_refused_without_spawning() {
        // The S-2 payload: old code ran `cmd /C start "" x & calc`.
        for payload in ["x & calc", "a|b", "nul & whoami", "'; rm -rf /"] {
            assert!(resolve(payload).is_err(), "payload refused: {payload}");
        }
    }

    #[test]
    fn nonexistent_path_is_refused() {
        assert!(resolve("Z:/definitely/not/a/real/path/xyz").is_err());
    }

    #[test]
    fn existing_path_resolves() {
        // Resolve-only (spawn_opener would pop a real Explorer window).
        let dir = resolve(std::env::temp_dir().to_str().unwrap_or_default());
        assert!(dir.is_ok(), "temp dir should resolve: {dir:?}");
    }
}
