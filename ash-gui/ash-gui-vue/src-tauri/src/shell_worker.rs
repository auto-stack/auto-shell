//! Shell worker — owns the `!Send` Shell on a dedicated OS thread.
//!
//! `auto_shell::Shell` holds an `AutovmReplSession` whose type registry is
//! `Rc<RefCell<…>>` (single-threaded), so it cannot cross a thread boundary.
//! Like the iced ash-gui (`ash-gui-bin/src/main.rs`), we keep the Shell on one
//! dedicated thread and communicate via channels. Here the "GUI" is the Tauri
//! event bus: the worker emits `command-result` events when commands finish.
//!
//! ## Plan 040 milestones
//!
//! - **M1**: full command preprocessing. Commands run through `shell.execute()`
//!   — the *complete* pipeline (variable / tilde / glob / alias expansion,
//!   redirects, env prefixes, hooks, security policy). This fixes the old
//!   `render_structured` shortcut, which called `cmd.run_atom` directly and
//!   bypassed expansion (`ls ~/foo`, `echo $HOME`, `ls *.md`,
//!   `grep x file > out.txt` all broke). Structured output is recovered via a
//!   [`CaptureHook`]: `Shell::format_output` calls
//!   `render_pipeline_to_structured` *after* expansion, then hands the
//!   `&RenderedOutput` to our hook, which clones it into a capture slot. The
//!   hook returns `None` so Shell still falls back to text; we prefer the
//!   captured structured data, else the `execute` text.
//! - **M2**: Shell initialization parity with the TUI/CLI REPL — aliases from
//!   `ash.toml`, `~/.ashrc` user functions, installed plugins (`init_shell`).
//! - **M4**: streaming output for long external commands — simple external
//!   commands are drained line-by-line via `spawn_external_stream`, emitting
//!   `command-output` chunks before the final `command-result`.
//! - **M5**: cancellation — a per-worker cancel flag checked in the stream
//!   drain loop; `cancel_command` flips it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ash_core::renderer::RenderedOutput;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

/// A request from a Tauri command.
pub enum CommandReq {
    /// Run `cmd`, attribute the result to `block_id`.
    Run { block_id: usize, cmd: String },
    /// Plan 040 M3: run a SmartCommand by name+args against the worker's Shell
    /// (preserves session cwd/env/functions). The reply comes back on `reply`.
    RunSmart {
        name: String,
        args: Vec<String>,
        reply: tokio::sync::oneshot::Sender<SmartResult>,
    },
}

/// Reply to a `RunSmart` request (Plan 040 M3).
pub struct SmartResult {
    pub output: String,
    pub error: Option<String>,
}

/// State managed by Tauri: a sender into the Shell worker thread + the shared
/// cancel flag (Plan 040 M5). `Clone + Send` — safe to stash in `tauri::State`.
///
/// Cancellation is a *concurrent* signal, not a queued command: setting the
/// flag takes effect immediately in the worker's streaming-drain loop, even
/// while a command is mid-flight (the worker is blocked in `spawn_blocking`
/// and cannot dequeue channel messages until it returns). Routing cancel
/// through the channel would miss the window — the drain loop polls this flag.
#[derive(Clone)]
pub struct ShellHandle {
    tx: mpsc::UnboundedSender<CommandReq>,
    /// Set by `cancel()`; polled by the worker's drain loop between lines.
    cancel: Arc<AtomicBool>,
}

impl ShellHandle {
    /// Submit a command (non-blocking). The result comes back as a Tauri event.
    pub fn submit(&self, block_id: usize, cmd: String) {
        let _ = self.tx.send(CommandReq::Run { block_id, cmd });
    }

    /// Plan 040 M3: run a SmartCommand by name on the worker's Shell (preserves
    /// session cwd/env/functions). Blocks until the worker finishes and replies.
    pub async fn run_smart(
        &self,
        name: String,
        args: Vec<String>,
    ) -> Result<SmartResult, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(CommandReq::RunSmart {
                name,
                args,
                reply: reply_tx,
            })
            .map_err(|_| "worker channel closed".to_string())?;
        reply_rx.await.map_err(|_| "worker dropped reply".to_string())
    }

    /// Cancel the running command (Plan 040 M5). Sets the shared flag, which
    /// the worker's streaming-drain loop checks between each line of output.
    /// Best-effort: only interrupts the streaming path (external commands); a
    /// command blocked in `shell.execute()` finishes on its own.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// Payload emitted on the `command-result` event. Mirrors the TS type.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CommandResult {
    pub block_id: usize,
    pub cwd: String,
    pub status: CommandStatus,
    pub output: RenderedOutput,
    pub duration_ms: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub enum CommandStatus {
    Success,
    Failed(String),
}

/// Spawn the Shell worker thread. Returns a handle for commands + boot data.
pub fn spawn(app: AppHandle) -> ShellHandle {
    let (tx, mut rx) = mpsc::unbounded_channel::<CommandReq>();

    // Plan 040 M5: the shared cancel flag. `cancel_command` flips this directly
    // (concurrent — not via the channel) so it lands even while the worker is
    // blocked inside a command's `spawn_blocking`. The worker resets it to
    // false before each command and polls it in the drain loop.
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_for_thread = cancel.clone();

    // Boot data is produced synchronously on the worker thread (Shell is !Send),
    // then surfaced to the frontend via the `command_list` command which reads
    // this shared snapshot after the worker has initialized.
    let boot = Arc::new(tokio::sync::Mutex::new(None::<BootSnapshot>));
    let boot_for_thread = boot.clone();
    let app_for_thread = app.clone();

    std::thread::Builder::new()
        .name("ash-gui-shell".into())
        .spawn(move || {
            // A small single-threaded Tokio runtime drives the mpsc receiver
            // so we can `.await` channel receives on this thread.
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build worker runtime");

            runtime.block_on(async move {
                let mut shell = auto_shell::Shell::new();

                // Plan 040 M2: bring the worker's Shell to parity with the
                // TUI/CLI REPL init sequence (repl.rs:36-79): env persistence,
                // aliases from ash.toml, ~/.ashrc user functions, plugins.
                init_shell(&mut shell);

                // Plan 040 M1: capture structured output via a RenderHook. Shell
                // calls this from `format_output` *after* full preprocessing.
                let captured: Arc<Mutex<Option<RenderedOutput>>> = Arc::new(Mutex::new(None));
                let hook = CaptureHook {
                    slot: captured.clone(),
                };
                shell.set_render_hook(Box::new(hook));

                // Produce the boot snapshot once, for `command_list`.
                let snapshot = harvest_boot(&shell, &app_for_thread);
                *boot_for_thread.lock().await = Some(snapshot);

                // Main loop: receive commands, run them, emit results.
                while let Some(req) = rx.recv().await {
                    match req {
                        CommandReq::Run { block_id, cmd } => {
                            // Reset the cancel flag for this command.
                            cancel_for_thread.store(false, Ordering::SeqCst);
                            let cwd = shell.pwd().to_string_lossy().to_string();
                            let started = Instant::now();
                            let (status, output) = match run_command(
                                &mut shell,
                                &cmd,
                                &captured,
                                &cancel_for_thread,
                                block_id,
                                &app_for_thread,
                            )
                            .await
                            {
                                Ok(out) => (CommandStatus::Success, out),
                                Err(msg) => {
                                    (CommandStatus::Failed(msg), RenderedOutput::Empty)
                                }
                            };
                            let result = CommandResult {
                                block_id,
                                cwd,
                                status,
                                output,
                                duration_ms: started.elapsed().as_millis() as u64,
                            };
                            // Emit to the main window. `emit` is best-effort at shutdown.
                            let _ = app_for_thread.emit("command-result", result);

                            // Plan 040 M6: persist the command line to the shared
                            // CLI history file so GUI + CLI stay in sync.
                            let _ = append_history(&cmd);
                        }
                        CommandReq::RunSmart { name, args, reply } => {
                            // Plan 040 M3: run the SmartCommand body against the
                            // worker's live Shell (preserves cwd/env/functions).
                            let specs = auto_shell::smart_command::loader::load_all();
                            let result = match specs.iter().find(|s| s.name == name) {
                                Some(spec) => {
                                    match auto_shell::smart_command::executor::execute(
                                        spec, &args, &mut shell,
                                    ) {
                                        Ok(()) => SmartResult {
                                            output: String::new(),
                                            error: None,
                                        },
                                        Err(e) => SmartResult {
                                            output: String::new(),
                                            error: Some(format!("{e}")),
                                        },
                                    }
                                }
                                None => SmartResult {
                                    output: String::new(),
                                    error: Some(format!("SmartCommand '{name}' not found")),
                                },
                            };
                            let _ = reply.send(result);
                        }
                    }
                }
            });
        })
        .expect("failed to spawn ash-gui Shell worker thread");

    // Stash the boot snapshot handle so `command_list` can read it.
    app.manage(BootState(boot));

    ShellHandle { tx, cancel }
}

/// Plan 040 M2: bring the worker Shell to parity with the TUI/CLI REPL init
/// sequence (`ash-tui/src/repl.rs:36-79`):
///   1. env persistence (`~/.config/ash/env.at`)
///   2. aliases from `~/.config/ash.toml`
///   3. `~/.ashrc` user functions / startup script
///   4. installed plugins (`load_all_plugins`)
///
/// Without this, GUI users got a bare Shell: `alias ll='ls -l'` set in config
/// did nothing, `.ashrc` functions were missing, and plugin commands were gone.
/// The pager/terminal commands (less/more/color) are TUI-only and stay out.
pub(crate) fn init_shell(shell: &mut auto_shell::Shell) {
    shell.load_env_persistence();

    // Aliases from ash.toml (mirrors repl.rs:50-55).
    let shell_config = auto_shell::config::AshShellConfig::load();
    for (name, value) in &shell_config.aliases {
        shell.set_alias(name, value);
    }

    // ~/.ashrc — user startup script (mirrors repl.rs:62-73). Seeds the
    // default .ashrc with example functions on first run.
    if let Some(home) = home_dir() {
        let rc_path = home.join(".ashrc");
        if rc_path.exists() {
            let _ = shell.source_file(&rc_path);
        } else if let Ok(content) = std::str::from_utf8(auto_shell::DEFAULT_ASHRC.as_bytes()) {
            let _ = std::fs::write(&rc_path, content);
            let _ = shell.source_file(&rc_path);
        }
    }

    // Plugins (mirrors repl.rs:78-79). Sources each plugin's `functions.ash`
    // and records capability warnings on stderr.
    if let Ok(report) = auto_shell::plugin::load_all_plugins(shell) {
        report.print_to_stderr();
    }
}

/// Snapshot produced at boot for completion + the tool sidebar.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct BootSnapshot {
    pub cwd: String,
    /// Home directory, for the frontend to abbreviate paths with `~`.
    pub home: String,
    pub commands: Vec<ToolEntry>,
    pub smart_commands: Vec<SmartCommandEntry>,
}

#[derive(Serialize, Clone)]
pub struct ToolEntry {
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Clone)]
pub struct SmartCommandEntry {
    pub name: String,
    pub description: String,
}

/// Managed state holding the boot snapshot (filled by the worker thread).
pub struct BootState(pub Arc<tokio::sync::Mutex<Option<BootSnapshot>>>);

/// Harvest the boot snapshot from a Shell (command registry + SmartCommands).
/// Mirrors `ash-gui-bin/src/main.rs:145-172`.
fn harvest_boot(shell: &auto_shell::Shell, _app: &AppHandle) -> BootSnapshot {
    let reg = shell.registry();
    let mut names: Vec<String> = reg.names().cloned().collect();
    names.sort();

    let commands: Vec<ToolEntry> = names
        .iter()
        .filter_map(|n| {
            reg.get(n).map(|cmd| {
                let sig = cmd.signature();
                ToolEntry {
                    name: sig.name,
                    description: sig.description,
                }
            })
        })
        .collect();

    let specs = auto_shell::smart_command::loader::load_all();
    let smart_commands: Vec<SmartCommandEntry> = specs
        .iter()
        .map(|s| SmartCommandEntry {
            name: s.name.clone(),
            description: s.description.clone(),
        })
        .collect();

    let home = home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();

    BootSnapshot {
        cwd: shell.pwd().to_string_lossy().to_string(),
        home,
        commands,
        smart_commands,
    }
}

// ── Command dispatch (Plan 040 M1: full preprocessing + structured capture) ──

/// A [`auto_shell::shell::RenderHook`] that captures the structured output
/// Shell produces in `format_output`, *after* full expansion. Returns `None`
/// so Shell still falls back to plain text (we prefer the captured structure).
struct CaptureHook {
    /// Receives the last structured output, if any (cleared per command).
    slot: Arc<Mutex<Option<RenderedOutput>>>,
}

impl auto_shell::shell::RenderHook for CaptureHook {
    fn render_structured(
        &self,
        rendered: &RenderedOutput,
        _term_width: u16,
        _icons: auto_shell::config::IconStyle,
    ) -> Option<String> {
        // Clone the structured output for the GUI; let Shell fall through to
        // text so `execute` still returns a usable string.
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(rendered.clone());
        }
        None
    }
}

/// Run a single command against the Shell and return its [`RenderedOutput`].
///
/// Two paths:
/// - **M4 streaming path**: for a *simple external command* (no redirect, no
///   pipe `|`, no chain `&&`/`||`, no DSL stage `.x`, not an Auto expression,
///   not a registered/builtin/Auto-function command) we spawn it via
///   [`spawn_external_stream`] and drain stdout line-by-line, emitting a
///   `command-output` chunk per line. This gives live feedback for long
///   commands (`find /`, a build, …) instead of one delayed result. The final
///   `command-result` carries the accumulated text. The drain loop also checks
///   the cancel flag (M5) so a `Cancel` request stops the stream early.
/// - **Default path**: everything else goes through `shell.execute()` (the
///   complete preprocessing pipeline, M1) and prefers the structured output
///   captured by [`CaptureHook`]; otherwise the text `execute` returned.
async fn run_command(
    shell: &mut auto_shell::Shell,
    cmd: &str,
    captured: &Arc<Mutex<Option<RenderedOutput>>>,
    cancel: &Arc<AtomicBool>,
    block_id: usize,
    app: &AppHandle,
) -> Result<RenderedOutput, String> {
    // Clear the capture slot before executing this command.
    if let Ok(mut slot) = captured.lock() {
        *slot = None;
    }

    // Plan 040 M4: try the streaming path for simple external commands.
    if let Some(rendered) =
        run_streaming_external(shell, cmd, cancel, block_id, app).await?
    {
        return Ok(rendered);
    }

    // Default path: full preprocessing via shell.execute().
    let result = shell.execute(cmd);

    // Structured output (Table/Record) captured during format_output?
    if let Ok(slot) = captured.lock() {
        if let Some(rendered) = slot.as_ref() {
            if !matches!(rendered, RenderedOutput::Empty) {
                return Ok(rendered.clone());
            }
        }
    }

    // No structured output — use the execute result (text or empty).
    match result {
        Ok(Some(s)) => Ok(RenderedOutput::Text(s)),
        Ok(None) => Ok(RenderedOutput::Empty),
        Err(e) => Err(format!("{e}")),
    }
}

/// Plan 040 M4: the shell-level keywords handled inside `execute_inner`
/// (alias/source/pushd/popd/dirs/jobs/fg/bg/def/hook/abbr/config/bind/up/u/b/
/// set/export/unset/env/path/completions/suspend) plus `cd`. These are never
/// real external processes, so the streaming path must skip them. Mirrors the
/// dispatch in `shell.rs:585-695` and the `EXTRA_BUILTINS` set (shell.rs:4323).
fn is_shell_builtin(name: &str) -> bool {
    matches!(
        name,
        "cd"
            | "alias"
            | "unalias"
            | "source"
            | "."
            | "pushd"
            | "popd"
            | "dirs"
            | "jobs"
            | "fg"
            | "bg"
            | "suspend"
            | "def"
            | "hook"
            | "abbr"
            | "config"
            | "bind"
            | "up"
            | "u"
            | "b"
            | "set"
            | "export"
            | "unset"
            | "env"
            | "env.path"
            | "path"
            | "completions"
            | "use"
            | "exit"
            | "quit"
            | "q"
    )
}

/// Plan 040 M4: streaming execution for a *simple external command*.
///
/// Returns `Ok(Some(_))` if the command was streamed (with a final rendered
/// output), or `Ok(None)` if it isn't eligible and the caller should fall back
/// to `shell.execute()`. Eligibility mirrors how `execute_inner` dispatches: a
/// command is streamable only if it would reach the "external command" branch —
/// i.e. it's not a registered command, builtin, Auto function, Auto expression,
/// or a redirect/pipe/chain. We also skip DSL pipe-stages (`.filter`, …) and
/// job-control/background forms.
///
/// On the streaming path we spawn the child directly with piped stdout and
/// drain it line-by-line, emitting `command-output` chunks for live feedback.
/// The cancel flag (M5) short-circuits the drain.
async fn run_streaming_external(
    shell: &auto_shell::Shell,
    cmd: &str,
    cancel: &Arc<AtomicBool>,
    block_id: usize,
    app: &AppHandle,
) -> Result<Option<RenderedOutput>, String> {
    use ash_core::cmd::external as ext;
    use ash_core::parser::{pipeline as chain_parser, pipe_stages, redirect};

    let trimmed = cmd.trim();
    // Empty / nothing to run.
    if trimmed.is_empty() {
        return Ok(None);
    }

    // Reject job-control / background / chain operators up front — those need
    // shell.execute()'s full machinery.
    if trimmed.ends_with('&') {
        return Ok(None);
    }
    // A redirect target present? Leave to execute() (it applies the redirect).
    let (_clean, redirect) = redirect::parse_redirect(trimmed);
    if let Some(redir) = &redirect {
        if redir.stdout.is_some() || redir.stdin.is_some() {
            return Ok(None);
        }
    }

    // Chain (&&/||) or multi-segment pipeline (|)? execute() handles those.
    let segments = chain_parser::parse_chain(trimmed);
    let has_logic = segments
        .iter()
        .any(|s| matches!(s.op, Some(chain_parser::ChainOp::And | chain_parser::ChainOp::Or)));
    if has_logic {
        return Ok(None);
    }
    let pipe_cmds: Vec<String> = segments.into_iter().map(|s| s.command).collect();
    if pipe_cmds.len() != 1 {
        return Ok(None); // a real `|` pipeline — not single-external
    }
    let single = &pipe_cmds[0];

    // A structured-pipeline DSL stage (`.filter`, `.sort`, …)? execute() path.
    if pipe_stages::parse_pipe_stage(single).is_some() {
        return Ok(None);
    }

    // Parse the command into parts to classify it.
    let parts = ext::parse_command(single);
    if parts.is_empty() {
        return Ok(None);
    }
    let cmd_name = &parts[0];

    // Auto expression? (the VM path). Defer to execute().
    if shell.is_auto_expression_pub(single) {
        return Ok(None);
    }
    // Registered command / builtin / Auto function? Not external — defer.
    if shell.registry().get(cmd_name).is_some() {
        return Ok(None);
    }
    if shell.has_auto_function(cmd_name) {
        return Ok(None);
    }
    // Alias? execute_inner expands the first word, so defer to it.
    if shell.aliases().contains_key(cmd_name.as_str()) {
        return Ok(None);
    }
    // Shell-level keywords handled inside execute_inner / execute_builtin —
    // never real processes. `is_legacy_builtin` covers echo/pwd/ls/grep/…;
    // the extra list mirrors the `EXTRA_BUILTINS` set in shell.rs:4323-4328
    // (alias/source/pushd/popd/dirs/jobs/fg/bg/set/export/unset/…). Spawning
    // any of these as an external process would be wrong.
    if auto_shell::cmd::builtin::is_legacy_builtin(cmd_name) || is_shell_builtin(cmd_name) {
        return Ok(None);
    }

    // Eligible simple external command — spawn it and stream stdout.
    let cwd = shell.pwd();
    let stream = match ext::spawn_external_stream(single, &cwd) {
        Ok(s) => s,
        Err(e) => {
            // Mirror execute_single_command's "did you mean?" UX: the error
            // string carries the exit code for known failures. Surface it.
            return Err(format!("{e}"));
        }
    };

    // Drain line-by-line, emitting chunks. Use a blocking read on this thread
    // (the worker runtime is current-thread + enable_all; spawn_blocking keeps
    // the async loop responsive and lets channel `Cancel` messages land).
    let cancel_clone = cancel.clone();
    let app_clone = app.clone();
    let rendered = tokio::task::spawn_blocking(move || {
        drain_stream(stream, block_id, &cancel_clone, &app_clone)
    })
    .await
    .map_err(|e| format!("streaming task failed: {e}"))?;

    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".to_string());
    }
    Ok(Some(rendered))
}

/// Drain an [`ExternalStream`] line-by-line, emitting a `command-output` chunk
/// per line. Returns the accumulated text as a [`RenderedOutput::Text`] (or
/// `Empty` if there was no output). Honours the cancel flag (M5): stops reading
/// and returns what it has so far.
fn drain_stream(
    stream: ash_core::pipeline::ExternalStream,
    block_id: usize,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
) -> RenderedOutput {
    let mut buf = String::new();
    for line in stream.lines() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        match line {
            Ok(l) => {
                buf.push_str(&l);
                buf.push('\n');
                let _ = app.emit(
                    "command-output",
                    CommandOutput {
                        block_id,
                        chunk: format!("{l}\n"),
                    },
                );
            }
            Err(_) => break, // broken pipe → stop
        }
    }
    if buf.is_empty() {
        RenderedOutput::Empty
    } else {
        RenderedOutput::Text(buf)
    }
}

/// Payload emitted on the `command-output` event (Plan 040 M4). One per line of
/// streamed output from a long external command, before the final
/// `command-result`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CommandOutput {
    pub block_id: usize,
    pub chunk: String,
}

// ── History persistence (Plan 040 M6) ──────────────────────────────────────

/// Resolve the user's home directory via env vars (`USERPROFILE` on Windows,
/// `HOME` elsewhere). `None` if neither is set.
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
}

/// The shared CLI history path (`~/.auto-shell-history`). Same file the TUI/CLI
/// REPL (`repl.rs:306-312`) and `read_history_file` consume. Returns `None` if
/// the home directory can't be resolved.
fn history_file() -> Option<std::path::PathBuf> {
    home_dir().map(|h| h.join(".auto-shell-history"))
}

/// Append a command line to the shared history file (Plan 040 M6). Best-effort:
/// a missing/unwritable file is silently ignored. The format is one command per
/// line, matching `reedline::FileBackedHistory` and `read_history_file`.
fn append_history(cmd: &str) -> std::io::Result<()> {
    use std::io::Write;
    let line = cmd.trim();
    if line.is_empty() {
        return Ok(());
    }
    let path = match history_file() {
        Some(p) => p,
        None => return Ok(()),
    };
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    // Escape embedded newlines so one history entry never spans multiple lines
    // (the file is line-oriented).
    let safe = line.replace('\n', " ");
    writeln!(f, "{safe}")?;
    Ok(())
}

/// Read the entire history file (one command per line, oldest first).
/// Returns an empty vec if the file is missing/unreadable. Mirrors the TUI
/// `read_history_file` (`ash-tui/src/repl.rs:1072`).
pub fn read_history() -> Vec<String> {
    let path = match history_file() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}
