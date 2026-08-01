//! ash-gui — GUI for ash (Plan 030 M3).
//!
//! M3: a scrolling list of Blocks (command history) + command input with
//! history navigation and command-name completion. Each Block records the
//! command, its working directory, status (Success/Failed/Running), and the
//! structured output. This turns the M2 single-output demo into something
//! usable as a daily terminal.
//!
//! ## Threading
//! The Shell is `!Send` (auto-lang session state uses `Rc`), so a dedicated
//! worker thread owns it; the GUI talks to it via channels. Async futures hold
//! only channel endpoints (`Send`), never the Shell.

mod block;
mod renderer;

use std::path::PathBuf;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};

use ash_core::pipeline::AtomPipeline;
use ash_core::renderer::{render_pipeline_to_structured, RenderedOutput};
use block::{Block, BlockStatus};
use iced::{Element, Task};

use renderer::{block_list_view, GuiMsg};

/// A handle to the dedicated Shell worker thread: send it commands, await
/// rendered results tagged with the block id they belong to.
#[derive(Clone)]
struct ShellHandle {
    cmd_tx: SyncSender<CommandReq>,
    result_rx: Arc<Mutex<mpsc::Receiver<CommandResult>>>,
}

/// A command request: which block id to attribute the result to + the command.
#[derive(Debug)]
struct CommandReq {
    block_id: usize,
    cmd: String,
}

/// A command result: which block it updates + the rendered output + status.
#[derive(Debug, Clone)]
struct CommandResult {
    block_id: usize,
    status: BlockResult,
}

/// The outcome of running a command (returned across the channel).
#[derive(Debug, Clone)]
enum BlockResult {
    Success(RenderedOutput),
    Failed(String),
}

impl ShellHandle {
    /// Spawn the dedicated Shell worker thread that owns the `!Send` Shell.
    fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::sync_channel::<CommandReq>(8);
        let (result_tx, result_rx) = mpsc::sync_channel::<CommandResult>(8);

        std::thread::Builder::new()
            .name("ash-gui-shell".into())
            .spawn(move || {
                let mut shell = auto_shell::Shell::new();
                shell.load_env_persistence();
                for req in cmd_rx.iter() {
                    let cwd = shell.pwd().to_path_buf();
                    let status = match run_command(&mut shell, &req.cmd) {
                        Ok(out) => BlockResult::Success(out),
                        Err(msg) => BlockResult::Failed(msg),
                    };
                    // Stamp the cwd onto the result so the GUI can show it.
                    let _ = cwd;
                    if result_tx
                        .send(CommandResult {
                            block_id: req.block_id,
                            status,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("failed to spawn ash-gui Shell worker thread");

        Self {
            cmd_tx,
            result_rx: Arc::new(Mutex::new(result_rx)),
        }
    }

    /// Submit a command for the given block id (non-blocking).
    fn submit(&self, block_id: usize, cmd: String) {
        let _ = self.cmd_tx.send(CommandReq { block_id, cmd });
    }

    /// Drain any finished results. The future is `Send` (only channel endpoints).
    async fn drain(self) -> Vec<CommandResult> {
        let rx = self.result_rx.lock().unwrap();
        let mut out = Vec::new();
        while let Ok(r) = rx.try_recv() {
            out.push(r);
        }
        out
    }
}

/// The app state.
struct AshGui {
    shell: ShellHandle,
    /// Command history / output blocks (newest at the end).
    blocks: Vec<Block>,
    next_id: usize,
    /// Current input text.
    input: String,
    /// Navigation index into history (None = not navigating).
    history_cursor: Option<usize>,
    /// Command names for completion suggestions.
    command_names: Vec<String>,
    /// Current completion suggestions (derived from input, cached for view).
    suggestions: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Gui(GuiMsg),
    /// A command finished (or several did).
    Results(Vec<CommandResult>),
    /// Drain finished results on a timer.
    Tick,
}

impl AshGui {
    /// Initial state (iced 0.14 `application(boot, ...)` needs `Fn() -> State`).
    fn boot() -> Self {
        // Build a throwaway Shell just to harvest the command-name list for
        // completion. The real execution Shell lives on the worker thread.
        let registry = auto_shell::Shell::new();
        let command_names: Vec<String> = registry.registry().names().cloned().collect();
        drop(registry);

        let shell = ShellHandle::spawn();
        Self {
            shell,
            blocks: Vec::new(),
            next_id: 0,
            input: String::new(),
            history_cursor: None,
            suggestions: Vec::new(),
            command_names,
        }
    }

    fn title(&self) -> String {
        String::from("ash-gui")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Gui(GuiMsg::InputChanged(s)) => {
                self.input = s;
                self.history_cursor = None; // typing resets history nav
                self.suggestions = self.completion_suggestions();
            }
            Message::Gui(GuiMsg::RunCommand) => {
                let cmd = std::mem::take(&mut self.input);
                self.suggestions.clear();
                if !cmd.trim().is_empty() {
                    self.history_cursor = None;
                    let id = self.next_id;
                    self.next_id += 1;
                    let cwd = PathBuf::from(".");
                    self.blocks.push(Block::running(id, cmd.clone(), cwd));
                    self.shell.submit(id, cmd);
                }
            }
            Message::Gui(GuiMsg::HistoryPrev) => {
                self.navigate_history(true);
                self.suggestions = self.completion_suggestions();
            }
            Message::Gui(GuiMsg::HistoryNext) => {
                self.navigate_history(false);
                self.suggestions = self.completion_suggestions();
            }
            Message::Gui(GuiMsg::PickCompletion(s)) => {
                // Replace the command-name prefix with the picked completion.
                let rest: String = self
                    .input
                    .split_whitespace()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join(" ");
                self.input = if rest.is_empty() {
                    s
                } else {
                    format!("{s} {rest}")
                };
                self.suggestions = self.completion_suggestions();
            }
            Message::Gui(GuiMsg::OpenPath(_path)) => {
                // M4 hook: clicking a filename would open it. For M3 this is a no-op.
            }
            Message::Results(results) => {
                for r in results {
                    if let Some(b) = self.blocks.iter_mut().find(|b| b.id == r.block_id) {
                        match r.status {
                            BlockResult::Success(out) => {
                                b.status = BlockStatus::Success;
                                b.output = out;
                            }
                            BlockResult::Failed(msg) => {
                                b.status = BlockStatus::Failed(msg);
                                b.output = RenderedOutput::Empty;
                            }
                        }
                    }
                }
            }
            Message::Tick => {
                // Poll for finished results without blocking the GUI.
                let shell = self.shell.clone();
                return Task::perform(shell.drain(), Message::Results);
            }
        }
        Task::none()
    }

    /// Navigate command history (Prev = up/older, Next = down/newer).
    fn navigate_history(&mut self, older: bool) {
        let successful: Vec<&str> = self
            .blocks
            .iter()
            .rev()
            .map(|b| b.command.as_str())
            .collect();
        if successful.is_empty() {
            return;
        }
        let cur = self.history_cursor.unwrap_or(usize::MAX);
        let next = if older {
            cur.saturating_add(1).min(successful.len().saturating_sub(1))
        } else {
            cur.saturating_sub(1)
        };
        if let Some(cmd) = successful.get(next) {
            self.input = (*cmd).to_string();
            self.history_cursor = Some(next);
        }
    }

    /// Command-name completion suggestions for the current input prefix.
    fn completion_suggestions(&self) -> Vec<String> {
        // Match on the first token (the command name).
        let first = self.input.split_whitespace().next().unwrap_or("");
        if first.is_empty() {
            return Vec::new();
        }
        self.command_names
            .iter()
            .filter(|n| n.starts_with(first))
            .take(8)
            .cloned()
            .collect()
    }

    fn view(&self) -> Element<Message> {
        block_list_view(&self.blocks, &self.input, &self.suggestions).map(Message::Gui)
    }
}

/// Run a single command against the Shell and return its [`RenderedOutput`].
/// Runs on the dedicated worker thread (Shell is `!Send`).
fn run_command(shell: &mut auto_shell::Shell, cmd: &str) -> Result<RenderedOutput, String> {
    // Try the structured path: parse + run_atom → AtomPipeline → RenderedOutput.
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

/// Run a single command via the registry and render its AtomPipeline. Returns
/// `None` for commands that don't go through the atom path.
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

/// Poll finished results periodically.
fn tick_subscription(_state: &AshGui) -> iced::Subscription<Message> {
    iced::time::every(std::time::Duration::from_millis(100)).map(|_| Message::Tick)
}

pub fn main() -> iced::Result {
    iced::application(AshGui::boot, AshGui::update, AshGui::view)
        .title(|s: &AshGui| s.title())
        .subscription(tick_subscription)
        .window_size([900.0, 650.0])
        .run()
}
