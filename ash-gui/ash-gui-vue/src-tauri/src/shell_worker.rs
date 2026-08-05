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
    /// `block_id` attributes streamed body output to the frontend's block.
    RunSmart {
        block_id: usize,
        name: String,
        args: Vec<String>,
        reply: tokio::sync::oneshot::Sender<SmartResult>,
    },
    /// Plan 041 M7: produce completions for `line` at `cursor`, using the
    /// worker's live Shell state (cwd/history/aliases). Runs the shared
    /// completion engine (`auto_shell::completions::engine::complete`) on the
    /// worker thread so the provider/specs stay in sync with the session.
    Complete {
        line: String,
        cursor: usize,
        reply: tokio::sync::oneshot::Sender<Vec<CompletionItem>>,
    },
    /// Plan 041 M5: get the prompt context (git branch/status) for the
    /// worker's current directory. Refreshes the global git cache, then
    /// returns the cached info (never blocks).
    PromptContext {
        reply: tokio::sync::oneshot::Sender<PromptContext>,
    },
}

/// Plan 041 M7: one completion candidate, serialized for the frontend. Mirrors
/// `auto_shell::completions::Completion` (terminal-independent core type).
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CompletionItem {
    /// What to insert into the input.
    pub replacement: String,
    /// What to show in the completion menu (may differ from replacement).
    pub display: String,
    /// Optional one-line description (e.g. "Reverse sort order" for `-r`).
    pub description: Option<String>,
    /// Semantic kind: command / file / flag / directory / variable / ...
    pub kind: String,
}

/// Plan 041 M5: prompt context (git branch/status) for the GUI title bar.
/// Mirrors `auto_shell::prompt::context::GitInfo` / `GitStatus`.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct PromptContext {
    /// Current git branch, or None if not in a git repo.
    pub git_branch: Option<String>,
    /// Working tree status counts (staged/unstaged/untracked/ahead/behind).
    pub git_status: Option<GitStatusInfo>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct GitStatusInfo {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub ahead: usize,
    pub behind: usize,
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

    /// Plan 041 M5: get the prompt context (git branch/status) for the
    /// worker's current directory.
    pub async fn prompt_context(&self) -> Result<PromptContext, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(CommandReq::PromptContext { reply: reply_tx })
            .map_err(|_| "worker channel closed".to_string())?;
        reply_rx.await.map_err(|_| "worker dropped reply".to_string())
    }

    /// Plan 041 M7: produce completions on the worker's Shell (live cwd/
    /// history/aliases). Blocks until the worker finishes and replies. Returns
    /// the shared engine's candidates, serialized for the frontend.
    pub async fn complete(&self, line: String, cursor: usize) -> Result<Vec<CompletionItem>, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(CommandReq::Complete {
                line,
                cursor,
                reply: reply_tx,
            })
            .map_err(|_| "worker channel closed".to_string())?;
        reply_rx.await.map_err(|_| "worker dropped reply".to_string())
    }

    /// Plan 040 M3: run a SmartCommand by name on the worker's Shell (preserves
    /// session cwd/env/functions). Blocks until the worker finishes and replies.
    /// `block_id` attributes streamed body output to the frontend's block.
    pub async fn run_smart(
        &self,
        block_id: usize,
        name: String,
        args: Vec<String>,
    ) -> Result<SmartResult, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(CommandReq::RunSmart {
                block_id,
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

                // Plan 040 M3: inject the OutputHook so a SmartCommand body's
                // per-command output streams to the frontend. The block slot is
                // retargeted per SmartCommand (None between them).
                let smart_block: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
                shell.set_output_hook(Box::new(StreamingOutputHook {
                    block: smart_block.clone(),
                    app: app_for_thread.clone(),
                }));

                // Produce the boot snapshot once, for `command_list`.
                let snapshot = harvest_boot(&shell, &app_for_thread);
                *boot_for_thread.lock().await = Some(snapshot);

                // Plan 041 M7: build the completion engine's inputs once (mirrors
                // repl.rs:89-95): registry signatures + a CompletionProvider with
                // built-in + tier + plugin specs. These live on the worker thread
                // (alongside the Shell) and feed `engine::complete` on each request.
                let completion_sigs: Vec<auto_shell::completions::CompletionSignature> =
                    shell.registry().params().into_iter().map(Into::into).collect();
                let mut completion_provider = auto_shell::completions::CompletionProvider::new();
                auto_shell::completions::definitions::register_all(&mut completion_provider);
                // Tier specs (generated > user > plugin), same overlay as ShellCompleter.
                load_tier_specs(&mut completion_provider);

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
                        CommandReq::RunSmart { block_id, name, args, reply } => {
                            // Plan 040 M3: run the SmartCommand body against the
                            // worker's live Shell (preserves cwd/env/functions).
                            // Retarget the OutputHook to this block so the body's
                            // per-command output streams as `command-output` chunks.
                            if let Ok(mut slot) = smart_block.lock() {
                                *slot = Some(block_id);
                            }
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
                            // Clear the slot so later normal commands don't leak.
                            if let Ok(mut slot) = smart_block.lock() {
                                *slot = None;
                            }
                            let _ = reply.send(result);
                        }
                        CommandReq::Complete { line, cursor, reply } => {
                            // Plan 041 M7: run the shared completion engine on the
                            // worker thread (live cwd/history/aliases), return
                            // serialized candidates to the frontend.
                            let ctx = completion_ctx(&shell);
                            let completions = auto_shell::completions::engine::complete(
                                &line,
                                cursor,
                                &completion_sigs,
                                &mut completion_provider,
                                &ctx,
                            );
                            let items: Vec<CompletionItem> =
                                completions.into_iter().map(completion_to_item).collect();
                            let _ = reply.send(items);
                        }
                        CommandReq::PromptContext { reply } => {
                            // Plan 041 M5: refresh the global git cache for the
                            // worker's cwd, then return the cached info.
                            let cwd = shell.pwd().to_path_buf();
                            auto_shell::prompt::context::refresh_git_info_async(cwd);
                            let ctx = auto_shell::prompt::context::AshContext::new(
                                shell.pwd().to_path_buf(),
                                home_dir().unwrap_or_default(),
                                None,
                                Some(shell.last_exit_code()),
                                auto_shell::prompt::AshConfig::default(),
                            );
                            let pc = match ctx.git_info() {
                                Some(gi) => PromptContext {
                                    git_branch: Some(gi.branch),
                                    git_status: Some(GitStatusInfo {
                                        staged: gi.status.staged,
                                        unstaged: gi.status.unstaged,
                                        untracked: gi.status.untracked,
                                        conflicted: gi.status.conflicted,
                                        ahead: gi.status.ahead,
                                        behind: gi.status.behind,
                                    }),
                                },
                                None => PromptContext::default(),
                            };
                            let _ = reply.send(pc);
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

/// Plan 041 M7: load generated → user → plugin tier completion specs into the
/// provider (override order: plugin > user > generated > built-in). Mirrors
/// `ShellCompleter::load_tier_specs` (ash-tui) so GUI + TUI resolve the same specs.
fn load_tier_specs(provider: &mut auto_shell::completions::CompletionProvider) {
    if let Some(dir) = auto_shell::completions::spec_tiers::generated_dir() {
        for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
            provider.register(spec);
        }
    }
    if let Some(dir) = auto_shell::completions::spec_tiers::user_dir() {
        for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
            provider.register(spec);
        }
    }
    for dir in auto_shell::plugin::loader::enabled_plugin_completion_dirs() {
        for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
            provider.register(spec);
        }
    }
}

/// Plan 041 M7: snapshot the worker Shell's live state into the engine's
/// context type (cwd / last command / exit code / history / aliases).
fn completion_ctx(shell: &auto_shell::Shell) -> auto_shell::completions::engine::CompletionCtx {
    // Recent history (bounded window, same as repl.rs:362-365 `sync_completion_state`).
    let history = history_file()
        .map(|p| read_recent_history(&p, 50))
        .unwrap_or_default();
    auto_shell::completions::engine::CompletionCtx {
        current_dir: shell.pwd().to_path_buf(),
        last_command: shell.last_command_line().map(String::from),
        last_exit_code: Some(shell.last_exit_code()),
        history,
        aliases: shell.aliases().clone(),
    }
}

/// Plan 041 M7: map the engine's core `Completion` to the serialized
/// `CompletionItem` the frontend receives.
fn completion_to_item(c: auto_shell::completions::Completion) -> CompletionItem {
    use auto_shell::completions::CompletionKind;
    let kind = match c.kind {
        CompletionKind::Command => "command",
        CompletionKind::External => "external",
        CompletionKind::File => "file",
        CompletionKind::Directory => "directory",
        CompletionKind::Variable => "variable",
        CompletionKind::Flag => "flag",
        CompletionKind::Subcommand => "subcommand",
        CompletionKind::AiSuggested => "ai",
    };
    CompletionItem {
        replacement: c.replacement,
        display: c.display,
        description: c.description,
        kind: kind.to_string(),
    }
}
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

/// Plan 040 M3: an [`auto_shell::shell::OutputHook`] that forwards a
/// SmartCommand body's per-command output to the frontend as `command-output`
/// chunks (same event the streaming path uses). The current `block_id` is held
/// in a shared slot so the worker can retarget the hook per SmartCommand.
struct StreamingOutputHook {
    /// The block_id to attribute output to; `None` means "not running a
    /// SmartCommand" (output is dropped — the normal `execute()` path already
    /// returns text via the worker loop).
    block: Arc<Mutex<Option<usize>>>,
    app: AppHandle,
}

impl auto_shell::shell::OutputHook for StreamingOutputHook {
    fn emit(&self, output: &str) {
        if let Ok(slot) = self.block.lock() {
            if let Some(block_id) = *slot {
                let _ = self.app.emit(
                    "command-output",
                    CommandOutput {
                        block_id,
                        chunk: output.to_string(),
                    },
                );
            }
        }
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

/// Plan 041 M8: terminal-only commands that the GUI cannot support (they need
/// crossterm raw mode + alternate screen, which a webview doesn't have). These
/// are registered in `ash-tui::terminal_commands()` for the CLI/TUI, but the
/// GUI worker doesn't call `register_commands` — so they'd otherwise fall
/// through to spawning the system binary into a TTY-less context.
fn is_terminal_only_command(name: &str) -> bool {
    matches!(name, "less" | "more" | "color")
}

/// Plan 041 M8: the friendly message shown when a user runs a terminal-only
/// command in the GUI. Explains why it's not available and what to do instead.
fn terminal_only_message(name: &str) -> String {
    match name {
        "less" | "more" => format!(
            "{name} 是终端翻页器,需要交互式终端(raw mode + 键盘控制)。\n\
             在 GUI 里,长输出已在上方块区可滚动浏览(M4 流式输出)——无需 {name}。\n\
             如需完整的终端交互,请用 CLI 版 ash 运行此命令。"
        ),
        "color" => String::from(
            "color 显示终端的 24-bit 真彩能力,依赖 crossterm 终端 API。\n\
             GUI 走 webview CSS 渲染,无终端颜色概念——此命令不适用。",
        ),
        _ => format!("{name} 是终端专属命令,GUI 不支持。"),
    }
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
    // Plan 041 M8: terminal-only commands (less/more/color) need crossterm raw
    // mode + alternate screen — meaningless in a webview GUI. The worker
    // doesn't register them (no register_commands), so without this guard
    // they'd fall through to spawning the system binary, which would try to
    // take over a non-existent TTY. Intercept and give a friendly note instead.
    if is_terminal_only_command(cmd_name) {
        return Ok(Some(RenderedOutput::Text(terminal_only_message(cmd_name))));
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
    // Plan 040 M5: capture a kill handle before the stream is consumed by
    // lines(), so we can terminate the child on cancel (not just stop reading).
    let kill = stream.kill_handle();
    let rendered = tokio::task::spawn_blocking(move || {
        drain_stream(stream, block_id, &cancel_clone, &kill, &app_clone)
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
/// and **kills the child process** via `kill` so it doesn't keep running as an
/// orphan after we stop reading its stdout.
fn drain_stream(
    stream: ash_core::pipeline::ExternalStream,
    block_id: usize,
    cancel: &Arc<AtomicBool>,
    kill: &Arc<Mutex<Option<u32>>>,
    app: &AppHandle,
) -> RenderedOutput {
    let mut buf = String::new();
    let mut cancelled = false;
    for line in stream.lines() {
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
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
    // Plan 040 M5: if cancelled, kill the child process so it doesn't outlive
    // the read loop as an orphan. Best-effort: one-shot (clears the PID), and
    // the process may have already exited.
    if cancelled {
        ash_core::pipeline::ExternalStream::kill_from_handle(kill);
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

/// Read the last `n` history entries (Plan 041 M7, for completion context).
/// Mirrors `ash-tui/src/repl.rs::read_recent_history`. Returns oldest-first.
fn read_recent_history(path: &std::path::Path, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let all = read_history_file(path);
    let len = all.len();
    if len <= n {
        all
    } else {
        all[len - n..].to_vec()
    }
}

/// Read history entries from a file (one command per line). Mirrors
/// `ash-tui/src/repl.rs::read_history_file`.
fn read_history_file(path: &std::path::Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(c) => c
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}
