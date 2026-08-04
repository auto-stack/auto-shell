//! Shell worker — owns the `!Send` Shell on a dedicated OS thread.
//!
//! `auto_shell::Shell` holds an `AutovmReplSession` whose type registry is
//! `Rc<RefCell<…>>` (single-threaded), so it cannot cross a thread boundary.
//! Like the iced ash-gui (`ash-gui-bin/src/main.rs`), we keep the Shell on one
//! dedicated thread and communicate via channels. Here the "GUI" is the Tauri
//! event bus: the worker emits `command-result` events when commands finish.

use std::sync::Arc;
use std::time::Instant;

use ash_core::pipeline::AtomPipeline;
use ash_core::renderer::{render_pipeline_to_structured, RenderedOutput};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

/// A request from a Tauri command: run `cmd`, attribute the result to `block_id`.
pub struct CommandReq {
    pub block_id: usize,
    pub cmd: String,
}

/// State managed by Tauri: a sender into the Shell worker thread.
/// `Clone + Send` — safe to stash in `tauri::State`.
#[derive(Clone)]
pub struct ShellHandle {
    tx: mpsc::UnboundedSender<CommandReq>,
}

impl ShellHandle {
    /// Submit a command (non-blocking). The result comes back as a Tauri event.
    pub fn submit(&self, block_id: usize, cmd: String) {
        let _ = self.tx.send(CommandReq { block_id, cmd });
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
                shell.load_env_persistence();

                // Produce the boot snapshot once, for `command_list`.
                let snapshot = harvest_boot(&shell, &app_for_thread);
                *boot_for_thread.lock().await = Some(snapshot);

                // Main loop: receive commands, run them, emit results.
                while let Some(req) = rx.recv().await {
                    let cwd = shell.pwd().to_string_lossy().to_string();
                    let started = Instant::now();
                    let (status, output) = match run_command(&mut shell, &req.cmd) {
                        Ok(out) => (CommandStatus::Success, out),
                        Err(msg) => {
                            (CommandStatus::Failed(msg), RenderedOutput::Empty)
                        }
                    };
                    let result = CommandResult {
                        block_id: req.block_id,
                        cwd,
                        status,
                        output,
                        duration_ms: started.elapsed().as_millis() as u64,
                    };
                    // Emit to the main window. `emit` is best-effort at shutdown.
                    let _ = app_for_thread.emit("command-result", result);
                }
            });
        })
        .expect("failed to spawn ash-gui Shell worker thread");

    // Stash the boot snapshot handle so `command_list` can read it.
    app.manage(BootState(boot));

    ShellHandle { tx }
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

    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| String::new());

    BootSnapshot {
        cwd: shell.pwd().to_string_lossy().to_string(),
        home,
        commands,
        smart_commands,
    }
}

// ── Command dispatch (mirrors ash-gui-bin/src/main.rs:335-367) ────────────────

/// Run a single command against the Shell and return its [`RenderedOutput`].
fn run_command(shell: &mut auto_shell::Shell, cmd: &str) -> Result<RenderedOutput, String> {
    // Structured path: parse → run_atom → AtomPipeline → RenderedOutput.
    if let Some(rendered) = render_structured(shell, cmd) {
        return Ok(rendered);
    }
    // Fallback: plain execute → text. Covers non-atom commands (echo, external).
    match shell.execute(cmd) {
        Ok(Some(s)) => Ok(RenderedOutput::Text(s)),
        Ok(None) => Ok(RenderedOutput::Empty),
        Err(e) => Err(format!("{e}")),
    }
}

/// Run a command via the registry and render its AtomPipeline.
/// Returns `None` for commands that don't go through the atom path.
fn render_structured(shell: &mut auto_shell::Shell, input: &str) -> Option<RenderedOutput> {
    let parts = ash_core::parser::parse_args(input);
    if parts.is_empty() {
        return None;
    }
    let cmd_name = &parts[0];
    let cmd = shell.registry().get(cmd_name)?;
    let signature = cmd.signature();
    let args = &parts[1..];
    let parsed = auto_shell::cmd::parser::parse_args(&signature, args).ok()?;
    if parsed.help_requested {
        return Some(RenderedOutput::Text(signature.format_help()));
    }
    let pipeline: AtomPipeline = cmd
        .run_atom(&parsed, AtomPipeline::empty(), shell)
        .ok()?;
    render_pipeline_to_structured(&pipeline).or(Some(RenderedOutput::Text(pipeline.into_text())))
}
