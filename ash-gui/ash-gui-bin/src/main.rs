//! ash-gui — minimal GUI for ash (Plan 030 M2).
//!
//! Proves the core hypothesis: a structured Atom (e.g. `ls` → FileList) renders
//! as a rich, interactive widget (a table) in a GUI — something the TUI can't
//! do. The Shell engine runs in-process (design §1.5); the GUI consumes ash-core's
//! frontend-agnostic `RenderedOutput` (Plan 030 M1) instead of formatted text.
//!
//! ## Threading
//! The Shell is `!Send` (auto-lang session state uses `Rc`), so it can't live in
//! an iced `Task::perform` future (which must be `Send`). Instead a **dedicated
//! worker thread owns the Shell** (Plan 029's AshCommandTool pattern); the GUI
//! sends commands on a channel and awaits `RenderedOutput` responses. The async
//! futures only hold channel endpoints (`Send`), never the Shell.

mod renderer;

use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};

use ash_core::pipeline::AtomPipeline;
use ash_core::renderer::{render_pipeline_to_structured, RenderedOutput};
use iced::{Element, Task};

use renderer::{rendered_to_iced, GuiMsg};

/// A handle to the dedicated Shell worker thread: send it commands, await
/// rendered results. The Shell itself stays on the worker thread.
#[derive(Clone)]
struct ShellHandle {
    cmd_tx: SyncSender<String>,
    /// Shared result slot: the worker writes the latest result, the awaiting
    /// future reads it. (M2 simplicity: one outstanding command at a time.)
    result_rx: Arc<Mutex<mpsc::Receiver<RenderedOutput>>>,
}

impl ShellHandle {
    /// Spawn the dedicated Shell worker thread that owns the `!Send` Shell.
    fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::sync_channel::<String>(1);
        let (result_tx, result_rx) = mpsc::sync_channel::<RenderedOutput>(1);

        std::thread::Builder::new()
            .name("ash-gui-shell".into())
            .spawn(move || {
                let mut shell = auto_shell::Shell::new();
                shell.load_env_persistence();
                // Each incoming command is run + rendered, then sent back.
                for cmd in cmd_rx.iter() {
                    let rendered = run_command(&mut shell, &cmd);
                    // If the GUI dropped the result receiver (e.g. quit), stop.
                    if result_tx.send(rendered).is_err() {
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

    /// Run `cmd` and await its rendered output. Takes `self` by value (the
    /// handle is cheaply `Clone`) so the returned future is `'static` + `Send`
    /// — it owns the channel endpoints, never the `!Send` Shell.
    async fn run(self, cmd: String) -> RenderedOutput {
        if self.cmd_tx.send(cmd).is_err() {
            return RenderedOutput::Text("(shell worker thread died)".into());
        }
        let rx = self.result_rx.lock().unwrap();
        rx.recv()
            .unwrap_or_else(|_| RenderedOutput::Text("(shell worker thread died)".into()))
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Gui(GuiMsg),
    /// A command finished; here's its rendered output.
    CommandDone(RenderedOutput),
    /// Fires once on startup to seed the view with an initial `ls`.
    Seed,
}

/// The app state.
struct AshGui {
    shell: ShellHandle,
    input: String,
    output: RenderedOutput,
    /// Whether the initial `ls` seed command has been dispatched.
    seeded: bool,
}

impl AshGui {
    /// Construct the initial state (without running anything). iced 0.14's
    /// `application(boot, ...)` needs a `Fn() -> State`, so boot returns this.
    fn boot() -> Self {
        Self {
            shell: ShellHandle::spawn(),
            input: String::new(),
            output: RenderedOutput::Empty,
            seeded: false,
        }
    }

    fn title(&self) -> String {
        String::from("ash-gui")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Gui(GuiMsg::InputChanged(s)) => {
                self.input = s;
            }
            Message::Gui(GuiMsg::RunCommand) => {
                let cmd = std::mem::take(&mut self.input);
                if !cmd.trim().is_empty() {
                    let shell = self.shell.clone();
                    return Task::perform(shell.run(cmd), Message::CommandDone);
                }
            }
            Message::CommandDone(rendered) => {
                self.output = rendered;
            }
            // Fire the initial `ls` once, on the first update tick, so the
            // window opens showing a table widget (the M2 demonstration).
            Message::Seed if !self.seeded => {
                self.seeded = true;
                let shell = self.shell.clone();
                return Task::perform(shell.run("ls".to_string()), Message::CommandDone);
            }
            Message::Seed => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<Message> {
        rendered_to_iced(&self.output, &self.input).map(Message::Gui)
    }
}

/// Run a single command against the Shell and return its [`RenderedOutput`].
/// Runs on the dedicated worker thread (Shell is `!Send`).
fn run_command(shell: &mut auto_shell::Shell, cmd: &str) -> RenderedOutput {
    // Try the structured path: parse + run_atom → AtomPipeline → RenderedOutput.
    if let Some(rendered) = render_structured(shell, cmd) {
        return rendered;
    }
    // Fallback: plain execute → text. Covers non-atom commands (echo, external).
    match shell.execute(cmd) {
        Ok(Some(s)) => RenderedOutput::Text(s),
        Ok(None) => RenderedOutput::Empty,
        Err(e) => RenderedOutput::Text(format!("{e:?}")),
    }
}

/// Run a single command via the registry and render its AtomPipeline. Returns
/// `None` for commands that don't go through the atom path — the caller falls
/// back to `execute`.
fn render_structured(shell: &mut auto_shell::Shell, input: &str) -> Option<RenderedOutput> {
    // Split the input into command + args (quote-aware), then run via registry.
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

pub fn main() -> iced::Result {
    // iced 0.14: application(boot_fn, update, view).run().
    // - boot_fn: Fn() -> State (AshGui::boot)
    // - update:  Fn(&mut State, Message) -> Task
    // - view:    Fn(&State) -> Element
    // A subscription fires the seed `ls` once on startup.
    iced::application(AshGui::boot, AshGui::update, AshGui::view)
        .title(|s: &AshGui| s.title())
        .subscription(seed_subscription)
        .window_size([800.0, 600.0])
        .run()
}

/// A subscription that emits `Seed` so the window opens showing an `ls` table
/// widget (the M2 demonstration). The `update` handler guards on `seeded`, so
/// repeat emissions are harmless no-ops.
fn seed_subscription(_state: &AshGui) -> iced::Subscription<Message> {
    use iced::time::{every, Duration};
    every(Duration::from_secs(1)).map(|_| Message::Seed)
}
