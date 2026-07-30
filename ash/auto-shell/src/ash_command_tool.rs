//! Bridge from ash shell commands to the `auto_ai_agent::tool::Tool` trait
//! (Plan 029 §2.2).
//!
//! [`AshCommandTool`] wraps a single ash command so an `auto_ai_agent`
//! `Agent` can invoke it during a ReAct loop (F4 tool-calling) or a
//! SmartCommand's AI judgment step.
//!
//! ## Why a dedicated thread?
//!
//! `Tool` requires `Send + Sync` (the agent runs `async` on a multi-thread
//! runtime). But `Shell` is **not** `Send` — it owns an `AutovmReplSession`
//! whose `type_registry` is `Rc<RefCell<...>>` (single-threaded, in
//! auto-lang). `Arc<Mutex<Shell>>` cannot satisfy the bound, and
//! `tokio::task::LocalSet` can't either (the bound is on the trait itself).
//!
//! The clean fix is a **dedicated OS thread** that owns the `Shell` outright
//! (it never crosses a thread boundary — `thread::spawn`'s closure `move`s it
//! in). The thread only ever exchanges `Send` values over a channel: command
//! strings in, results out. So the `Tool` holds only a `Send + Sync` channel
//! sender, and session state (cwd, variables) persists across calls because
//! it's always the same `Shell` in the same thread.

use std::sync::mpsc;

use auto_ai_agent::tool::Tool;
use auto_ai_agent::ToolError;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::shell::Shell;

/// Dangerous command patterns an Agent must never run directly. If a rebuilt
/// command matches one, the tool refuses before sending it to the shell — the
/// agent must seek explicit user approval through another path. Mirrors the
/// safeguard in auto-ai-cli's `RunCommand`.
const DANGER_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm -rf $home",
    ":(){:|:&};:",
    "mkfs",
    "dd if=",
    "shutdown",
    "reboot",
];

/// A request sent to the dedicated shell thread: the command string plus a
/// one-shot channel to return the result on. Both are `Send`. Public so
/// callers that hold an [`AshCommandShellThread`] sender can name the type.
pub struct CmdRequest {
    cmd: String,
    respond: oneshot::Sender<Result<String, ToolError>>,
}

/// Owns a dedicated thread that runs an ash [`Shell`].
///
/// The thread reads `CmdRequest`s from the channel, executes each against its
/// private `Shell`, and sends the result back on the embedded one-shot
/// channel. Because the `Shell` lives only inside that thread, its non-`Send`
/// interior never crosses a boundary.
///
/// Dropping the last [`mpsc::Sender`] (i.e. dropping all `AshCommandTool`s
/// sharing it, or dropping this handle) causes the thread's `recv` to return
/// `None` and the thread exits cleanly — no leak, no join needed.
pub struct AshCommandShellThread {
    tx: mpsc::Sender<CmdRequest>,
}

impl AshCommandShellThread {
    /// Start a dedicated shell thread. Returns a sender you pass to
    /// [`AshCommandTool::new`].
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<CmdRequest>();
        std::thread::spawn(move || {
            let mut shell = Shell::new();
            // Loop until all senders are dropped (recv returns None).
            while let Ok(req) = rx.recv() {
                let result = match shell.execute_for_agent(&req.cmd, false, false) {
                    Ok(Some(out)) => Ok(out),
                    Ok(None) => Ok(String::new()),
                    Err(e) => Err(ToolError::Exec(format!("{e}"))),
                };
                // If the responder went away (caller dropped the future),
                // sending fails — just discard and continue.
                let _ = req.respond.send(result);
            }
            // rx returned None: all senders dropped, thread exits.
        });
        Self { tx }
    }

    /// The sender for tools to use.
    pub fn sender(&self) -> mpsc::Sender<CmdRequest> {
        self.tx.clone()
    }
}

/// A [`Tool`] backed by a single ash command, executed on a dedicated shell
/// thread.
///
/// Construct one per command the Agent may call, all sharing a sender from a
/// single [`AshCommandShellThread`] so they operate on the same session state
/// (cwd, variables, history). See [`AshCommandShellThread`] for why a thread
/// is needed.
pub struct AshCommandTool {
    name: String,
    description: String,
    tx: mpsc::Sender<CmdRequest>,
}

impl AshCommandTool {
    /// Create a tool wrapping a single command. `tx` comes from
    /// [`AshCommandShellThread::sender`].
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        tx: mpsc::Sender<CmdRequest>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            tx,
        }
    }
}

#[async_trait::async_trait]
impl Tool for AshCommandTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let cmd_str = json_args_to_cli(&self.name, args)?;

        // Refuse known-dangerous patterns before they reach the shell.
        let lower = cmd_str.to_lowercase();
        for pat in DANGER_PATTERNS {
            if lower.contains(pat) {
                return Err(ToolError::Exec(format!(
                    "refused: command matches danger pattern '{pat}'. \
                     Requires explicit user approval outside the agent loop."
                )));
            }
        }

        // Send the command to the dedicated shell thread and await its reply.
        // If the thread has exited (sender dropped), report it as an error
        // rather than hanging.
        let (otx, orx) = oneshot::channel();
        self.tx
            .send(CmdRequest {
                cmd: cmd_str,
                respond: otx,
            })
            .map_err(|_| ToolError::Exec("shell thread has exited".into()))?;
        orx.await
            .map_err(|_| ToolError::Exec("shell thread dropped the response".into()))?
    }
}

/// Rebuild a CLI string from the model's JSON arguments.
///
/// Supports:
/// - `{"args": ["-l", "src"]}` — explicit arg list (most precise); each token
///   is shell-quoted if it contains whitespace.
/// - a bare string (`"-l src"`) — space-joined after the command name.
/// - an object of named values — flattened positionally as a best-effort.
/// - null / empty — just the command name.
fn json_args_to_cli(name: &str, args: &Value) -> Result<String, ToolError> {
    if args.is_null() {
        return Ok(name.to_string());
    }

    // Explicit args list: {"args": ["-l", "src"]}.
    if let Some(arr) = args.get("args").and_then(|a| a.as_array()) {
        let parts: Vec<String> = arr.iter().map(value_to_cli_token).collect();
        return Ok(format!("{} {}", name, parts.join(" ")));
    }

    // Bare string args.
    if let Some(s) = args.as_str() {
        return Ok(if s.is_empty() {
            name.to_string()
        } else {
            format!("{name} {s}")
        });
    }

    // Object of named flags/args: flatten values positionally.
    if let Some(obj) = args.as_object() {
        if obj.is_empty() {
            return Ok(name.to_string());
        }
        let parts: Vec<String> = obj.values().map(value_to_cli_token).collect();
        return Ok(format!("{} {}", name, parts.join(" ")));
    }

    Err(ToolError::Args(format!(
        "unsupported args shape for '{name}': {args}"
    )))
}

/// Convert a single JSON value into a shell CLI token, quoting if needed.
fn value_to_cli_token(v: &Value) -> String {
    match v {
        Value::String(s) => quote_if_needed(s),
        other => other.to_string(),
    }
}

/// Quote a token with double quotes if it contains whitespace or is empty.
fn quote_if_needed(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace()) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── args → CLI conversion ──────────────────────────────────────────

    #[test]
    fn null_args_yields_bare_command() {
        assert_eq!(json_args_to_cli("ls", &Value::Null).unwrap(), "ls");
    }

    #[test]
    fn explicit_args_list() {
        let args = json!({"args": ["-l", "src"]});
        assert_eq!(json_args_to_cli("ls", &args).unwrap(), "ls -l src");
    }

    #[test]
    fn args_list_quotes_whitespace() {
        let args = json!({"args": ["my file.txt"]});
        assert_eq!(
            json_args_to_cli("cat", &args).unwrap(),
            "cat \"my file.txt\""
        );
    }

    #[test]
    fn bare_string_args() {
        let args = json!("-l --color=auto");
        assert_eq!(json_args_to_cli("ls", &args).unwrap(), "ls -l --color=auto");
    }

    #[test]
    fn empty_string_args_yields_bare_command() {
        let args = json!("");
        assert_eq!(json_args_to_cli("pwd", &args).unwrap(), "pwd");
    }

    #[test]
    fn object_args_flatten_positionally() {
        let args = json!({"path": "src", "flag": true});
        let result = json_args_to_cli("cat", &args).unwrap();
        assert!(result.starts_with("cat "), "got: {result}");
        assert!(result.contains("src"));
        assert!(result.contains("true"));
    }

    #[test]
    fn empty_object_yields_bare_command() {
        let args = json!({});
        assert_eq!(json_args_to_cli("ls", &args).unwrap(), "ls");
    }

    #[test]
    fn number_token_not_quoted() {
        let args = json!({"args": [42]});
        assert_eq!(json_args_to_cli("echo", &args).unwrap(), "echo 42");
    }

    // ── Tool execution via the dedicated thread ────────────────────────

    /// Helper: start a thread + build a tool on it.
    fn tool(name: &str, desc: &str) -> (AshCommandShellThread, AshCommandTool) {
        let thread = AshCommandShellThread::start();
        let tool = AshCommandTool::new(name, desc, thread.sender());
        (thread, tool)
    }

    #[tokio::test]
    async fn runs_command_through_shell() {
        let (_thread, tool) = tool("echo", "print text");
        let out = tool.execute(&json!({"args": ["hello-agent"]})).await.unwrap();
        assert!(out.contains("hello-agent"), "got: {out}");
    }

    /// The core reason for the dedicated-thread design: session state (cwd,
    /// variables) persists across calls because it's always the same Shell.
    #[tokio::test]
    async fn preserves_session_state_across_calls() {
        let (_thread, tool) = tool("cd", "change directory");
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_string_lossy().replace('\\', "/");

        // cd into a temp dir...
        let _ = tool.execute(&json!({"args": [tmp_str.clone()]})).await.unwrap();
        // ...then pwd must reflect it (state survived across calls).
        // We need a pwd tool sharing the SAME thread (same Shell).
        let pwd_tool = AshCommandTool::new("pwd", "print cwd", tool.tx_clone_for_test());
        let pwd = pwd_tool.execute(&Value::Null).await.unwrap();

        // Normalize both sides for comparison: lowercase, forward slashes,
        // trim trailing slash so "/Temp/" matches "/Temp".
        let norm = |s: &str| {
            s.trim()
                .to_lowercase()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_string()
        };
        assert!(
            norm(&pwd) == norm(&tmp_str),
            "pwd '{}' should reflect cd into '{}'",
            pwd,
            tmp_str
        );
    }

    #[tokio::test]
    async fn refuses_dangerous_pattern() {
        let (_thread, tool) = tool("echo", "print");
        let result = tool.execute(&json!({"args": ["rm -rf /"]})).await;
        assert!(result.is_err(), "should refuse danger pattern");
        assert!(result.unwrap_err().to_string().contains("refused"));
    }

    #[tokio::test]
    async fn error_when_thread_exited() {
        // A tool whose sender's receiver was never connected (simulating a
        // shell thread that has exited) reports a clean error, not a hang.
        let orphan_tx: mpsc::Sender<CmdRequest> = {
            let (t, r) = mpsc::channel();
            drop(r);
            t
        };
        let tool = AshCommandTool::new("pwd", "print cwd", orphan_tx);
        let result = tool.execute(&Value::Null).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("shell thread"));
    }

    #[tokio::test]
    async fn is_usable_as_dyn_tool() {
        let (_thread, tool) = tool("pwd", "print cwd");
        let t: &dyn Tool = &tool;
        assert_eq!(t.name(), "pwd");
        let out = t.execute(&Value::Null).await.unwrap();
        assert!(!out.trim().is_empty(), "pwd should return a path");
    }
}

#[cfg(test)]
impl AshCommandTool {
    /// Test-only: clone the underlying sender so two tools can share one
    /// shell thread (and thus one session state).
    fn tx_clone_for_test(&self) -> mpsc::Sender<CmdRequest> {
        self.tx.clone()
    }
}
