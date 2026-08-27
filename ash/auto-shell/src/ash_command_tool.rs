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

/// Shell control characters that turn a single argument into multiple
/// commands when spliced into a CLI string (Plan 072 M4 / S-7).
const METACHARS: &[char] = &[';', '|', '&', '>', '<'];

/// Characters double quotes cannot neutralize (expansion happens inside
/// quotes), so tokens containing them are refused outright.
const EXPANSION_CHARS: &[char] = &['$', '`'];

/// A request sent to the dedicated shell thread. Public so callers that hold
/// an [`AshCommandShellThread`] sender can name the type. Both variants are
/// `Send`.
pub enum CmdRequest {
    /// Run a shell command line via `Shell::execute_for_agent`.
    ShellCmd {
        cmd: String,
        respond: oneshot::Sender<Result<String, ToolError>>,
    },
    /// Evaluate AutoLang code via `Shell::eval_auto` (Plan 029 §6).
    AutoEval {
        code: String,
        respond: oneshot::Sender<Result<String, ToolError>>,
    },
}

/// Owns a dedicated thread that runs an ash [`Shell`].
///
/// The thread reads [`CmdRequest`]s from the channel, executes each against its
/// private `Shell`, and sends the result back on the embedded one-shot
/// channel. Because the `Shell` lives only inside that thread, its non-`Send`
/// interior never crosses a boundary.
///
/// Dropping the last [`mpsc::Sender`] (i.e. dropping all tools sharing it, or
/// dropping this handle) causes the thread's `recv` to return `None` and the
/// thread exits cleanly — no leak, no join needed.
pub struct AshCommandShellThread {
    tx: mpsc::Sender<CmdRequest>,
}

impl AshCommandShellThread {
    /// Start a dedicated shell thread with NO security policy (historic
    /// behavior). See [`Self::start_with_policy`] — callers that live inside
    /// an interactive session with CLI security flags should prefer it.
    pub fn start() -> Self {
        Self::start_with_policy(ash_core::security::SecurityPolicy::default())
    }

    /// Plan 072 M2 (S-5): start the dedicated shell thread under the
    /// **interactive session's** security policy. Without this the agent's
    /// Shell was a fresh default-policy one — `ash --read-only` / `--sandbox`
    /// did not constrain AI-initiated commands.
    pub fn start_with_policy(policy: ash_core::security::SecurityPolicy) -> Self {
        let (tx, rx) = mpsc::channel::<CmdRequest>();
        std::thread::spawn(move || {
            let mut shell = Shell::new();
            shell.set_policy(policy);
            // Loop until all senders are dropped (recv returns None).
            while let Ok(req) = rx.recv() {
                match req {
                    CmdRequest::ShellCmd { cmd, respond } => {
                        let r = match shell.execute_for_agent(&cmd, false, false) {
                            Ok(Some(out)) => Ok(out),
                            Ok(None) => Ok(String::new()),
                            Err(e) => Err(ToolError::Exec(format!("{e}"))),
                        };
                        let _ = respond.send(r);
                    }
                    CmdRequest::AutoEval { code, respond } => {
                        let mut r = match shell.eval_auto(&code) {
                            Ok(Some(out)) => Ok(out),
                            Ok(None) => Ok(String::new()),
                            Err(e) => Err(ToolError::Exec(format!("{e}"))),
                        };
                        // Plan 072 M2 (S-5): a system() call inside the script
                        // may have been denied by policy — the interactive
                        // path only prints it. Surface it to the model.
                        if let Some(reason) = shell.take_denial() {
                            r = r.map(|out| format!("{out}\n[security] command denied: {reason}"));
                        }
                        let _ = respond.send(r);
                    }
                }
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
            .send(CmdRequest::ShellCmd {
                cmd: cmd_str,
                respond: otx,
            })
            .map_err(|_| ToolError::Exec("shell thread has exited".into()))?;
        orx.await
            .map_err(|_| ToolError::Exec("shell thread dropped the response".into()))?
    }
}

/// Plan 068(统一 agent):提案工具 —— 与 [`AshCommandTool`] 同名注册,但
/// `execute` **不执行**命令:把拼好的 CLI 串送进 proposal 通道(宿主渲染
/// 建议卡等用户审批),并告知 agent 命令已提交审批、结果将在用户执行后的
/// 下一轮对话可见。用户点执行走普通命令路径,执行结果经会话上下文快照
/// 回流(多轮闭环)。
pub struct ProposeTool {
    name: String,
    description: String,
    sink: mpsc::Sender<String>,
}

impl ProposeTool {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        sink: mpsc::Sender<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            sink,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ProposeTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        // Lenient conversion: the proposal card is human-reviewed in full, so
        // metacharacters are shown to the user instead of being refused.
        let cmd_str = json_args_to_cli_lenient(&self.name, args)?;
        self.sink
            .send(cmd_str.clone())
            .map_err(|_| ToolError::Exec("proposal channel closed".into()))?;
        Ok(format!(
            "已提交审批:建议命令 `{cmd_str}` 已展示给用户,等待用户决定是否执行。
\"
            用户执行后,命令与结果会在下一轮对话的上下文中可见。请基于这一点继续回答
\"
            (给出操作指引/说明这条命令做什么),不要假设它已执行。"
        ))
    }
}

/// Rebuild a CLI string from the model's JSON arguments — **strict** form for
/// commands that will be executed directly (Plan 072 M4 / S-7).
///
/// A malicious or confused model could pass `{"args": ["hi", ";", "touch",
/// "pwn"]}`; splicing that raw produced `echo hi ; touch pwn`, riding the
/// read-only whitelist into arbitrary execution. The strict form neutralizes
/// this: tokens carrying control characters get shell-quoted (a quoted `;` is
/// a literal argument, harmless), and characters double quotes cannot
/// neutralize (`$`, backtick) are refused outright with a hint to use the
/// proposal tool instead.
///
/// Supports:
/// - `{"args": ["-l", "src"]}` — explicit arg list (most precise).
/// - a bare string (`"-l src"`) — space-joined; refused if it contains any
///   metacharacter (it names multiple tokens, so quoting can't help).
/// - an object of named values — flattened positionally as a best-effort.
/// - null / empty — just the command name.
fn json_args_to_cli(name: &str, args: &Value) -> Result<String, ToolError> {
    if args.is_null() {
        return Ok(name.to_string());
    }

    // Explicit args list: {"args": ["-l", "src"]}.
    if let Some(arr) = args.get("args").and_then(|a| a.as_array()) {
        let parts: Result<Vec<String>, ToolError> =
            arr.iter().map(|v| strict_token(name, v)).collect();
        return Ok(format!("{} {}", name, parts?.join(" ")));
    }

    // Bare string args: multiple intended tokens — any metacharacter here is
    // ambiguous at best, injectable at worst. Refuse and point at proposals.
    if let Some(s) = args.as_str() {
        if s.is_empty() {
            return Ok(name.to_string());
        }
        if s.chars().any(|c| METACHARS.contains(&c) || EXPANSION_CHARS.contains(&c)) {
            return Err(metachar_error(name, s));
        }
        return Ok(format!("{name} {s}"));
    }

    // Object of named flags/args: flatten values positionally.
    if let Some(obj) = args.as_object() {
        if obj.is_empty() {
            return Ok(name.to_string());
        }
        let parts: Result<Vec<String>, ToolError> =
            obj.values().map(|v| strict_token(name, v)).collect();
        return Ok(format!("{} {}", name, parts?.join(" ")));
    }

    Err(ToolError::Args(format!(
        "unsupported args shape for '{name}': {args}"
    )))
}

/// Lenient form for the proposal path — the command string goes to a
/// human-reviewed approval card, so it is shown verbatim, quoting only for
/// whitespace (pre-M4 behavior).
fn json_args_to_cli_lenient(name: &str, args: &Value) -> Result<String, ToolError> {
    if args.is_null() {
        return Ok(name.to_string());
    }
    if let Some(arr) = args.get("args").and_then(|a| a.as_array()) {
        let parts: Vec<String> = arr.iter().map(value_to_cli_token).collect();
        return Ok(format!("{} {}", name, parts.join(" ")));
    }
    if let Some(s) = args.as_str() {
        return Ok(if s.is_empty() {
            name.to_string()
        } else {
            format!("{name} {s}")
        });
    }
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

fn metachar_error(name: &str, token: &str) -> ToolError {
    ToolError::Exec(format!(
        "refused: argument '{token}' for '{name}' contains shell control \
         characters ($ ; | & > < `). Pass arguments as separate array items \
         without control characters, or use the proposal flow so the user \
         can review the full command."
    ))
}

/// One strict-mode token: expansion chars refused, control chars quoted.
fn strict_token(name: &str, v: &Value) -> Result<String, ToolError> {
    match v {
        Value::String(s) => {
            if s.chars().any(|c| EXPANSION_CHARS.contains(&c)) {
                return Err(metachar_error(name, s));
            }
            Ok(quote_strict(s))
        }
        other => Ok(other.to_string()),
    }
}

/// Convert a single JSON value into a shell CLI token, quoting if needed.
fn value_to_cli_token(v: &Value) -> String {
    match v {
        Value::String(s) => quote_if_needed(s),
        other => other.to_string(),
    }
}

/// Quote a token with double quotes if it contains whitespace or is empty
/// (historic lenient behavior, used for proposal-card text).
fn quote_if_needed(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace()) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Strict-path quoting: also quote tokens carrying shell control characters —
/// a quoted `;`/`|` is a literal argument, neutralizing token-splitting
/// injection (Plan 072 M4 / S-7).
fn quote_strict(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || METACHARS.contains(&c)) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// A [`Tool`] that evaluates AutoLang code on the dedicated shell thread
/// (Plan 029 §6).
///
/// Unlike [`AshCommandTool`] (which runs a shell command line), this executes
/// AutoLang source — `fn` definitions, `while`/`try-catch`, expressions — via
/// [`Shell::eval_auto`]. Used by NL→AutoLang (`ash ask`): the Agent generates
/// a script, calls this tool to run it, sees the result or error, and
/// self-corrects in the next ReAct turn.
///
/// Shares the same [`AshCommandShellThread`] sender (the thread dispatches
/// `CmdRequest::AutoEval` vs `CmdRequest::ShellCmd`).
pub struct EvalAutoTool {
    tx: mpsc::Sender<CmdRequest>,
}

impl EvalAutoTool {
    /// Create the tool. `tx` comes from [`AshCommandShellThread::sender`].
    pub fn new(tx: mpsc::Sender<CmdRequest>) -> Self {
        Self { tx }
    }
}

#[async_trait::async_trait]
impl Tool for EvalAutoTool {
    fn name(&self) -> &str {
        "eval_auto"
    }

    fn description(&self) -> &str {
        "Evaluate AutoLang source code and return the result. Use this to run \
         multi-step scripts with fn/while/try-catch/if. The code persists \
         across calls (definitions survive). Prefer the ash command tools for \
         shell work — system(\"cmd\") inside code runs under the session's \
         security policy and known-dangerous commands are refused. Pass the \
         source as {\"code\": \"...\"}."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "The AutoLang source code to evaluate"
                }
            },
            "required": ["code"]
        })
    }

    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let code = args
            .get("code")
            .and_then(|c| c.as_str())
            .ok_or_else(|| ToolError::Args("missing 'code' field".into()))?;

        // Plan 072 M2 (S-4): danger-check the source itself — this also
        // covers system("...") call strings nested inside it, which used to
        // bypass every tool-layer safeguard.
        let lower = code.to_lowercase();
        for pat in DANGER_PATTERNS {
            if lower.contains(pat) {
                return Err(ToolError::Exec(format!(
                    "refused: code matches danger pattern '{pat}'. Restructure \
                     the script without it, or have the user run it directly."
                )));
            }
        }

        let (otx, orx) = oneshot::channel();
        self.tx
            .send(CmdRequest::AutoEval {
                code: code.to_string(),
                respond: otx,
            })
            .map_err(|_| ToolError::Exec("shell thread has exited".into()))?;
        orx.await
            .map_err(|_| ToolError::Exec("shell thread dropped the response".into()))?
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

    // ── Plan 072 M4 (S-7): metacharacter neutralization ────────────────

    #[test]
    fn injection_separator_tokens_get_quoted_not_spliced() {
        // The S-7 payload: used to splice into `echo hi ; touch pwn` and ride
        // the read-only whitelist into executing a write command.
        let args = json!({"args": ["hi", ";", "touch", "pwn"]});
        let cli = json_args_to_cli("echo", &args).unwrap();
        assert_eq!(cli, "echo hi \";\" touch pwn");
        // Pipe/redirection separators are neutralized the same way.
        let args = json!({"args": ["a", "|", "b"]});
        assert_eq!(json_args_to_cli("echo", &args).unwrap(), "echo a \"|\" b");
    }

    #[test]
    fn regex_pipe_arg_is_quoted_preserving_intent() {
        // Legitimate `grep "foo|bar"`: quoting keeps the pipe inside the
        // pattern instead of rejecting the call.
        let args = json!({"args": ["foo|bar", "file.txt"]});
        assert_eq!(
            json_args_to_cli("grep", &args).unwrap(),
            "grep \"foo|bar\" file.txt"
        );
    }

    #[test]
    fn expansion_chars_are_refused() {
        for bad in ["$HOME", "a`b"] {
            let args = json!({"args": [bad]});
            let err = json_args_to_cli("echo", &args).unwrap_err();
            assert!(
                err.to_string().contains("control"),
                "refuse {bad}: {err}"
            );
        }
    }

    #[test]
    fn bare_string_with_metachar_is_refused() {
        let args = json!("hi ; touch pwn");
        assert!(json_args_to_cli("echo", &args).is_err());
    }

    #[test]
    fn lenient_path_keeps_raw_string_for_proposal_card() {
        // The proposal card is human-reviewed in full — show verbatim.
        let args = json!({"args": ["hi", ";", "touch", "pwn"]});
        assert_eq!(
            json_args_to_cli_lenient("echo", &args).unwrap(),
            "echo hi ; touch pwn"
        );
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
            "pwd '{pwd}' should reflect cd into '{tmp_str}'"
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

    // ── EvalAutoTool (Plan 029 §6) ─────────────────────────────────────

    async fn eval_auto_run(code: &str) -> Result<String, ToolError> {
        let thread = AshCommandShellThread::start();
        let tool = EvalAutoTool::new(thread.sender());
        // Drive the async tool call on a one-shot runtime (Shell::new inside
        // the thread does AutoLang VM init that can't run in a tokio ctx, but
        // the thread itself is a plain std::thread so this is fine).
        tool.execute(&json!({"code": code})).await
    }

    #[tokio::test]
    async fn eval_auto_runs_simple_expression() {
        let out = eval_auto_run("1 + 2").await.unwrap();
        assert!(out.contains('3'), "1+2 should produce 3, got: {out}");
    }

    #[tokio::test]
    async fn eval_auto_returns_string_value() {
        // A string expression returns its value (print() returns nothing, so
        // test a value-producing expression instead).
        let out = eval_auto_run("\"hello-auto\"").await.unwrap();
        assert!(out.contains("hello-auto"), "got: {out}");
    }

    #[tokio::test]
    async fn eval_auto_runs_fn_definition_and_call() {
        // Define a fn and call it — multi-line AutoLang with control flow.
        let code = "fn greet(name) {\n  return \"hi \" + name\n}\ngreet(\"world\")";
        let out = eval_auto_run(code).await.unwrap();
        assert!(out.contains("hi world"), "got: {out}");
    }

    #[tokio::test]
    async fn eval_auto_syntax_error_returns_error() {
        // Malformed code should surface as a ToolError, not panic.
        let result = eval_auto_run("fn broken(").await;
        assert!(result.is_err(), "syntax error should be an error");
    }

    #[tokio::test]
    async fn eval_auto_missing_code_arg_errors() {
        let thread = AshCommandShellThread::start();
        let tool = EvalAutoTool::new(thread.sender());
        let result = tool.execute(&json!({"not_code": "x"})).await;
        assert!(result.is_err());
    }

    /// Plan 072 M2 (S-4): `system("rm -rf /")` inside generated code used to
    /// bypass every tool-layer safeguard.
    #[tokio::test]
    async fn eval_auto_refuses_embedded_dangerous_system_call() {
        let result = eval_auto_run("system(\"rm -rf /\")").await;
        assert!(result.is_err(), "danger pattern inside system() must be refused");
    }

    /// Plan 072 M2 (S-5): the agent's shell thread must run under the
    /// interactive session's policy, not a fresh default one.
    #[tokio::test]
    async fn shell_thread_runs_under_passed_policy() {
        let thread = AshCommandShellThread::start_with_policy(
            ash_core::security::SecurityPolicy {
                deny: vec!["pwd".into()],
                ..Default::default()
            },
        );
        let tool = AshCommandTool::new("pwd", "print cwd", thread.sender());
        let result = tool.execute(&Value::Null).await;
        assert!(
            result.is_err(),
            "--deny'd command must be refused on the agent shell thread"
        );
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
