//! AI chat session for `ash`'s `?` mode (Plan 027).
//!
//! A `ChatSession` owns an in-memory conversation (`Vec<Message>`) and persists
//! it to `~/.auto-shell-ai-chat.json`. It is intentionally decoupled from the
//! REPL: callers build the system prompt and pass it in, so this module is
//! fully unit-testable without a network or a `Repl`.
//!
//! v1 is chat-only (no tools). See `plans/027-ash-ai-chat-mode.md`.

use std::future::Future;
use std::path::Path;
use std::path::PathBuf;

/// Build the per-request system prompt for the chat. Pure fn so it is unit-
/// testable and the cwd is always current (the user may `cd` between turns).
pub fn build_system_prompt(cwd: &Path) -> String {
    format!(
        "You are an AI assistant for Ash (AutoShell), a shell similar to bash/fish.\n\
         The user's current directory is: {}\n\
         Answer the user's questions helpfully and concisely. You may discuss shell\n\
         commands, explain concepts, or help troubleshoot. Plain text only — no markdown.",
        cwd.display()
    )
}

/// Run a future on a fresh single-thread tokio runtime and block on it.
/// The REPL is synchronous; this mirrors `Repl::ask_ai`'s runtime pattern so
/// each chat turn can call the async `AiClient` without a global runtime.
pub fn block_on_async<F: Future>(fut: F) -> F::Output {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    rt.block_on(fut)
}

/// The recognized chat slash commands (v1 minimal set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    /// Forget the conversation history.
    Clear,
    /// Leave chat mode (same as pressing Esc).
    Exit,
}

/// If `line` is one of the chat slash commands (case-insensitive, surrounding
/// whitespace ignored), return it. Otherwise return `None`.
pub fn parse_slash_command(line: &str) -> Option<SlashCommand> {
    match line.trim().to_lowercase().as_str() {
        "/clear" => Some(SlashCommand::Clear),
        "/exit" => Some(SlashCommand::Exit),
        _ => None,
    }
}

/// Path to the persisted chat history: `~/.auto-shell-ai-chat.json`.
pub fn history_path() -> PathBuf {
    history_file_under(&dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
}

/// Build the history file path for a given home directory. Factored out so the
/// path logic is testable without depending on the OS home-dir lookup (which
/// `dirs` resolves via native APIs and does not honor `HOME` on Windows).
fn history_file_under(home: &Path) -> PathBuf {
    home.join(".auto-shell-ai-chat.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn smoke() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn system_prompt_contains_cwd() {
        let cwd = Path::new("/tmp/some-project");
        let s = build_system_prompt(cwd);
        assert!(s.contains("Ash"), "prompt should name Ash");
        assert!(s.contains("/tmp/some-project"), "prompt should include cwd");
        assert!(s.to_lowercase().contains("no markdown"), "prompt should forbid markdown");
    }

    #[test]
    fn block_on_async_runs_future() {
        let val = block_on_async(async { 42 });
        assert_eq!(val, 42);
    }

    #[test]
    fn parse_slash_commands() {
        assert_eq!(parse_slash_command("/clear"), Some(SlashCommand::Clear));
        assert_eq!(parse_slash_command("/exit"), Some(SlashCommand::Exit));
        assert_eq!(parse_slash_command("  /CLEAR  "), Some(SlashCommand::Clear));
        assert_eq!(parse_slash_command("/Exit"), Some(SlashCommand::Exit));
        assert_eq!(parse_slash_command("hello"), None);
        assert_eq!(parse_slash_command("/unknown"), None);
        assert_eq!(parse_slash_command(""), None);
    }

    #[test]
    fn history_file_under_is_home_plus_filename() {
        let home = Path::new("/home/user");
        let p = history_file_under(home);
        assert_eq!(p, Path::new("/home/user/.auto-shell-ai-chat.json"));
    }

    #[test]
    fn history_path_has_correct_filename() {
        // We can't control dirs::home_dir() across platforms, but the filename
        // component is deterministic regardless of where home resolves.
        let p = history_path();
        assert_eq!(
            p.file_name().and_then(|s| s.to_str()),
            Some(".auto-shell-ai-chat.json")
        );
    }
}
