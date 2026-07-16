//! AI chat session for `ash`'s `?` mode (Plan 027).
//!
//! A `ChatSession` owns an in-memory conversation (`Vec<Message>`) and persists
//! it to `~/.auto-shell-ai-chat.json`. It is intentionally decoupled from the
//! REPL: callers build the system prompt and pass it in, so this module is
//! fully unit-testable without a network or a `Repl`.
//!
//! v1 is chat-only (no tools). See `plans/027-ash-ai-chat-mode.md`.

use auto_ai_client::{AiClient, CompletionRequest, CompletionResponse, Message};
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

/// A persistent chat conversation backed by a JSON file.
///
/// `messages` holds only user+assistant turns; the system prompt is rebuilt
/// per request (cwd changes between sessions) and is NOT stored here.
pub struct ChatSession {
    messages: Vec<Message>,
    history_path: PathBuf,
}

impl ChatSession {
    /// Load the conversation from `~/.auto-shell-ai-chat.json`.
    /// Missing or corrupt file → empty conversation (recovers gracefully).
    pub fn load() -> Self {
        Self::with_history_path(history_path())
    }

    /// Construct from an explicit history file path (used by `load` and tests).
    /// A missing file yields an empty conversation (normal first run, silent);
    /// a corrupt file also recovers to empty but logs a single warning line.
    pub fn with_history_path(path: PathBuf) -> Self {
        let messages = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Vec<Message>>(&text) {
                Ok(msgs) => msgs,
                Err(e) => {
                    // Corrupt history: recover to empty, but tell the user.
                    eprintln!("warning: chat history was unreadable, starting fresh: {}", e);
                    Vec::new()
                }
            },
            Err(_) => Vec::new(), // missing file — normal first run, stay silent
        };
        ChatSession { messages, history_path: path }
    }

    /// Number of stored turns (user + assistant messages).
    pub fn turn_count(&self) -> usize {
        self.messages.len()
    }

    /// Append a user turn.
    pub fn push_user(&mut self, text: &str) {
        self.messages.push(Message::user(text));
    }

    /// Append an assistant turn.
    pub fn push_assistant(&mut self, text: &str) {
        self.messages.push(Message::assistant(text));
    }

    /// Send one user turn. Builds a multi-turn request (history + this user
    /// message), streams the assistant reply to stdout as deltas arrive, and on
    /// success appends BOTH the user and assistant messages to the history.
    /// On error the history is left untouched (no orphan user turn).
    ///
    /// `system` is the per-request system prompt (NOT stored in `messages`).
    pub async fn send_turn_streaming(
        &mut self,
        user: &str,
        system: &str,
    ) -> Result<String, String> {
        let client = AiClient::new().map_err(|e| format!("AI client init: {}", e))?;

        // Build the request with the user turn appended, WITHOUT mutating
        // `self.messages` yet. We only persist the user turn once the call
        // succeeds, so a failed/erroring turn leaves the history clean (no
        // orphan user message with no assistant reply).
        let mut messages = self.messages.clone();
        messages.push(Message::user(user));
        let req = CompletionRequest {
            model: "tier:mid".to_string(),
            messages,
            max_tokens: Some(4096),
            temperature: Some(0.4),
            system_prompt: Some(system.to_string()),
            tools: Vec::new(),
            stream: false, // complete_stream sets this to true itself.
        };

        use std::io::Write;
        let on_event = |ev: serde_json::Value| {
            if let Some(text) = ev.get("text").and_then(|t| t.as_str()) {
                print!("{}", text);
                let _ = std::io::stdout().flush();
            }
        };

        let resp: CompletionResponse = client
            .complete_stream(&req, on_event)
            .await
            .map_err(|e| format!("{}", e))?;
        println!(); // newline after the streamed reply

        if let Some(err) = &resp.error {
            return Err(err.clone());
        }

        // Success: now persist both turns.
        let text = resp.content.trim().to_string();
        self.push_user(user);
        self.push_assistant(&text);
        Ok(text)
    }

    /// Forget the conversation (in-memory only; call `save` to persist).
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Serialize the conversation to the history file atomically (write to a
    /// temp sibling, then rename). A crash mid-write won't corrupt the file.
    pub fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_string(&self.messages)
            .map_err(std::io::Error::other)?;
        let tmp = self.history_path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.history_path)
    }
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

    #[test]
    fn load_from_missing_file_is_empty() {
        let tmp = std::env::temp_dir().join("ash_ai_missing_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");
        assert!(!path.exists());

        let s = ChatSession::with_history_path(path.clone());
        assert_eq!(s.turn_count(), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let tmp = std::env::temp_dir().join("ash_ai_roundtrip_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");

        let mut s = ChatSession::with_history_path(path.clone());
        s.push_user("hello");
        s.push_assistant("hi there");
        s.save().unwrap();
        assert!(path.exists(), "save should write the file");

        let s2 = ChatSession::with_history_path(path);
        assert_eq!(s2.turn_count(), 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_from_corrupt_file_is_empty() {
        let tmp = std::env::temp_dir().join("ash_ai_corrupt_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");
        std::fs::write(&path, "this is { not valid json").unwrap();

        let s = ChatSession::with_history_path(path.clone());
        assert_eq!(s.turn_count(), 0, "corrupt file should recover to empty");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn clear_empties_and_persists() {
        let tmp = std::env::temp_dir().join("ash_ai_clear_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");

        let mut s = ChatSession::with_history_path(path.clone());
        s.push_user("a");
        s.push_assistant("b");
        s.clear();
        assert_eq!(s.turn_count(), 0);
        s.save().unwrap();
        // Reload to confirm persistence.
        let s2 = ChatSession::with_history_path(path);
        assert_eq!(s2.turn_count(), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
