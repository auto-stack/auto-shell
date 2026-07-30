//! AI chat session for `ash`'s `?` mode (Plan 027) — upgraded to agent-backed
//! tool-calling (Plan 029 F4).
//!
//! A `ChatSession` owns an [`Agent`] (the auto-ai-agent harness) plus a set of
//! [`AshCommandTool`]s, so the model can call ash commands during a turn
//! (tool-calling). It persists the conversation text turns to
//! `~/.auto-shell-ai-chat.json`.
//!
//! This replaces Plan 027's hand-rolled `CompletionRequest` + streaming loop
//! with `Agent::run_stream`, which handles the ReAct loop, tool dispatch, and
//! history internally. The session keeps only the persistence shell.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use auto_ai_agent::agent::{Agent, StreamEvent};
use auto_ai_agent::{Assistant, Client as AgentClient};
use auto_ai_client::AiClient;

use crate::ash_command_tool::{AshCommandShellThread, AshCommandTool};
use auto_ai_client::Message;

/// Build a per-request system prompt naming the cwd.
///
/// Historically (Plan 027) this was injected every chat turn. Under the
/// agent-backed F4 (Plan 029) the [`Assistant`] role supplies its own system
/// prompt and the agent discovers the cwd via the `pwd` tool instead, so this
/// is no longer called for F4. It is retained for F3 (NL→command) and other
/// callers that still build a single-shot request.
pub fn build_system_prompt(cwd: &Path) -> String {
    format!(
        "You are an AI assistant for Ash (AutoShell), a shell similar to bash/fish.\n\
         The user's current directory is: {}\n\
         Answer the user's questions helpfully and concisely. You may discuss shell\n\
         commands, explain concepts, or help troubleshoot. Plain text only — no markdown.",
        cwd.display()
    )
}

/// Run a future on a fresh tokio runtime and block on it.
///
/// The REPL is synchronous, so each chat turn spins up a one-shot runtime to
/// drive the async `Agent::run_stream` call. The future passed in MUST NOT
/// itself construct an `AiClient` (or any `reqwest::blocking::Client`) — that
/// runs a blocking daemon probe and panics when built inside a tokio runtime
/// context ("Cannot drop a runtime in a context where blocking is not
/// allowed"). Callers build the client on the sync side first; see
/// [`ChatSession::load`].
pub fn block_on_async<F: Future>(fut: F) -> F::Output {
    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
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

/// Read persisted text messages from a history file. Missing file → empty;
/// corrupt file → empty + warning (recovers gracefully).
fn load_messages(path: &Path) -> Vec<Message> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Vec<Message>>(&text) {
            Ok(msgs) => msgs,
            Err(e) => {
                eprintln!("warning: chat history was unreadable, starting fresh: {}", e);
                Vec::new()
            }
        },
        Err(_) => Vec::new(), // missing file — normal first run, stay silent
    }
}

/// Register the ash command tools an agent can call during F4 tool-calling.
///
/// All tools share one [`AshCommandShellThread`] so they operate on the same
/// session state (cwd, variables). v1 registers a small, safe command set;
/// Plan 029's `register_all` (full 80 commands) is deferred.
fn register_ash_tools(agent: &mut Agent, tx: std::sync::mpsc::Sender<crate::ash_command_tool::CmdRequest>) {
    agent.register_tool(AshCommandTool::new(
        "pwd", "print the current working directory", tx.clone(),
    ));
    agent.register_tool(AshCommandTool::new(
        "ls", "list directory contents (names + types + sizes)", tx.clone(),
    ));
    agent.register_tool(AshCommandTool::new(
        "cat", "print a file's contents", tx.clone(),
    ));
    agent.register_tool(AshCommandTool::new(
        "cd", "change the current directory", tx.clone(),
    ));
    agent.register_tool(AshCommandTool::new(
        "echo", "print text", tx.clone(),
    ));
    agent.register_tool(AshCommandTool::new(
        "grep", "search for a pattern in files", tx,
    ));
}

/// A persistent, agent-backed chat conversation.
///
/// Wraps an [`Agent`] (which owns the LLM client, ReAct loop, tool registry,
/// and in-memory history) plus the dedicated shell thread that backs the
/// [`AshCommandTool`]s. Persistence of the *text* turns (user/assistant only —
/// tool messages are filtered out for backward compatibility) is the session's
/// responsibility.
///
/// The `AiClient` is created ONCE at load time (in a synchronous context),
/// NOT inside the async turn. See [`block_on_async`] for why.
pub struct ChatSession {
    /// The agent harness: owns client + memory + tools + role.
    agent: Agent,
    /// Shared client handle. Kept separately so [`Self::clear`] can rebuild
    /// the agent without re-probing the daemon.
    client: Arc<dyn AgentClient>,
    /// History file path.
    history_path: PathBuf,
    /// Keeps the dedicated shell thread alive — if dropped, the tools' sender
    /// goes dead and calls fail with "shell thread has exited".
    shell_thread: AshCommandShellThread,
}

impl ChatSession {
    /// Load the conversation from `~/.auto-shell-ai-chat.json` and connect to
    /// the daemon. Call from a SYNCHRONOUS context (the daemon probe blocks).
    pub fn load() -> Result<Self, String> {
        let ai_client = AiClient::new().map_err(|e| format!("AI client init: {}", e))?;
        let client: Arc<dyn AgentClient> = Arc::new(ai_client);
        Ok(Self::with_client_and_path(client, history_path()))
    }

    /// Construct with an explicit client and the default history path.
    pub fn with_client(client: Arc<dyn AgentClient>) -> Self {
        Self::with_client_and_path(client, history_path())
    }

    /// Construct from an explicit client + history file path. Builds the
    /// agent, registers tools, and preloads persisted text turns.
    pub fn with_client_and_path(client: Arc<dyn AgentClient>, path: PathBuf) -> Self {
        let messages = load_messages(&path);
        let shell_thread = AshCommandShellThread::start();
        let tx = shell_thread.sender();

        let mut agent = Agent::new(Assistant, client.clone());
        register_ash_tools(&mut agent, tx);
        // Replay persisted text turns into the agent's memory. preload skips
        // tool-role messages, so it's safe even with mixed histories.
        agent.preload_messages(messages);

        ChatSession {
            agent,
            client,
            history_path: path,
            shell_thread,
        }
    }

    /// Number of user turns in the conversation (each user turn pairs with an
    /// assistant reply). Counts user messages rather than raw history length,
    /// because tool-calling adds tool-role messages that aren't "turns".
    pub fn turn_count(&self) -> usize {
        self.agent
            .history()
            .iter()
            .filter(|m| m.role == "user")
            .count()
    }

    /// Send one user turn through the agent's ReAct loop, streaming events to
    /// `on_event` as they arrive. The agent manages multi-turn memory and tool
    /// dispatch internally. On success returns the assistant's final text.
    ///
    /// The caller supplies `on_event` so it can render `Delta`/`ToolStart`/
    /// `Tool` events to the terminal (F4 shows tool calls as they happen).
    pub async fn send_turn_streaming(
        &mut self,
        user: &str,
        on_event: Arc<dyn Fn(StreamEvent) + Send + Sync>,
    ) -> Result<String, String> {
        // v1: no cancellation. Ctrl-C handling is a later enhancement.
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = self
            .agent
            .run_stream(user, on_event, cancel)
            .await
            .map_err(|e| format!("{}", e))?;
        Ok(result.output)
    }

    /// Forget the conversation. Rebuilds the agent (it has no clear-memory
    /// method) while reusing the client and shell thread, so no daemon re-probe
    /// and no tool disruption.
    pub fn clear(&mut self) {
        let tx = self.shell_thread.sender();
        let mut agent = Agent::new(Assistant, self.client.clone());
        register_ash_tools(&mut agent, tx);
        self.agent = agent;
    }

    /// Serialize the text turns (user + assistant, tool messages filtered) to
    /// the history file atomically (write temp, then rename). A crash mid-write
    /// won't corrupt the file.
    pub fn save(&self) -> std::io::Result<()> {
        // Keep only user/assistant text turns — drop tool/role messages so the
        // format stays compatible with older ash versions and stays compact.
        let text_turns: Vec<&Message> = self
            .agent
            .history()
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .collect();
        let json = serde_json::to_string(&text_turns).map_err(std::io::Error::other)?;
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
        let p = history_path();
        assert_eq!(
            p.file_name().and_then(|s| s.to_str()),
            Some(".auto-shell-ai-chat.json")
        );
    }

    /// Build a ChatSession at `path` with a no-op client (no daemon probe).
    fn session_at(path: PathBuf) -> ChatSession {
        let client: Arc<dyn AgentClient> =
            Arc::new(AiClient::with_url("http://0.0.0.0:0"));
        ChatSession::with_client_and_path(client, path)
    }

    #[test]
    fn load_from_missing_file_is_empty() {
        let tmp = std::env::temp_dir().join("ash_ai_missing_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");
        assert!(!path.exists());

        let s = session_at(path.clone());
        assert_eq!(s.turn_count(), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_from_corrupt_file_is_empty() {
        let tmp = std::env::temp_dir().join("ash_ai_corrupt_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");
        std::fs::write(&path, "this is { not valid json").unwrap();

        let s = session_at(path.clone());
        assert_eq!(s.turn_count(), 0, "corrupt file should recover to empty");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn clear_empties_turn_count() {
        // We can't easily seed agent memory with turns without a live LLM,
        // but clear() must leave turn_count at 0 and keep tools working.
        let tmp = std::env::temp_dir().join("ash_ai_clear_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");

        let mut s = session_at(path.clone());
        s.clear();
        assert_eq!(s.turn_count(), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The shell thread must survive clear() — otherwise tools would report
    /// "shell thread has exited" after a /clear.
    #[tokio::test]
    async fn clear_keeps_shell_thread_alive() {
        use auto_ai_agent::tool::Tool;

        let tmp = std::env::temp_dir().join("ash_ai_clearthread_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");

        let mut s = session_at(path.clone());
        s.clear();
        // Build a tool on the surviving shell thread and run it — if the
        // thread died, this errors with "shell thread has exited".
        let tool = AshCommandTool::new("pwd", "print cwd", s.shell_thread.sender());
        let result = tool.execute(&serde_json::Value::Null).await;
        assert!(result.is_ok(), "tool should still work after clear: {:?}", result);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// save() writes a JSON array that load_messages can read back.
    #[test]
    fn save_roundtrips_empty_session() {
        let tmp = std::env::temp_dir().join("ash_ai_save_empty_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");

        let s = session_at(path.clone());
        s.save().unwrap();
        assert!(path.exists(), "save should write the file");
        let reloaded = load_messages(&path);
        assert!(reloaded.is_empty(), "empty session saves empty history");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn save_persists_preloaded_text_turns() {
        let tmp = std::env::temp_dir().join("ash_ai_save_preloaded_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("chat.json");

        // Seed a history file with two text turns, load it (preloads into
        // agent memory), then save and reload to confirm round-trip.
        let seed = serde_json::to_string(&vec![
            Message::user("hello"),
            Message::assistant("hi there"),
        ])
        .unwrap();
        std::fs::write(&path, &seed).unwrap();

        let s = session_at(path.clone());
        assert_eq!(s.turn_count(), 1, "one user turn preloaded");
        s.save().unwrap();
        let reloaded = load_messages(&path);
        assert_eq!(reloaded.len(), 2, "both text turns persisted");
        assert_eq!(reloaded[0].role, "user");
        assert_eq!(reloaded[1].role, "assistant");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
