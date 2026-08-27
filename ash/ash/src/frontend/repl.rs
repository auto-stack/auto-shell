use miette::Result;
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings, default_vi_normal_keybindings,
    EditCommand, Emacs, FileBackedHistory,
    KeyCode, KeyModifiers, Reedline, ReedlineEvent, ReedlineMenu, Signal, Vi,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::menu::{AshMenu, AshMenuConfig};
use auto_shell::completions::CompletionSignature;
use auto_shell::completions::definitions;
use crate::completions_reedline::{CompletionState, ShellCompleter};
use crate::term::hinter::AshHinter;
use ash_core::completions::CompletionProvider;
use crate::prompt::AshPrompt;
use auto_shell::shell::Shell;
use crate::term::highlight::AshHighlighter;

/// Read-Eval-Print Loop for AutoShell
pub struct Repl {
    shell: Shell,
    line_editor: Reedline,
    prompt: AshPrompt,
    /// Shared completion state — updated after each cd/pushd/etc.
    completion_state: Arc<Mutex<CompletionState>>,
    /// Plan 322: Input mode state (Shell/AutoScript/AI + lock + continuation).
    mode_state: auto_shell::repl_mode::ModeState,
    /// Plan 027: Lazy-initialized persistent AI chat session.
    chat: Option<auto_shell::ai::ChatSession>,
    /// Plan 072 M2 (S-3): receiving end of the chat approval gate — the
    /// agent queues non-readonly commands here; each turn drains them for a
    /// user verdict ([Enter] run / [e] replace / anything else cancel).
    ai_proposals: Option<std::sync::mpsc::Receiver<String>>,
}

impl Repl {
    /// Create a new REPL instance
    pub fn new() -> Result<Self> {
        let mut shell = Shell::new();
        // Plan 037 M2.1: inject the TUI render hook so structured data renders
        // as ratatui tables (decoupled from Shell via the RenderHook trait).
        shell.set_render_hook(Box::new(crate::renderer::TuiRenderHook));
        // Plan 037 M2.2: inject the interactive pager (for `show --pager`) and
        // register the terminal-only commands (less/more/color) that moved out
        // of auto-shell. Without these, --pager falls through to streamed
        // highlighting and those commands are unavailable.
        shell.set_pager_hook(Box::new(crate::commands::TuiPagerHook));
        shell.register_commands(crate::commands::terminal_commands());
        // Plan 309 Task 1.2 P4: apply persisted env from ~/.config/ash/env.at.
        shell.load_env_persistence();

        // Plan 302 Step 4.2: Load ~/.config/ash.toml
        let shell_config = auto_shell::config::AshShellConfig::load();

        // Apply aliases from config
        for (name, value) in &shell_config.aliases {
            shell.set_alias(name, value);
        }

        // Plan 302 Step 1.3: Load ~/.ashrc (user startup script — like .bashrc).
        // This is where user-defined functions (AutoLang `fn`) and aliases live.
        // On first start (file missing), seed it with example functions so users
        // discover the feature. Functions defined here register into the
        // persistent session and are callable from the prompt.
        if let Some(home) = dirs::home_dir() {
            let rc_path = home.join(".ashrc");
            if rc_path.exists() {
                let _ = shell.source_file(&rc_path); // silently ignore errors
            } else {
                // First run: create a default .ashrc with example functions.
                if let Ok(content) = std::str::from_utf8(auto_shell::DEFAULT_ASHRC.as_bytes()) {
                    let _ = std::fs::write(&rc_path, content);
                    let _ = shell.source_file(&rc_path);
                }
            }
        }

        // Plan 033: load installed plugins (sources plugin `functions.ash`,
        // records capability warnings). Completion & SmartCommand contributions
        // are picked up lazily by their respective loaders.
        let plugin_report = auto_shell::plugin::load_all_plugins(&mut shell)?;
        plugin_report.print_to_stderr();

        // Set up history file (configurable size)
        let history_path = Self::get_history_path()?;
        let history = Box::new(
            FileBackedHistory::with_file(shell_config.history_size, history_path)
                .map_err(|e| miette::miette!("Failed to create history: {}", e))?,
        );

        // Create completer for Tab completion (with registry signatures)
        let completion_sigs: Vec<CompletionSignature> =
            shell.registry().params().into_iter().map(Into::into).collect();

        // Create CompletionProvider and register external command definitions
        let mut provider = CompletionProvider::new();
        definitions::register_all(&mut provider);

        // Shared state for completion (cwd, etc.)
        let completion_state = Arc::new(Mutex::new(CompletionState::new(shell.pwd().to_path_buf())));

        let completer = Box::new(ShellCompleter::new(
            completion_sigs,
            provider,
            Arc::clone(&completion_state),
        ));

        // Use AshMenu (adaptive completion menu replacing ColumnarMenu)
        let completion_menu = Box::new(AshMenu::new(AshMenuConfig {
            name: "completion_menu".to_string(),
            ..Default::default()
        }));

        // History candidate menu — a separate list sourced from history
        // (NOT the command-based Tab completions). Bound to Ctrl+R. This is
        // the fzf-history style "popup of all matching history entries".
        let history_menu = Box::new(AshMenu::new(AshMenuConfig {
            name: "history_menu".to_string(),
            ..Default::default()
        }));

        // Plan 302 Step 3.2: Determine edit mode (Vi or Emacs)
        // Priority: $ASH_EDIT_MODE env var > ash.toml edit_mode > ~/.ashrc
        let use_vi = std::env::var("ASH_EDIT_MODE").map(|v| v == "vi").unwrap_or_else(|_| {
            if shell_config.is_vi_mode() {
                return true;
            }
            // Fallback: check ~/.ashrc for "set editing-mode vi"
            if let Some(home) = dirs::home_dir() {
                let rc = home.join(".ashrc");
                if let Ok(content) = std::fs::read_to_string(&rc) {
                    return content.lines().any(|line| {
                        let line = line.trim();
                        line == "set editing-mode vi"
                    });
                }
            }
            false
        });

        // The editor's edit mode (Emacs/Vi + ash keybindings) is built by
        // `build_edit_mode`. Multi-line editing lives in the editor modal
        // (`editor_overlay`, Plan 070), not in reedline.
        let edit_mode = build_edit_mode(use_vi);

        // Create modular prompt (AshPrompt)
        let prompt = AshPrompt::new(auto_shell::prompt::AshConfig::load());

        // Plan 302: Fish-style autosuggestion hinter (configurable).
        // Plan 032 M1.2: replaced reedline's CwdAwareHinter with AshHinter,
        // which keeps the same prefix-match behavior and adds a fuzzy
        // (prefix-subsequence) fallback so typing `gcm` can ghost-complete to
        // `git commit -m`. No AI here — real-time ghost-text must stay local.
        let hinter: Option<Box<AshHinter>> = if shell_config.autosuggestion {
            // Explicit dim style so the hint is clearly distinguishable from typed
            // text — reedline's default `LightGray` is too close to the terminal's
            // default foreground on Windows and reads as normal text.
            let hint_style = nu_ansi_term::Style::new()
                .fg(nu_ansi_term::Color::DarkGray)
                .italic();
            Some(Box::new(
                AshHinter::default()
                    .with_style(hint_style)
                    .with_min_chars(shell_config.autosuggestion_min_chars),
            ))
        } else {
            None
        };

        // Plan 302 Step 3.1: Syntax highlighting (configurable)
        let highlighter: Option<Box<AshHighlighter>> = if shell_config.syntax_highlighting {
            Some(Box::new(AshHighlighter::new()))
        } else {
            None
        };

        let mut line_editor = Reedline::create()
            .with_history(history)
            .with_completer(completer)
            .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
            .with_menu(ReedlineMenu::HistoryMenu(history_menu))
            .with_quick_completions(true)
            .with_partial_completions(true)
            .with_edit_mode(edit_mode);

        if let Some(h) = highlighter {
            line_editor = line_editor.with_highlighter(h);
        }
        if let Some(h) = hinter {
            line_editor = line_editor.with_hinter(h);
        }

        Ok(Self { shell, line_editor, prompt, completion_state, mode_state: Default::default(), chat: None, ai_proposals: None })
    }

    /// Get the path to the history file
    fn get_history_path() -> Result<PathBuf> {
        let mut history_path = dirs::home_dir()
            .ok_or_else(|| miette::miette!("Could not determine home directory"))?;

        history_path.push(".auto-shell-history");
        Ok(history_path)
    }

    /// Expand history references in the input line
    ///
    /// Supports: `!!` (last), `!n` (by number), `!-n` (relative),
    /// `!string` (prefix search), `!?string` (contains search).
    ///
    /// Returns Ok(true) if expansion occurred, Ok(false) if no expansion needed
    fn expand_line_history(&mut self, line: &mut String) -> Result<bool> {
        // Check if line contains history expansion character
        if !line.contains('!') {
            return Ok(false);
        }

        // Read history from file (reedline doesn't expose history via API)
        let history_path = Self::get_history_path()?;
        let history_strings = read_history_file(&history_path);

        if history_strings.is_empty() {
            return Ok(false);
        }

        struct FileHistory {
            strings: Vec<String>,
        }
        impl ash_core::parser::history::History for FileHistory {
            fn search(&self, _query: Option<&str>) -> Vec<String> {
                self.strings.clone()
            }
        }

        let file_history = FileHistory { strings: history_strings };
        let expanded = ash_core::parser::history::expand_history(line, &file_history)?;

        if &expanded != line {
            *line = expanded;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Update the shared completion state with the current working directory
    /// and Plan 032 context plumbing (last command/exit code/recent
    /// history/aliases), so context-aware ranking and AI completion layers
    /// have a coherent snapshot on the next `complete()` call.
    fn sync_completion_state(&self) {
        // Pull history outside the lock: it does file I/O, and we never want
        // to hold the completion-state mutex across a read of the (potentially
        // large) history file.
        let recent = Self::get_history_path()
            .ok()
            .map(|p| read_recent_history(&p, 50))
            .unwrap_or_default();

        if let Ok(mut state) = self.completion_state.lock() {
            state.current_dir = self.shell.pwd().to_path_buf();
            // Plan 032 M0.3: mirror the shell accessors (029 §2.3) into the
            // completion context. `last_command_line()` is None before the
            // first command; map it so the Option round-trips faithfully.
            state.last_command = self.shell.last_command_line().map(String::from);
            state.last_exit_code = Some(self.shell.last_exit_code());
            state.history = recent;
            state.aliases = self.shell.aliases().clone();
        }
    }

    /// Plan 322: Update the AshPrompt character symbol based on ModeState.
    /// Also handles continuation prompt for multiline, and mirrors the lock
    /// into the Shell — the Shell owns the Auto/Shell dispatch (`locked_mode`),
    /// so without this the F2 lock only changed the prompt symbol and input
    /// still went through auto-detect.
    fn update_prompt(&mut self) {
        let symbol = self.mode_state.prompt();
        self.prompt.set_character_symbol(&symbol);
        self.shell.set_locked_mode(self.mode_state.locked);
    }

    /// Plan 322: apply a mode-switch prefix — toggle the lock (F1=\x11 Shell,
    /// F2=\x12 AutoScript) or unlock (Esc/F3 prefixes) and refresh the
    /// prompt. The mode is announced by the right-prompt tag (`auto`/`Shell`)
    /// and, for the AutoScript lock, by the editor box title — no banner
    /// lines in the transcript (Plan 070).
    fn apply_mode_switch(&mut self, prefix: char) {
        use auto_shell::repl_mode::InputMode;
        match prefix {
            '\x11' => self.mode_state.toggle_lock(InputMode::Shell),
            '\x12' => self.mode_state.toggle_lock(InputMode::AutoScript),
            _ => self.mode_state.unlock(),
        }
        self.update_prompt();
    }

    /// Plan 070: the AutoScript lock opens the editor box (single-shot). Any
    /// outcome — run (boxed echo + execute), cancel (boxed echo), or plain
    /// exit — returns to the normal inline mode afterwards. Mode keys pressed
    /// inside (F1-F3/Alt+1-3) exit and then switch modes like at the prompt.
    fn open_script_editor(&mut self) {
        let mut pending_prefix = None;
        match crate::editor_overlay::run_editor("", "▌# AutoScript") {
            crate::editor_overlay::EditorOutcome::Run(text) => {
                self.commit_script_echo("▌# AutoScript", &text, false);
                let _ = self.execute_with_header(&text);
                self.after_editor_execute();
            }
            crate::editor_overlay::EditorOutcome::Cancelled(text) => {
                self.commit_script_echo("▌# AutoScript", &text, true);
            }
            crate::editor_overlay::EditorOutcome::Exit => {}
            // F2 inside the AutoScript editor means "leave" (we're already
            // there — unlock below, no reopen); other prefixes switch after.
            crate::editor_overlay::EditorOutcome::ExitThen('\x12') => {}
            crate::editor_overlay::EditorOutcome::ExitThen(p) => pending_prefix = Some(p),
        }
        self.mode_state.unlock();
        self.update_prompt();
        if let Some(p) = pending_prefix {
            self.dispatch_mode_prefix(p);
        }
    }

    /// Plan 070: Ctrl+O — one-shot editor box from any inline mode, seeded
    /// with the current line. Runs through the normal execution path (routing
    /// follows the current lock / auto-detect), then returns to the prompt.
    /// Mode keys pressed inside exit and then switch (F2 here re-enters the
    /// script editor via the lock, since Ctrl+O started outside it).
    fn run_editor_once(&mut self, prefill: &str) {
        match crate::editor_overlay::run_editor(prefill, "> 命令") {
            crate::editor_overlay::EditorOutcome::Run(text) => {
                self.commit_script_echo("> 命令", &text, false);
                let _ = self.execute_with_header(&text);
                self.after_editor_execute();
            }
            crate::editor_overlay::EditorOutcome::Cancelled(text) => {
                self.commit_script_echo("> 命令", &text, true);
            }
            crate::editor_overlay::EditorOutcome::Exit => {}
            crate::editor_overlay::EditorOutcome::ExitThen(p) => self.dispatch_mode_prefix(p),
        }
    }

    /// Apply a mode-switch prefix exactly like the run() keybinding branches
    /// do: F1/F2 toggle locks, F3 opens the AI chat. Shared by the editor's
    /// ExitThen outcomes (finish-plan finding ①).
    fn dispatch_mode_prefix(&mut self, prefix: char) {
        match prefix {
            '\x11' | '\x12' => self.apply_mode_switch(prefix),
            '\x13' => {
                let _ = self.run_chat_loop();
            }
            _ => {}
        }
    }

    /// Post-execution bookkeeping shared by both editor entry points — the
    /// same refreshes the inline run() loop does after a command.
    fn after_editor_execute(&mut self) {
        auto_shell::prompt::context::refresh_git_info_async(self.shell.pwd());
        self.sync_completion_state();
    }

    /// Plan 070: commit editor content into the linear transcript — a dim,
    /// rounded-border, line-numbered box (same visual language as the live
    /// editor box), so the echo reads as "what was in the box" and stays
    /// copy-friendly.
    fn commit_script_echo(&self, title: &str, text: &str, cancelled: bool) {
        println!(
            "{}",
            crate::editor_overlay::render_script_block(title, text, cancelled)
        );
    }


    /// Plan 027: the standalone AI chat loop. Owns the reedline editor until
    /// the user exits via Esc, F1/F2/F3 (mode switch), or
    /// `/exit`. Persists the conversation on exit.
    fn run_chat_loop(&mut self) -> Result<()> {
        // Lock AI mode so the prompt shows `▌?`.
        self.mode_state.locked = Some(auto_shell::repl_mode::InputMode::AI);
        self.update_prompt();

        // Lazily load the persistent session and print a banner.
        // `load()` builds the AiClient here, in a SYNCHRONOUS context — it
        // runs the blocking daemon probe, which must NOT happen inside the
        // async turn (see `frontend::ai::ChatSession` docs).
        //
        // Plan 072 M2 (S-3/S-5): the CLI session now carries the approval
        // gate (non-readonly commands become proposals, drained for a user
        // verdict after each turn) and runs under the interactive session's
        // security policy (`--read-only`/`--sandbox` constrain AI commands).
        if self.chat.is_none() {
            let (ptx, prx) = std::sync::mpsc::channel::<String>();
            match auto_shell::ai::ChatSession::load_secured(
                self.shell.policy.clone(),
                Some(ptx),
            ) {
                Ok(session) => {
                    self.chat = Some(session);
                    self.ai_proposals = Some(prx);
                }
                Err(e) => {
                    eprintln!(
                        "  AI error: {}\n  (set ZHIPU_API_KEY / ANTHROPIC_API_KEY / \
                         OPENAI_API_KEY or start the aaid daemon)",
                        e
                    );
                    // Can't chat without a client — leave AI mode.
                    self.mode_state.locked = None;
                    self.update_prompt();
                    return Ok(());
                }
            }
        }
        let turns = self.chat.as_ref().expect("chat session initialized above").turn_count();
        if turns > 0 {
            println!("  * 已恢复 {} 轮对话 *", turns / 2);
        } else {
            println!("  * 开始新对话 *  (/clear 清空  /exit 退出  F1/F2/F3/Esc 离开)");
        }

        loop {
            let sig = self.line_editor.read_line(&self.prompt);
            let line = match sig {
                Ok(Signal::Success(l)) => l.trim().to_string(),
                Ok(Signal::CtrlD) => break,          // Ctrl-D exits chat
                Ok(Signal::CtrlC) => continue,       // Ctrl-C: new prompt, stay in chat
                Err(_) => continue,
            };

            // Exit prefixes: Esc (\x14), F1/F2/F3 (\x11/\x12/\x13). Save,
            // then hand the prefix to the shared mode-switch helper
            // (toggle/unlock + prompt refresh + banner).
            if let Some(prefix) = line.chars().next() {
                if matches!(prefix, '\x11' | '\x12' | '\x13' | '\x14') {
                    if let Some(session) = self.chat.as_mut() {
                        let _ = session.save();
                    }
                    self.apply_mode_switch(prefix);
                    // If it was F1/F2/F3 with trailing text, we dropped it
                    // (chat doesn't interpret those). Acceptable for v1.
                    break;
                }
            }

            // Slash commands.
            if let Some(cmd) = auto_shell::ai::parse_slash_command(&line) {
                match cmd {
                    auto_shell::ai::SlashCommand::Exit => {
                        if let Some(session) = self.chat.as_mut() {
                            let _ = session.save();
                        }
                        self.mode_state.locked = None;
                        self.update_prompt();
                        break;
                    }
                    auto_shell::ai::SlashCommand::Clear => {
                        if let Some(session) = self.chat.as_mut() {
                            session.clear();
                            let _ = session.save();
                        }
                        println!("  * 对话已清空 *");
                        continue;
                    }
                }
            }

            // Unknown slash command → no-op notice.
            if line.starts_with('/') {
                println!("  未知命令: {} (可用: /clear /exit)", line);
                continue;
            }

            // Empty line → no-op.
            if line.is_empty() {
                continue;
            }

            // A real chat turn.
            let _ = self.handle_chat_turn(&line);
        }

        Ok(())
    }

    /// Plan 027/029: send one chat turn through the agent's ReAct loop. The
    /// agent may call ash command tools (pwd/ls/cat/...) mid-turn; tool events
    /// are rendered inline so the user sees what the agent is doing.
    fn handle_chat_turn(&mut self, user: &str) -> Result<()> {
        let on_event: Arc<dyn Fn(auto_ai_agent::agent::StreamEvent) + Send + Sync> = Arc::new(
            |ev| match ev {
                auto_ai_agent::agent::StreamEvent::Delta { text } => {
                    use std::io::Write;
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
                auto_ai_agent::agent::StreamEvent::ToolStart { tool, args } => {
                    println!("\n  \x1b[2m\u{2699} {tool} {}\x1b[0m", brief_args(&args));
                }
                auto_ai_agent::agent::StreamEvent::Tool { tool, result, .. } => {
                    println!("\n  \x1b[2m\u{2190} {tool}: {}\x1b[0m", brief_result(&result));
                }
                auto_ai_agent::agent::StreamEvent::Warning { text } => {
                    println!("\n  \x1b[2m\u{26a0}\u{fe0f} {text}\x1b[0m");
                }
                auto_ai_agent::agent::StreamEvent::Done { .. } => {} // keep chat output clean
                auto_ai_agent::agent::StreamEvent::Thinking { .. } => {}
                auto_ai_agent::agent::StreamEvent::Error { message } => {
                    println!("\n  [error] {message}");
                }
                auto_ai_agent::agent::StreamEvent::Cancelled { .. } => {
                    println!("\n  [cancelled]");
                }
            },
        );
        let session = self.chat.as_mut().expect("chat session initialized in run_chat_loop");
        // Plan 029 §7.2: refresh the agent's context (cwd/last-command/aliases)
        // before each turn — the user may have `cd`'d since the last turn.
        session.update_context(&self.shell);
        let result = auto_shell::ai::block_on_async(
            session.send_turn_streaming(user, on_event),
        );
        match result {
            Ok(_full_text) => {
                println!(); // newline after the streamed reply
                let _ = session.save();
            }
            Err(e) => {
                eprintln!(
                    "  AI error: {}\n  (set ZHIPU_API_KEY / ANTHROPIC_API_KEY / \
                     OPENAI_API_KEY or start the aaid daemon)",
                    e
                );
            }
        }
        // Plan 072 M2 (S-3): approval gate — anything the agent proposed
        // (non-readonly commands) waits for the user's verdict here.
        self.drain_ai_proposals();
        Ok(())
    }

    /// Plan 072 M2 (S-3): present each AI-proposed command for approval.
    /// `[Enter]` executes it (through the normal command path, under the
    /// session's security policy), `e` replaces it with an edited line,
    /// anything else (incl. Ctrl-C/Ctrl-D) cancels.
    fn drain_ai_proposals(&mut self) {
        // Drain first, then act — the verdict handling needs `&mut self`
        // (execute_with_header) which conflicts with the receiver borrow.
        let mut queued: Vec<String> = Vec::new();
        if let Some(rx) = self.ai_proposals.as_mut() {
            while let Ok(cmd) = rx.try_recv() {
                queued.push(cmd);
            }
        }
        for cmd in queued {
            println!("\n  \x1b[1m📋 建议命令\x1b[0m {cmd}");
            print!("  \x1b[2m[Enter] 执行 · [e] 输入替代命令 · 其他 取消\x1b[0m › ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let verdict = match self.line_editor.read_line(&self.prompt) {
                Ok(Signal::Success(l)) => l.trim().to_string(),
                // Ctrl-C / Ctrl-D / EOF all cancel — never execute on abort.
                _ => {
                    println!("  已取消");
                    continue;
                }
            };
            match verdict.as_str() {
                "" => {
                    let _ = self.execute_with_header(&cmd);
                }
                "e" => {
                    print!("  \x1b[2m替代命令(空=取消):\x1b[0m ");
                    let _ = std::io::stdout().flush();
                    if let Ok(Signal::Success(edited)) = self.line_editor.read_line(&self.prompt) {
                        let edited = edited.trim();
                        if edited.is_empty() {
                            println!("  已取消");
                        } else {
                            let _ = self.execute_with_header(edited);
                        }
                    } else {
                        println!("  已取消");
                    }
                }
                _ => println!("  已取消"),
            }
        }
    }

    /// Plan 037 M3: execute a command and print its output, prefixed by a
    /// red right-aligned `✗` marker ONLY when the command failed. Success is
    /// silent — in the reedline CLI the typed input line sits directly above
    /// the result, and a "Nms ✓" line would carry no information (slow
    /// commands surface via the prompt's `$cmd_duration` module instead).
    ///
    /// Returns `(output_snippet, exit_code)`:
    /// - `output_snippet` — first 200 chars of output (for the suggest-next
    ///   context; empty on error or no output).
    /// - `exit_code` — the command's exit code (0 on success, non-zero on
    ///   error; for an `Err` from `execute` we report 1 since no code was set).
    ///
    /// reedline 0.44.0 cannot pin a header while output scrolls, so this is
    /// the plan's documented fallback. Non-interactive paths (`-c`/`-s`/script)
    /// do NOT go through here, keeping their machine-readable output free of
    /// decoration.
    fn execute_with_header(&mut self, command: &str) -> (String, i32) {
        let start = std::time::Instant::now();
        let result = self.shell.execute(command);
        let elapsed = start.elapsed();

        // Exit code: Shell sets last_exit_code even on Err for security denies
        // etc.; for a generic Err with no code set, fall back to 1.
        let exit_code = result.as_ref().map(|_| self.shell.last_exit_code()).unwrap_or_else(|_| {
            let code = self.shell.last_exit_code();
            if code != 0 { code } else { 1 }
        });

        // Terminal width for right-aligning the marker (0 = unknown).
        let term_width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(0);

        // Print the failure marker (nothing on success), then the output.
        if let Some(status) = crate::block_header::render_failure_status(
            exit_code,
            elapsed,
            term_width,
        ) {
            println!("{}", status);
        }

        let snippet = match result {
            Ok(Some(s)) => {
                let snippet: String = s.chars().take(200).collect();
                auto_shell::shell::print_command_output(&s);
                snippet
            }
            Ok(None) => String::new(),
            Err(e) => {
                eprintln!("Error: {}", e);
                String::new()
            }
        };
        (snippet, exit_code)
    }


    /// Open the current input line in $EDITOR (or vim/notepad) and return the result.
    /// Plan 304: Multi-line edit via Ctrl+E.
    fn edit_in_editor(&self, initial_content: &str) -> Result<String> {
        let tmp_dir = std::env::temp_dir();
        let tmp_file = tmp_dir.join("ash_edit_buffer.txt");
        std::fs::write(&tmp_file, initial_content)
            .map_err(|e| miette::miette!("editor: failed to write temp file: {}", e))?;

        // Determine editor: $VISUAL > $EDITOR > platform default
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| {
                if cfg!(windows) { "notepad".to_string() } else { "vim".to_string() }
            });

        // Parse editor command (may have args like "code --wait")
        let parts: Vec<&str> = editor.split_whitespace().collect();
        let (cmd, extra_args) = match parts.split_first() {
            Some((c, args)) => (*c, args.to_vec()),
            None => ("vim", vec![]),
        };

        let mut command = std::process::Command::new(cmd);
        command.args(&extra_args).arg(&tmp_file);

        // Inherit terminal for the editor
        let status = command.status()
            .map_err(|e| miette::miette!("editor: failed to launch '{}': {}", cmd, e))?;

        if !status.success() {
            return Err(miette::miette!("editor: exited with status {}", status));
        }

        let content = std::fs::read_to_string(&tmp_file)
            .map_err(|e| miette::miette!("editor: failed to read temp file: {}", e))?;

        // Clean up temp file
        let _ = std::fs::remove_file(&tmp_file);

        Ok(content.trim().to_string())
    }

    /// Plan 008 (MS2-A): apply a security policy to the underlying shell.
    pub fn set_policy(&mut self, policy: ash_core::security::SecurityPolicy) {
        self.shell.set_policy(policy);
    }

    /// Run the REPL loop
    pub fn run(&mut self) -> Result<()> {
        // One-time Ctrl+C handler init (protects ASH during commands)
        auto_shell::signal::init();

        // Plan 322: start with the mode-aware symbol (`>`) instead of the
        // pre-322 config default (`❯`), so the prompt looks the same before
        // and after the first F1/F2 switch.
        self.update_prompt();

        // Plan 070: one-time key legend; modes themselves live on the right
        // prompt (`auto` / `Shell` / `AI` tag).
        println!("{}", startup_legend());

        // Initial git cache: sync refresh + start filesystem watcher for cwd
        auto_shell::prompt::context::on_directory_changed(self.shell.pwd());

        loop {
            // Plan 070: the AutoScript lock opens the editor box — any path
            // that leaves the lock set (F2 press, F2-exits-chat) enters it
            // here instead of showing an inline `▌#` prompt. Single-shot:
            // every outcome returns to the normal inline mode.
            if self.mode_state.locked == Some(auto_shell::repl_mode::InputMode::AutoScript) {
                self.open_script_editor();
                continue;
            }

            // Plan 029 §7.3: if a suggest-next fetch completed, show it before
            // the next prompt. (Best-effort: if it hasn't finished yet, nothing
            // shows — the fetch never blocks.)
            if let Some(suggestions) = auto_shell::ai::suggest::take_pending() {
                if !suggestions.is_empty() {
                    println!("\n  \x1b[2m\u{1f4a1} 接下来可能想:\x1b[0m");
                    for s in &suggestions {
                        println!("  \x1b[2m   {s}\x1b[0m");
                    }
                }
            }

            // Read input
            let sig = self.line_editor.read_line(&self.prompt);

            match sig {
                Ok(Signal::Success(line)) => {
                    let mut line = line.trim().to_string();

                    // Plan 322: Mode-switching prefix chars (from F1/F2/F3/Esc keybindings).
                    // \x11=F1 (toggle Shell lock), \x12=F2 (toggle Auto lock),
                    // \x14=Esc (unlock). Each switch prints a one-line mode banner.
                    if line.starts_with('\x11') {
                        self.apply_mode_switch('\x11');
                        continue;
                    }
                    if line.starts_with('\x12') {
                        self.apply_mode_switch('\x12');
                        continue;
                    }
                    if line.starts_with('\x14') {
                        self.apply_mode_switch('\x14');
                        continue;
                    }
                    // Plan 070: Ctrl+O — the embedded script editor. The \x0f
                    // marker lands AFTER any typed text (the keybinding appends
                    // it), so strip from the suffix and prefill the editor.
                    if let Some(prefill) = line.strip_suffix('\x0f') {
                        self.run_editor_once(prefill.trim_start_matches('\x0f'));
                        continue;
                    }
                    // F3 = AI mode: the persistent AI chat.
                    // Plan 069 (unified agent): one AI mode; the one-shot NL
                    // translate flow (ask_ai + approval card) is retired.
                    // F4 was retired alongside (2026-08-26 user decision:
                    // F3 is the only AI entry).
                    if line.starts_with('\x13') {
                        self.run_chat_loop()?;
                        continue;
                    }

                    // Plan 304: Ctrl+E — open line in editor
                    // If line starts with "\x05" (Ctrl+E character), edit in $EDITOR
                    if line.starts_with('\x05') {
                        line = line[1..].trim().to_string();
                        line = match self.edit_in_editor(&line) {
                            Ok(edited) => edited,
                            Err(e) => {
                                eprintln!("editor: {}", e);
                                continue;
                            }
                        };
                        if line.is_empty() {
                            continue;
                        }
                        println!("{}", line); // show edited command
                    }

                    // Plan 322: Multi-line input handling (syntax-based).
                    // Detects unclosed { } ( ) [ ] " ' or trailing backslash,
                    // then reads continuation lines with a `·` prompt.
                    loop {
                        if auto_shell::repl_mode::needs_continuation(&line) {
                            // For trailing backslash: strip it and join with space.
                            let trimmed = line.trim_end();
                            if trimmed.ends_with('\\') && !trimmed.ends_with("\\\\") {
                                line.truncate(line.trim_end().len() - 1);
                                line.push(' ');
                            } else {
                                line.push('\n'); // For unclosed delimiters: join with newline.
                            }
                            // Plan 322 #1: switch prompt to · during continuation.
                            self.mode_state.in_continuation = true;
                            self.update_prompt();
                            let cont = self.line_editor.read_line(&self.prompt);
                            self.mode_state.in_continuation = false;
                            self.update_prompt();
                            match cont {
                                Ok(Signal::Success(next)) => {
                                    line.push_str(&next);
                                }
                                Ok(Signal::CtrlD) => break, // Ctrl-D accepts what we have
                                _ => break,
                            }
                        } else {
                            break;
                        }
                    }

                    // Skip empty lines
                    if line.is_empty() {
                        continue;
                    }

                    // Plan 304: Expand abbreviations (abbr) in-line
                    let (expanded, was_expanded) = self.shell.expand_abbreviations(&line);
                    if was_expanded {
                        println!("{}", expanded); // show the expanded form
                        line = expanded;
                    }

                    // Expand history references (!!, !n, etc.)
                    match self.expand_line_history(&mut line) {
                        Ok(true) => {
                            // History was expanded, show the expanded command
                            println!("{}", line);
                        }
                        Ok(false) => {
                            // No history expansion needed
                        }
                        Err(e) => {
                            eprintln!("History expansion error: {}", e);
                            continue;
                        }
                    }

                    // Handle exit command
                    if line == "exit" || line == "quit" || line == "q" {
                        println!("Goodbye!");
                        break;
                    }

                    // Handle interactive commands (vim, less, ssh, etc.)
                    // These need full terminal control — just execute directly
                    // with inherited stdio, bypassing the shell's pipeline system.
                    if ash_core::cmd::interactive::is_interactive_command(&line) {
                        let result = ash_core::cmd::external::execute_external(
                            &line,
                            &self.shell.pwd(),
                            false, // inherit stdio, not capture
                        );
                        if let Err(e) = result {
                            eprintln!("Error: {}", e);
                        }
                        // Refresh git info after interactive command
                        auto_shell::prompt::context::refresh_git_info_async(
                            self.shell.pwd(),
                        );
                        self.sync_completion_state();
                        continue;
                    }

                    // Plan 322 #3: Update last_auto before execution (for AI mode restore).
                    if self.mode_state.locked.is_none() {
                        self.mode_state.last_auto = if self.shell.is_auto_expression_pub(&line) {
                            auto_shell::repl_mode::InputMode::AutoScript
                        } else {
                            auto_shell::repl_mode::InputMode::Shell
                        };
                    }

                    // Evaluate the line. Plan 037 M3: execute_with_header
                    // prints a red ✗ marker before the output on failure
                    // (silent on success) and returns the snippet for the
                    // suggest-next context (§7.3).
                    let (output_snippet, _exit_code) = self.execute_with_header(&line);
                    // After command execution, async-refresh git cache (most
                    // changes are caught by filesystem watcher, but this covers
                    // edge cases like external git commands).
                    auto_shell::prompt::context::refresh_git_info_async(
                        self.shell.pwd(),
                    );

                    // Sync completion state (cwd may have changed after cd/pushd)
                    self.sync_completion_state();

                    // Plan 029 §7.3: async-suggest next command (opt-in). Fire
                    // a background fetch; the result shows before the next prompt
                    // if it arrived in time. Never blocks the shell.
                    if auto_shell::ai::suggest::is_enabled() {
                        auto_shell::ai::suggest::suggest_next_async(
                            self.shell.pwd().to_string_lossy().to_string(),
                            line.clone(),
                            output_snippet,
                        );
                    }
                }
                Ok(Signal::CtrlD) => {
                    println!();
                    println!("Goodbye!");
                    break;
                }
                Ok(Signal::CtrlC) => {
                    // User pressed Ctrl+C, just show new prompt
                    continue;
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                    continue;
                }
            }
        }

        Ok(())
    }
}

/// Check if a line has unclosed quotes (single or double).
///
/// Counts quote characters outside of the other quote type.
/// An odd count means the quote is unclosed.
fn has_unclosed_quote(line: &str) -> bool {
    let mut single_count = 0;
    let mut double_count = 0;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // Skip escaped character
                chars.next();
            }
            '\'' if double_count % 2 == 0 => {
                single_count += 1;
            }
            '"' if single_count % 2 == 0 => {
                double_count += 1;
            }
            _ => {}
        }
    }

    single_count % 2 != 0 || double_count % 2 != 0
}

/// Read history entries from the reedline FileBackedHistory file.
///
/// The file format is simple: one command per line. Blank lines are skipped.
/// We deduplicate by keeping only the most recent occurrence of each command
/// (matches what users expect from `!!` — the last time they ran it).
fn read_history_file(path: &std::path::Path) -> Vec<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect()
}

/// Read the last `n` history entries from the reedline history file
/// (Plan 032 M0.2).
///
/// Unlike [`read_history_file`], this only reads what's needed for completion
/// context (bounded to `n`), keeping the snapshot cheap even when the history
/// file grows large. Entries are returned in chronological order (oldest of
/// the window first), matching how callers feed them to ranking/AI prompts.
///
/// Returns an empty vec if the file is missing/unreadable or `n == 0`.
pub fn read_recent_history(path: &std::path::Path, n: usize) -> Vec<String> {
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

// Plan 030 M0: brief_args / brief_result / brief_truncate moved to the
// terminal-dep-free `super::brief` module so `ash ask` (frontend/ask.rs) can use
// them without the frontend-tui feature. This file re-exports them for its own
// StreamEvent rendering below.
use auto_shell::ai::brief::{brief_args, brief_result};

/// Common ash keybindings added to every edit-mode keybinding set (Tab
/// completion, Ctrl+F hint accept, Ctrl+R history menu, F1-F3 mode switches,
/// Esc unlock, Alt+1/2/3/4 aliases). Module-scope so both the initial editor
/// build and runtime multiline rebuilds share one source of truth.
fn add_common_keybindings(keybindings: &mut reedline::Keybindings) {
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::UntilFound(vec![
                ReedlineEvent::Menu("completion_menu".to_string()),
                ReedlineEvent::MenuNext,
                ReedlineEvent::Edit(vec![EditCommand::Complete]),
            ]),
            ReedlineEvent::Repaint,
        ]),
    );
    // Plan 302: Ctrl+F accepts the full autosuggestion hint (Fish-style).
    // NOTE: must be `HistoryHintComplete`, NOT `EditCommand::Complete` —
    // the latter triggers the completion *menu* and never accepts a hint.
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('f'),
        ReedlineEvent::HistoryHintComplete,
    );
    // Plan 302 Step 3.4: Ctrl+→ accepts next word of autosuggestion
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Right,
        ReedlineEvent::HistoryHintWordComplete,
    );
    // Ctrl+R — pop up the history candidate menu (fzf-history style).
    // Separate from Tab (command-based) completions: this lists matching
    // entries from shell history. Supersedes the old inline SearchHistory.
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('r'),
        ReedlineEvent::Menu("history_menu".to_string()),
    );
    // Ctrl+S — forward history search (legacy inline search retained as a
    // non-popup fallback alongside the Ctrl+R menu).
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('s'),
        ReedlineEvent::SearchHistory,
    );
            // Plan 304: Ctrl+E — edit in $EDITOR (sends \x05 prefix)
            keybindings.add_binding(
                KeyModifiers::CONTROL,
                KeyCode::Char('e'),
                ReedlineEvent::Edit(vec![EditCommand::InsertString("\x05".to_string())]),
            );
            // Plan 070: Ctrl+O — open the embedded script editor with the
            // current line prefilled (sends \x0f AFTER any typed text; the
            // run() branch uses strip_suffix). Overrides reedline's own
            // OpenEditor default — the embedded modal replaces it.
            keybindings.add_binding(
                KeyModifiers::CONTROL,
                KeyCode::Char('o'),
                ReedlineEvent::Multiple(vec![
                    ReedlineEvent::Edit(vec![EditCommand::InsertString("\x0f".to_string())]),
                    ReedlineEvent::Submit,
                ]),
            );
            // Plan 070: Ctrl+Enter submits — same key as the editor modal's
            // run action, one muscle memory across inline and modal.
            keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Enter, ReedlineEvent::Submit);
    // Plan 322: F1/F2/F3 mode switching. Insert a prefix char + submit
    // immediately (no Enter needed). The run loop detects the prefix.
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::F(1),
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::Edit(vec![EditCommand::InsertString("\x11".to_string())]),
            ReedlineEvent::Submit,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::F(2),
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::Edit(vec![EditCommand::InsertString("\x12".to_string())]),
            ReedlineEvent::Submit,
        ]),
    );
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::F(3),
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::Edit(vec![EditCommand::InsertString("\x13".to_string())]),
            ReedlineEvent::Submit,
        ]),
    );
    // (F4 retired 2026-08-26: F3 is the only AI entry — user decision.)
    // Esc — unlock mode (insert \x14 + submit).
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Esc,
        ReedlineEvent::Multiple(vec![
            ReedlineEvent::Edit(vec![EditCommand::InsertString("\x14".to_string())]),
            ReedlineEvent::Submit,
        ]),
    );
    // Plan 322 #4: Alt+1/2/3 as laptop-friendly F1/F2/F3 aliases.
    for (key, prefix) in [('1', "\x11"), ('2', "\x12"), ('3', "\x13")] {
        keybindings.add_binding(
            KeyModifiers::ALT,
            KeyCode::Char(key),
            ReedlineEvent::Multiple(vec![
                ReedlineEvent::Edit(vec![EditCommand::InsertString(prefix.to_string())]),
                ReedlineEvent::Submit,
            ]),
        );
    }
}

/// Build the line editor's edit mode (Emacs or Vi + ash keybindings).
///
/// Multi-line editing is NOT done here — Plan 070 moved it to the editor
/// modal (`editor_overlay`), which runs between `read_line` calls with its
/// own ratatui viewport. The inline editor keeps Enter=submit; Ctrl+Enter
/// also submits (see `add_common_keybindings`) so the muscle memory carries
/// across the inline/modal boundary.
fn build_edit_mode(use_vi: bool) -> Box<dyn reedline::EditMode> {
    if use_vi {
        let mut insert_kb = default_vi_insert_keybindings();
        let normal_kb = default_vi_normal_keybindings();
        add_common_keybindings(&mut insert_kb);
        Box::new(Vi::new(insert_kb, normal_kb))
    } else {
        let mut keybindings = default_emacs_keybindings();
        add_common_keybindings(&mut keybindings);
        Box::new(Emacs::new(keybindings))
    }
}

#[cfg(test)]
mod build_edit_mode_tests {
    use super::build_edit_mode;
    use crossterm::event::{Event, KeyEvent, KeyEventKind, KeyEventState};
    use reedline::{EditCommand, EditMode, KeyCode, KeyModifiers, ReedlineEvent, ReedlineRawEvent};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> ReedlineRawEvent {
        ReedlineRawEvent::try_from(Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }))
        .unwrap()
    }

    #[test]
    fn plain_enter_submits() {
        let mut em = build_edit_mode(false);
        assert!(matches!(
            em.parse_event(key(KeyCode::Enter, KeyModifiers::NONE)),
            ReedlineEvent::Enter // default emacs binding (validator path → submit)
        ));
    }

    #[test]
    fn ctrl_enter_submits() {
        // Same key as the editor modal's run key — one muscle memory.
        let mut em = build_edit_mode(false);
        assert!(matches!(
            em.parse_event(key(KeyCode::Enter, KeyModifiers::CONTROL)),
            ReedlineEvent::Submit
        ));
    }

    #[test]
    fn ctrl_o_binds_to_editor_prefix() {
        // Ctrl+O (opens the editor modal) must be captured before reedline's
        // own OpenEditor default — the marker \x0f is appended after any
        // typed text, so the branch uses strip_suffix.
        let mut em = build_edit_mode(false);
        let ev = em.parse_event(key(KeyCode::Char('o'), KeyModifiers::CONTROL));
        match ev {
            ReedlineEvent::Multiple(events) => {
                assert!(events.iter().any(|e| matches!(
                    e,
                    ReedlineEvent::Edit(cmds) if matches!(
                        cmds.as_slice(),
                        [EditCommand::InsertString(s)] if s == "\x0f"
                    )
                )));
            }
            other => panic!("expected Multiple, got {other:?}"),
        }
    }
}

/// Plan 070: one-time key legend, printed at REPL startup. Mode changes are
/// announced by the right-prompt tag (`auto`/`Shell`/`AI`) instead of the
/// old per-switch banners — the transcript stays clean.
fn startup_legend() -> &'static str {
    "  \x1b[2mF1 命令锁 · F2 脚本编辑器 · F3 AI 对话 · Ctrl+O 编辑当前行 · Tab 补全 · Ctrl+R 历史\x1b[0m"
}

#[cfg(test)]
mod startup_legend_tests {
    use super::startup_legend;

    #[test]
    fn legend_covers_every_mode_key() {
        let l = startup_legend();
        for key in ["F1", "F2", "F3", "Ctrl+O", "Tab", "Ctrl+R"] {
            assert!(l.contains(key), "legend should mention {key}: {l}");
        }
    }
}

#[cfg(test)]
mod read_recent_history_tests {
    use super::read_recent_history;
    use std::io::Write;

    fn write_history(entries: &[&str]) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "ash-032-history-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut f = std::fs::File::create(&dir).unwrap();
        for e in entries {
            writeln!(f, "{e}").unwrap();
        }
        dir
    }

    #[test]
    fn returns_all_when_fewer_than_n() {
        let path = write_history(&["ls", "cd /tmp", "git status"]);
        let got = read_recent_history(&path, 50);
        assert_eq!(got, vec!["ls", "cd /tmp", "git status"]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn returns_last_n_in_chronological_order() {
        let cmds: Vec<String> = (0..100).map(|i| format!("cmd{i}")).collect();
        let refs: Vec<&str> = cmds.iter().map(String::as_str).collect();
        let path = write_history(&refs);
        let got = read_recent_history(&path, 50);
        // Exactly 50, and they are the LAST 50 (cmd50..cmd99), oldest-first.
        assert_eq!(got.len(), 50);
        assert_eq!(got.first().map(String::as_str), Some("cmd50"));
        assert_eq!(got.last().map(String::as_str), Some("cmd99"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn n_zero_returns_empty() {
        let path = write_history(&["ls"]);
        assert!(read_recent_history(&path, 0).is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_returns_empty() {
        let bogus = std::path::PathBuf::from("/this/path/does/not/exist/032");
        assert!(read_recent_history(&bogus, 50).is_empty());
    }

    #[test]
    fn skips_blank_lines() {
        let path = write_history(&["ls", "", "   ", "cd"]);
        let got = read_recent_history(&path, 50);
        assert_eq!(got, vec!["ls", "cd"]);
        let _ = std::fs::remove_file(&path);
    }
}
