//! AI chat session for `ash`'s `?` mode (Plan 027).
//!
//! A `ChatSession` owns an in-memory conversation (`Vec<Message>`) and persists
//! it to `~/.auto-shell-ai-chat.json`. It is intentionally decoupled from the
//! REPL: callers build the system prompt and pass it in, so this module is
//! fully unit-testable without a network or a `Repl`.
//!
//! v1 is chat-only (no tools). See `plans/027-ash-ai-chat-mode.md`.

use std::path::Path;

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
}
