//! Block TUI — ratatui inline-viewport REPL (Plan 038 M0 + M1).
//!
//! The **experimental** ratatui-owned terminal path, counterpart to the
//! reedline-driven [`crate::Repl`]. Plan 038 explores replacing reedline's
//! hold on the terminal with a ratatui `Viewport::Inline`, so that finished
//! commands can be pushed into the host scrollback as blocks while a fixed
//! bottom editor stays visible (something reedline 0.44.0 cannot do).
//!
//! ## Milestone status
//! - **M0**: skeleton — `Viewport::Inline(3)` + event loop + `insert_before`.
//! - **M1** (current): the bottom editor is live. Keys route through a reedline
//!   `EditMode` (Emacs/Vi keybinding parser) into the [`Editor`], which mutates
//!   a reedline `LineBuffer`. Enter submits the line → it's pushed into the
//!   scrollback as a one-line "block". C-a/C-e/M-b/M-f/Backspace/basic vi work.
//!
//! Everything beyond this (history, completion, hints, real block rendering,
//! subprocess handoff, orchestration) lands in M2-M4. See
//! `docs/plans/038-block-tui-migration.md`.
//!
//! ## Terminal ownership
//! Unlike the reedline REPL (which enters raw mode per `read_line` and returns
//! to cooked mode between commands), this module owns the terminal for its
//! entire run: raw mode is enabled on [`BlockTui::run`] entry and disabled on
//! exit (incl. via the panic hook). This is the core architectural difference
//! and the source of most of the orchestration work in M4.

use std::io::{self, stdout};

// ratatui-crossterm re-exports the selected crossterm version (0.29) as
// `ratatui_crossterm::crossterm`. Using this path everywhere keeps the crossterm
// types unified — Plan 038 §1.6 found that reedline's own crossterm 0.29
// re-export and ash-tui's previous 0.27 pin were *different types*.
use ratatui_crossterm::crossterm::{
    event::{self, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::terminal::{Terminal, TerminalOptions, Viewport};
use ratatui_core::text::{Line, Span};
use reedline::{
    default_emacs_keybindings, default_vi_insert_keybindings,
    default_vi_normal_keybindings, Emacs, Vi,
};

use crate::editor::{Editor, EditorOutcome};

/// The fixed height of the inline viewport (rows). M2: 2 rows for the editor
/// (prompt+buffer, status) + up to 6 rows for the completion menu when open.
/// The unused rows are blank when the menu is closed (ratatui clears them).
const VIEWPORT_HEIGHT: u16 = 8;
/// Max completion candidates to show at once (the rest are reachable via ↓).
const MAX_MENU_ROWS: u16 = 6;

/// The block-TUI experimental REPL. Construct with [`BlockTui::run`]. Returns
/// when the user exits (Ctrl+D on empty / `exit` / Ctrl+C from the outer loop).
pub struct BlockTui;

impl BlockTui {
    /// Entry point — owns the terminal for the duration of the call.
    ///
    /// Restores the terminal on exit (including via a panic hook, so a panic
    /// inside the ratatui loop does not leave the user's terminal in raw mode).
    pub fn run() -> io::Result<()> {
        // Install a panic hook that restores the terminal before the panic
        // message prints — otherwise a panic leaves the user in a broken raw
        // mode terminal where the message is unreadable.
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            original_hook(info);
        }));

        // RAII guard: setup on creation, restore on drop. Guarantees terminal
        // restoration even when `run` returns early or propagates an error.
        let _guard = TerminalGuard::new()?;

        let mut terminal = Terminal::with_options(
            ratatui_crossterm::CrosstermBackend::new(stdout()),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT_HEIGHT),
            },
        )?;

        // M1: pick Emacs or Vi from $ASH_EDIT_MODE (mirrors the reedline REPL's
        // selection in repl.rs). Vi gives ESC/i/a/o + w/b/e motions in normal
        // mode; the EditMode parser emits the EditCommands our dispatch handles.
        let edit_mode = build_edit_mode();
        let mut editor = Editor::new(edit_mode);

        // M4: mode state (Shell/AutoScript/AI lock + continuation). Drives the
        // prompt symbol and F1-F4/Esc behavior. Pure data, reused as-is from
        // the reedline REPL (repl_mode.rs).
        let mut mode_state = auto_shell::repl_mode::ModeState::default();
        // M4: accumulates multi-line continuation input across submits.
        let mut pending_input = String::new();
        // M4 Gap 6: the modular prompt (directory + git branch + status),
        // same as the reedline REPL's AshPrompt. render_all() gives us the
        // left string (env info) to prepend before the mode symbol.
        let prompt = crate::prompt::AshPrompt::new(
            auto_shell::prompt::AshConfig::load(),
        );

        // M2/M3: build the execution Shell + history/completion/hint sources.
        // The Shell is the REAL execution shell (render hook + terminal
        // commands injected) — M3 uses it to run commands.
        let (mut shell, mut history_store, completer, hinter, completion_state, cwd) =
            build_shell_and_sources();
        // Initialize the git cache for prompt modules (directory/git_branch/
        // git_status). Mirrors repl.rs:739 (on_directory_changed on run start).
        auto_shell::prompt::context::on_directory_changed(shell.pwd());
        // HistoryNav is owned HERE (not in Editor) because it needs the
        // FileBackedHistory on each ↑/↓, and that store lives in this scope.
        let mut history_nav = crate::editor::history::HistoryNav::new();
        // HintSource lives HERE too (needs the history store). Editor only
        // holds the computed hint string + the completion menu.
        let mut hint_source: Option<crate::editor::hints::HintSource<_>> =
            hinter.map(|h| crate::editor::hints::HintSource::new(h));
        editor = editor.with_completion(crate::editor::completion::CompletionMenu::new(
            Box::new(completer),
        ));

        loop {
            // ── Draw the fixed-bottom viewport ──────────────────────
            // Row 0: prompt indicator + the editor's buffer + dim hint suffix.
            // Row 1: a status line.
            // Rows 2+: completion menu (only when open).
            let ip = editor.insertion_point();
            let buf_text = editor.text().to_string();
            let hint_text = editor.hint().to_string();
            // Snapshot the completion menu state for the closure.
            let comp_suggestions: Vec<(String, String, usize)> = editor
                .completion()
                .and_then(|m| if m.is_open() { Some(m) } else { None })
                .map(|m| {
                    m.suggestions()
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            (
                                s.value.clone(),
                                s.description.clone().unwrap_or_default(),
                                i,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let comp_selected = editor.completion().map_or(0, |m| m.selected());
            terminal.draw(|frame| {
                let area: Rect = frame.area();
                // Input line: prompt symbol + typed text + dim hint suffix.
                // The prompt symbol comes from mode_state (M4): `▌` prefix if
                // locked (blue), then the mode symbol (>/#/?/·).
                let (prompt_spans, prompt_width) = prompt_spans(&mode_state, &prompt);
                let mut input_spans = prompt_spans;
                input_spans.push(Span::raw(&buf_text));
                if !hint_text.is_empty() {
                    input_spans.push(Span::styled(
                        hint_text.clone(),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                let input_line = Line::from(input_spans);
                let status = Line::from(vec![
                    Span::styled("block-tui M2 ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        "Enter=submit  Tab=complete  ↑↓=history  Ctrl+D=exit",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                frame.render_widget(input_line, Rect::new(area.x, area.y, area.width, 1));
                frame.render_widget(status, Rect::new(area.x, area.y + 1, area.width, 1));

                // Completion menu (rows 2+), only if there are candidates.
                if !comp_suggestions.is_empty() {
                    for (row, (value, desc, idx)) in comp_suggestions.iter().enumerate() {
                        let selected = *idx == comp_selected;
                        let style = if selected {
                            Style::default().fg(Color::Black).bg(Color::Cyan)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        let label = if desc.is_empty() {
                            value.clone()
                        } else {
                            format!("{value}  {desc}")
                        };
                        let menu_line = Line::from(vec![Span::styled(format!("  {label}"), style)]);
                        let row_y = area.y + 2 + row as u16;
                        if row_y < area.bottom() {
                            frame.render_widget(
                                menu_line,
                                Rect::new(area.x, row_y, area.width, 1),
                            );
                        }
                    }
                }

                // Place the visible caret over the insertion point. Column =
                // viewport x + prompt width (2 for "❯ ") + display width of
                // the text left of the insertion point.
                let prefix = buf_text.get(..ip).unwrap_or("");
                let col_offset = unicode_width::UnicodeWidthStr::width(prefix) as u16;
                // Cursor sits after the prompt (width varies with mode symbol).
                frame.set_cursor_position((area.x + prompt_width + col_offset, area.y));
            })?;

            // ── Read + dispatch one event ───────────────────────────
            // Editor::handle_event takes the raw crossterm Event directly so
            // it can intercept arrow keys (which reedline's default keybindings
            // leave unbound → would be a no-op).
            let event = event::read()?;

            // M4: drain any finished suggest-next results (background thread,
            // never blocks). Push them into the scrollback as a dim hint block
            // (like the reedline REPL's println, but via insert_before).
            if let Some(sugs) = auto_shell::ai::suggest::take_pending() {
                if !sugs.is_empty() {
                    let n = sugs.len();
                    terminal.insert_before(1 + n as u16, move |buf| {
                        use ratatui_core::text::{Line, Span};
                        let header = Line::from(vec![Span::styled(
                            "💡 接下来可能想:",
                            Style::default().fg(Color::DarkGray),
                        )]);
                        buf.set_line(buf.area.x, buf.area.y, &header, buf.area.width);
                        for (i, s) in sugs.iter().enumerate() {
                            let line = Line::from(vec![Span::styled(
                                format!("   {s}"),
                                Style::default().fg(Color::DarkGray),
                            )]);
                            let y = buf.area.y + 1 + i as u16;
                            if y < buf.area.bottom() {
                                buf.set_line(buf.area.x, y, &line, buf.area.width);
                            }
                        }
                    })?;
                }
            }

            // M4: mode-switch keys (F1/F2/Esc) intercepted BEFORE the editor.
            // Unlike the reedline REPL (which injected prefix bytes + Submit),
            // ratatui gives us the raw KeyCode — so we just toggle mode state
            // and skip the editor entirely (the buffer is untouched).
            if let Some(mode_changed) = handle_mode_key(&event, &mut mode_state) {
                if mode_changed {
                    // Clear any pending continuation when switching modes.
                    pending_input.clear();
                    mode_state.in_continuation = false;
                    editor.take_line(); // reset buffer
                    continue;
                }
            }

            // M4: Ctrl+E — open the current line in $EDITOR (teardown/rebuild
            // ratatui around the editor subprocess). The edited result replaces
            // the buffer.
            if is_ctrl_e(&event) {
                let current = editor.text().to_string();
                match crate::subprocess::run_external_editor(&mut terminal, &current) {
                    Ok(edited) => {
                        if !edited.is_empty() {
                            editor.replace_buffer(edited);
                        }
                    }
                    Err(e) => {
                        // Push the error as a block so the user sees it.
                        terminal.insert_before(1, |buf| {
                            let line = Line::from(vec![Span::styled(
                                format!("editor: {e}"),
                                Style::default().fg(Color::Red),
                            )]);
                            buf.set_line(buf.area.x, buf.area.y, &line, buf.area.width);
                        })?;
                    }
                }
                continue;
            }

            // M4 Gap 5: Ctrl+R — reverse history search (inline, like bash).
            // Enters a sub-loop: types a query, shows matching history entries
            // pushed into scrollback, Enter selects one.
            if is_ctrl_r(&event) {
                handle_history_search(&mut terminal, &history_store, &mut editor)?;
                continue;
            }

            // M4-7: F4 / Alt+4 — persistent AI chat (ReAct loop with tool use).
            // Also synchronous (the chat blocks the event loop per turn).
            if is_ai_chat_key(&event) {
                handle_ai_chat(&mut shell, &mut terminal)?;
                continue;
            }

            // M2: history navigation intercept. When the completion menu is
            // closed and the user hits ↑/↓, walk the history store (Editor
            // itself only knows line-start/end for ↑/↓ since it doesn't own
            // the FileBackedHistory).
            let menu_open = editor
                .completion()
                .map_or(false, |m| m.is_open());
            let history_dir = if !menu_open {
                history_arrow_direction(&event)
            } else {
                None
            };
            let handled_by_history = match history_dir {
                Some(older) => handle_history_arrow(
                    &mut editor,
                    &mut history_nav,
                    &history_store,
                    older,
                ),
                None => false,
            };

            let outcome = if handled_by_history {
                EditorOutcome::Continue
            } else {
                editor.handle_event(event)
            };

            // M2: recompute the autosuggestion hint after each event (the
            // outer loop owns the HintSource since it needs the history store).
            if let Some(hs) = hint_source.as_mut() {
                let h = hs.current_hint(
                    editor.text(),
                    editor.insertion_point(),
                    &history_store,
                    &cwd,
                );
                editor.set_hint(h);
            } else {
                editor.set_hint(String::new());
            }

            match outcome {
                EditorOutcome::Continue => continue,
                EditorOutcome::Exit => break,
                EditorOutcome::Submitted => {
                    let line = editor.take_line();

                    // M4: multi-line continuation. If the accumulated input
                    // needs continuation (unclosed { } ( ) [ ] " ' or trailing
                    // backslash), append this line to the pending buffer, set
                    // the continuation prompt (·), and keep editing in the same
                    // editor — no nested read_line (simpler than reedline).
                    let acc = if pending_input.is_empty() {
                        line.clone()
                    } else {
                        // Joining: trailing backslash → space (line continuation),
                        // unclosed delimiter → newline.
                        let joiner = if pending_input.trim_end().ends_with('\\')
                            && !pending_input.trim_end().ends_with("\\\\")
                        {
                            pending_input.truncate(pending_input.trim_end().len() - 1);
                            ' ' as char
                        } else {
                            '\n'
                        };
                        format!("{pending_input}{joiner}{line}")
                    };

                    if auto_shell::repl_mode::needs_continuation(&acc) {
                        pending_input = acc;
                        mode_state.in_continuation = true;
                        continue;
                    }
                    // Complete line: clear continuation state.
                    mode_state.in_continuation = false;
                    let full_line = if pending_input.is_empty() {
                        line
                    } else {
                        let rest = pending_input.clone();
                        pending_input.clear();
                        rest
                    };
                    let mut trimmed = full_line.trim().to_string();

                    // M2: persist non-empty lines to history (shared file).
                    if !trimmed.is_empty() {
                        use reedline::History;
                        let _ = history_store
                            .save(reedline::HistoryItem::from_command_line(trimmed.clone()));
                    }

                    // exit/quit leave the loop.
                    if trimmed == "exit" || trimmed == "quit" || trimmed == "q" {
                        break;
                    }
                    // Empty line: no block, just a fresh prompt.
                    if trimmed.is_empty() {
                        continue;
                    }

                    // M4 Gap 3: expand abbreviations (abbr) in-line.
                    // Mirrors repl.rs:936-940. If expanded, show the expanded
                    // form so the user sees what will actually run.
                    let (expanded_abbr, abbr_changed) = shell.expand_abbreviations(&trimmed);
                    if abbr_changed {
                        push_block(
                            &mut terminal,
                            &[&format!("→ {expanded_abbr}")],
                            Color::DarkGray,
                        )?;
                        trimmed = expanded_abbr;
                    }

                    // M4 Gap 2: expand history references (!!, !n, !?string).
                    // Mirrors repl.rs:942-955. Reads the shared history file
                    // (~/.auto-shell-history) and applies bash-style expansion.
                    if trimmed.contains('!') {
                        match expand_history_refs(&trimmed, &history_store) {
                            Ok(Some(expanded)) => {
                                push_block(
                                    &mut terminal,
                                    &[&format!("→ {expanded}")],
                                    Color::DarkGray,
                                )?;
                                trimmed = expanded;
                            }
                            Ok(None) => {} // no expansion needed
                            Err(e) => {
                                push_block(
                                    &mut terminal,
                                    &[&format!("history expansion error: {e}")],
                                    Color::Red,
                                )?;
                                continue;
                            }
                        }
                    }

                    // M4: update last_auto before execution (for AI mode restore).
                    if mode_state.locked.is_none() {
                        mode_state.last_auto = if shell.is_auto_expression_pub(&trimmed) {
                            auto_shell::repl_mode::InputMode::AutoScript
                        } else {
                            auto_shell::repl_mode::InputMode::Shell
                        };
                    }

                    // M3: interactive commands (vim/less/top/...) need full
                    // terminal control — tear down ratatui, run them with
                    // inherited stdio, then rebuild. Non-interactive commands
                    // go through Shell::execute and render as a block.
                    //
                    // Also intercept the BUILT-IN `less`/`more` commands and
                    // `show --pager`: they're registered Shell commands (not
                    // external programs), so is_interactive_command doesn't
                    // catch them — but they enter their own alt-screen + raw
                    // mode, which conflicts with the block TUI's held raw mode
                    // (the guard's disable_raw_mode on drop would break us).
                    // Running them through the teardown/rebuild handoff lets
                    // them manage the terminal from a clean cooked state.
                    let first_word = trimmed.split_whitespace().next().unwrap_or("");
                    let is_pager_cmd = first_word == "less" || first_word == "more";
                    let is_paged_show = first_word == "show" && trimmed.contains("--pager");
                    if ash_core::cmd::interactive::is_interactive_command(&trimmed)
                        || is_pager_cmd
                        || is_paged_show
                    {
                        if is_pager_cmd || is_paged_show {
                            // Built-in pager: teardown ratatui → shell.execute
                            // (the pager manages its own terminal) → rebuild.
                            // The output (if any, e.g. when piped) is discarded
                            // since the pager already displayed it.
                            crate::subprocess::run_with_handoff(&mut terminal, || {
                                let _ = shell.execute(&trimmed);
                            })?;
                        } else {
                            // External interactive command (vim/ssh/top).
                            crate::subprocess::hand_off_to_interactive(
                                &mut terminal,
                                &trimmed,
                                &shell.pwd(),
                            )?;
                        }
                        continue;
                    }

                    // ── Execute + render (mirrors repl.rs execute_with_header) ──
                    let start = std::time::Instant::now();

                    // Gap 4: try structured rendering first (ls/ps/find → table
                    // widget). Non-atom commands fall through to text.
                    if let Some(rendered) = try_render_structured(&mut shell, &trimmed) {
                        let elapsed = start.elapsed();
                        let exit_code = shell.last_exit_code();
                        render_structured_block(
                            &mut terminal, &trimmed, exit_code, elapsed, &rendered,
                        )?;
                    } else {
                        let result = shell.execute(&trimmed);
                        let elapsed = start.elapsed();
                        let exit_code = match &result {
                            Ok(_) => shell.last_exit_code(),
                            Err(_) => {
                                let c = shell.last_exit_code();
                                if c != 0 { c } else { 1 }
                            }
                        };
                        let (body_text, is_error): (Option<String>, bool) = match result {
                            Ok(Some(s)) => (Some(s), false),
                            Ok(None) => (None, false),
                            Err(e) => (Some(format!("Error: {e}")), true),
                        };
                        let output_snippet: String = body_text
                            .as_deref()
                            .map(|s| s.chars().take(200).collect())
                            .unwrap_or_default();
                        let body_lines: Vec<String> = body_text
                            .map(|s| strip_ansi(&s).lines().map(|l| l.to_string()).collect())
                            .unwrap_or_default();
                        let height = 1u16 + body_lines.len() as u16;
                        let header_cmd = trimmed.clone();
                        terminal.insert_before(height, move |buf| {
                            render_block(buf, &header_cmd, exit_code, elapsed, &body_lines, is_error);
                        })?;
                        // suggest-next (only for the text path; structured
                        // commands have their output already displayed).
                        if auto_shell::ai::suggest::is_enabled() {
                            auto_shell::ai::suggest::suggest_next_async(
                                shell.pwd().to_string_lossy().to_string(),
                                trimmed.clone(),
                                output_snippet,
                            );
                        }
                    }

                    // Sync completion state after execution.
                    if let Ok(mut state) = completion_state.lock() {
                        state.current_dir = shell.pwd().to_path_buf();
                        state.last_command = shell.last_command_line().map(String::from);
                        state.last_exit_code = Some(shell.last_exit_code());
                        state.aliases = shell.aliases().clone();
                    }
                    continue;
                }
            }
        }

        Ok(())
    }
}


/// Is the event F3/F4 or Alt+3/Alt+4 (AI chat)? Plan 069: F3 (old NL translate)
/// now enters the same unified AI chat — the CLI has ONE AI mode.
fn is_ai_chat_key(event: &ratatui_crossterm::crossterm::event::Event) -> bool {
    use ratatui_crossterm::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    if let Event::Key(ke) = event {
        matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            && (ke.code == KeyCode::F(3)
                || ke.code == KeyCode::F(4)
                || (ke.code == KeyCode::Char('3') && ke.modifiers.contains(KeyModifiers::ALT))
                || (ke.code == KeyCode::Char('4') && ke.modifiers.contains(KeyModifiers::ALT)))
    } else {
        false
    }
}


/// Commands sent from the main thread to the chat worker thread.
enum ChatCmd {
    /// Submit a user turn (with the shell context snapshot for update_context).
    Turn { user: String, context: String },
    /// Clear the conversation.
    Clear,
    /// Exit the worker (drops the session).
    Exit,
}

/// Events sent from the chat worker thread back to the main thread.
enum ChatEv {
    /// A chunk of streamed assistant text.
    Delta(String),
    /// A formatted tool/warning/error line.
    ToolLine(String),
    /// The turn completed (Ok = final text, Err = error message).
    Done(Result<String, String>),
    /// A /clear completed.
    Cleared,
}

/// Spawn a background worker thread that owns the ChatSession. Returns:
/// - the cmd sender (to submit turns / clear / exit)
/// - the event receiver (to drain streaming events)
/// - the JoinHandle (to reclaim the thread on exit)
fn spawn_chat_worker(
    mut session: auto_shell::ai::ChatSession,
) -> (
    std::sync::mpsc::Sender<ChatCmd>,
    std::sync::mpsc::Receiver<ChatEv>,
    std::thread::JoinHandle<()>,
) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<ChatCmd>();
    let (ev_tx, ev_rx) = std::sync::mpsc::channel::<ChatEv>();

    let handle = std::thread::Builder::new()
        .name("ash-block-tui-chat".into())
        .spawn(move || {
            for cmd in cmd_rx {
                match cmd {
                    ChatCmd::Exit => break,
                    ChatCmd::Clear => {
                        session.clear();
                        let _ = session.save();
                        let _ = ev_tx.send(ChatEv::Cleared);
                    }
                    ChatCmd::Turn { user, context } => {
                        session.set_context_str(context);
                        // The on_event callback sends StreamEvents over the
                        // channel as ChatEv variants (no stdout writes).
                        let tx = ev_tx.clone();
                        let on_event: std::sync::Arc<
                            dyn Fn(auto_ai_agent::agent::StreamEvent) + Send + Sync,
                        > = std::sync::Arc::new(move |ev| {
                            let chat_ev = match ev {
                                auto_ai_agent::agent::StreamEvent::Delta { text } => {
                                    ChatEv::Delta(text)
                                }
                                auto_ai_agent::agent::StreamEvent::ToolStart { tool, args } => {
                                    ChatEv::ToolLine(format!(
                                        "  ⚙ {tool} {}",
                                        auto_shell::ai::brief::brief_args(&args)
                                    ))
                                }
                                auto_ai_agent::agent::StreamEvent::Tool {
                                    tool, result, ..
                                } => ChatEv::ToolLine(format!(
                                    "  ← {tool}: {}",
                                    auto_shell::ai::brief::brief_result(&result)
                                )),
                                auto_ai_agent::agent::StreamEvent::Warning { text } => {
                                    ChatEv::ToolLine(format!("  ⚠ {text}"))
                                }
                                auto_ai_agent::agent::StreamEvent::Error { message } => {
                                    ChatEv::ToolLine(format!("  [error] {message}"))
                                }
                                auto_ai_agent::agent::StreamEvent::Cancelled { .. } => {
                                    ChatEv::ToolLine("  [cancelled]".to_string())
                                }
                                auto_ai_agent::agent::StreamEvent::Thinking { text } => {
                                    ChatEv::ToolLine(format!("  💭 {text}"))
                                }
                                auto_ai_agent::agent::StreamEvent::Done { .. } => {
                                    // Done is handled by the send_turn_streaming return.
                                    return;
                                }
                                // auto-ai 新增的回合边界事件(2026-08-23 漂移,
                                // ask.rs 同款兜底):CLI 内联展示无需呈现。
                                auto_ai_agent::agent::StreamEvent::TurnStart { .. }
                                | auto_ai_agent::agent::StreamEvent::TurnEnd { .. } => {
                                    return;
                                }
                            };
                            let _ = tx.send(chat_ev);
                        });
                        let result = auto_shell::ai::block_on_async(
                            session.send_turn_streaming(&user, on_event),
                        );
                        match result {
                            Ok(text) => {
                                let _ = session.save();
                                let _ = ev_tx.send(ChatEv::Done(Ok(text)));
                            }
                            Err(e) => {
                                let _ = ev_tx.send(ChatEv::Done(Err(e)));
                            }
                        }
                    }
                }
            }
        })
        .expect("failed to spawn chat worker thread");

    (cmd_tx, ev_rx, handle)
}

/// F4: persistent AI chat with real-time streaming output.
///
/// Uses a background worker thread (owns ChatSession) + bidirectional channels.
/// The main loop is poll-driven (event::poll + channel drain), so ratatui
/// redraws during the AI turn — the user sees Delta text and tool events
/// appear live, and the viewport never freezes.
fn handle_ai_chat(
    shell: &mut auto_shell::Shell,
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
) -> io::Result<()> {
    // Load the session on the main thread (sync daemon probe).
    let session = match auto_shell::ai::ChatSession::load() {
        Ok(s) => s,
        Err(e) => {
            push_block(
                terminal,
                &[
                    &format!("  AI error: {e}"),
                    "  (set ZHIPU_API_KEY / ANTHROPIC_API_KEY / OPENAI_API_KEY or start aaid daemon)",
                ],
                Color::Red,
            )?;
            return Ok(());
        }
    };
    let turns = session.turn_count();
    let (cmd_tx, ev_rx, _worker_handle) = spawn_chat_worker(session);

    if turns > 0 {
        push_block(terminal, &[&format!("  * 已恢复 {} 轮对话 *", turns / 2)], Color::Cyan)?;
    } else {
        push_block(
            terminal,
            &["  * 开始新对话 *  (/clear 清空  /exit 退出  Esc/F4 离开)"],
            Color::Cyan,
        )?;
    }

    // The chat input loop: poll for keys + drain streaming events.
    let mut input_buf = String::new();
    let mut streaming_text = String::new();
    let mut tool_lines: Vec<String> = Vec::new();
    let mut ai_busy = false; // true while waiting for a turn to complete.

    loop {
        // ── Draw the current chat state ──────────────────────
        // If streaming, show the accumulated text + tool lines + input prompt.
        if ai_busy || !streaming_text.is_empty() || !tool_lines.is_empty() {
            let mut lines: Vec<String> = Vec::new();
            for tl in &tool_lines {
                lines.push(tl.clone());
            }
            if !streaming_text.is_empty() {
                lines.push(streaming_text.clone());
            }
            if ai_busy {
                lines.push("  ⏳ ...".to_string());
            }
            // We can't re-draw the scrollback; instead, push a snapshot.
            // But pushing every frame floods the scrollback. So we DON'T push
            // during streaming — we draw into the viewport instead (below).
        }

        // Draw the chat prompt + input into the viewport (not scrollback).
        terminal.draw(|frame| {
            let area: Rect = frame.area();
            let prompt_text = if ai_busy {
                "▌? (AI thinking...) ".to_string()
            } else {
                format!("▌? {input_buf}")
            };
            let prompt_line = Line::from(vec![Span::styled(
                prompt_text,
                Style::default().fg(Color::Magenta),
            )]);
            frame.render_widget(prompt_line, Rect::new(area.x, area.y, area.width, 1));

            // If streaming, show accumulated text below the prompt.
            if !streaming_text.is_empty() {
                let stream_lines: Vec<&str> = streaming_text.lines().collect();
                for (i, sl) in stream_lines.iter().enumerate() {
                    let row_y = area.y + 1 + i as u16;
                    if row_y < area.bottom() {
                        let line = Line::from(vec![Span::raw(format!("  {sl}"))]);
                        frame.render_widget(line, Rect::new(area.x, row_y, area.width, 1));
                    }
                }
            }
        })?;

        // ── Poll for a key event (non-blocking, 50ms timeout) ──
        if event::poll(std::time::Duration::from_millis(50))? {
            let event = event::read()?;
            if let Some(kc) = key_code(&event) {
                match kc {
                    KeyCode::Enter => {
                        let line = input_buf.trim().to_string();
                        input_buf.clear();
                        if line.is_empty() {
                            continue;
                        }
                        // Slash commands.
                        if let Some(cmd) = auto_shell::ai::parse_slash_command(&line) {
                            match cmd {
                                auto_shell::ai::SlashCommand::Exit => {
                                    let _ = cmd_tx.send(ChatCmd::Exit);
                                    break;
                                }
                                auto_shell::ai::SlashCommand::Clear => {
                                    let _ = cmd_tx.send(ChatCmd::Clear);
                                    // Wait for Cleared confirmation.
                                    loop {
                                        match ev_rx.recv() {
                                            Ok(ChatEv::Cleared) => {
                                                push_block(
                                                    terminal,
                                                    &["  * 对话已清空 *"],
                                                    Color::DarkGray,
                                                )?;
                                                break;
                                            }
                                            Ok(_) => {}
                                            Err(_) => break,
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                        if line.starts_with('/') {
                            push_block(
                                terminal,
                                &[&format!("  未知命令: {line} (可用: /clear /exit)")],
                                Color::DarkGray,
                            )?;
                            continue;
                        }
                        // Submit the turn.
                        let context = auto_shell::ai::context::build_context_block(shell);
                        streaming_text.clear();
                        tool_lines.clear();
                        ai_busy = true;
                        let _ = cmd_tx.send(ChatCmd::Turn {
                            user: line,
                            context,
                        });
                    }
                    KeyCode::Esc => {
                        if ai_busy {
                            // Can't cancel yet (v1 no cancel); just ignore.
                        } else {
                            let _ = cmd_tx.send(ChatCmd::Exit);
                            break;
                        }
                    }
                    KeyCode::Backspace => {
                        input_buf.pop();
                    }
                    KeyCode::Char(c) => {
                        if !event_has_ctrl(&event) {
                            input_buf.push(c);
                        }
                    }
                    _ => {}
                }
            }
        }

        // ── Drain streaming events (non-blocking) ────────────
        while let Ok(ev) = ev_rx.try_recv() {
            match ev {
                ChatEv::Delta(text) => {
                    streaming_text.push_str(&text);
                }
                ChatEv::ToolLine(line) => {
                    tool_lines.push(line);
                }
                ChatEv::Done(result) => {
                    ai_busy = false;
                    // Push the complete turn output as a block into scrollback.
                    let mut block_lines: Vec<String> = tool_lines.drain(..).collect();
                    match result {
                        Ok(final_text) => {
                            if !streaming_text.is_empty() {
                                block_lines.push(streaming_text.clone());
                            }
                            streaming_text.clear();
                        }
                        Err(e) => {
                            block_lines.push(format!("  AI error: {e}"));
                            block_lines.push("  (check API key / daemon)".to_string());
                            streaming_text.clear();
                        }
                    }
                    if !block_lines.is_empty() {
                        let refs: Vec<&str> = block_lines.iter().map(|s| s.as_str()).collect();
                        push_block(terminal, &refs, Color::DarkGray)?;
                    }
                    break; // break the while-recv loop, go back to polling
                }
                ChatEv::Cleared => {
                    // Unexpected here (Clear is handled synchronously above).
                }
            }
        }
    }

    // Ensure the worker exits cleanly.
    let _ = cmd_tx.send(ChatCmd::Exit);
    Ok(())
}

/// Push a multi-line block of text into the scrollback above the viewport.
fn push_block(
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
    lines: &[&str],
    color: Color,
) -> io::Result<()> {
    let owned: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let n = owned.len() as u16;
    terminal.insert_before(n, move |buf| {
        for (i, text) in owned.iter().enumerate() {
            let line = Line::from(vec![Span::styled(text.clone(), Style::default().fg(color))]);
            let y = buf.area.y + i as u16;
            if y < buf.area.bottom() {
                buf.set_line(buf.area.x, y, &line, buf.area.width);
            }
        }
    })
}

/// Read a line of input in raw mode, character by character, until Enter.
/// Writes each char to stdout as it's typed (simple echo). Used by F3/F4
/// for quick prompt reads that don't need the full editor.
fn read_raw_line(
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
    buf: &mut String,
) -> io::Result<()> {
    use std::io::Write;
    loop {
        let event = event::read()?;
        if let Some(code) = key_code(&event) {
            match code {
                KeyCode::Enter => {
                    let mut out = std::io::stdout();
                    let _ = writeln!(out);
                    let _ = out.flush();
                    break;
                }
                KeyCode::Backspace => {
                    if buf.pop().is_some() {
                        let mut out = std::io::stdout();
                        let _ = out.write_all(b"\x08 \x08");
                        let _ = out.flush();
                    }
                }
                KeyCode::Char(c) => {
                    buf.push(c);
                    let mut out = std::io::stdout();
                    let _ = write!(out, "{c}");
                    let _ = out.flush();
                }
                KeyCode::Esc => {
                    buf.clear();
                    break;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Extract the KeyCode from a crossterm event (if it's a key press/repeat).
fn key_code(event: &ratatui_crossterm::crossterm::event::Event) -> Option<KeyCode> {
    use ratatui_crossterm::crossterm::event::{Event, KeyEventKind};
    if let Event::Key(ke) = event {
        if matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Some(ke.code);
        }
    }
    None
}

/// Is the event an Enter keypress?
fn is_enter(event: &ratatui_crossterm::crossterm::event::Event) -> bool {
    key_code(event) == Some(KeyCode::Enter)
}

/// Try to execute a command via the structured atom path and get a
/// `RenderedOutput`. Returns `None` for non-atom commands (echo, external,
/// aliases) — the caller falls back to `shell.execute`.
///
/// Ported from ash-gui's `render_structured` (ash-gui-bin/src/main.rs:350).
fn try_render_structured(
    shell: &mut auto_shell::Shell,
    input: &str,
) -> Option<ash_core::renderer::RenderedOutput> {
    use ash_core::pipeline::AtomPipeline;
    use ash_core::renderer::render_pipeline_to_structured;
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
        return Some(ash_core::renderer::RenderedOutput::Text(signature.format_help()));
    }
    let pipeline: AtomPipeline = cmd
        .run_atom(&parsed, AtomPipeline::empty(), shell)
        .ok()?;
    render_pipeline_to_structured(&pipeline)
        .or(Some(ash_core::renderer::RenderedOutput::Text(pipeline.into_text())))
}

/// Render a structured `RenderedOutput` as a block into the scrollback.
/// Tables get the full ratatui widget (borders, zebra striping, colored cells);
/// Text/Record/Error get line-based rendering. Gap 4.
fn render_structured_block(
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
    command: &str,
    exit_code: i32,
    elapsed: std::time::Duration,
    rendered: &ash_core::renderer::RenderedOutput,
) -> io::Result<()> {
    use ash_core::renderer::RenderedOutput;
    use auto_shell::config::IconStyle;

    let term_width = ratatui_crossterm::crossterm::terminal::size()
        .map(|(w, _)| w)
        .unwrap_or(80);
    let icons = IconStyle::Plain;

    // For Table: render the widget directly via render_table_to_buffer.
    if let RenderedOutput::Table { columns, rows, .. } = rendered {
        // Compute table height: border top + header + border + data rows + border.
        let table_height = 3u16 + rows.len() as u16;
        let total_height = 1u16 + table_height; // command header + table

        let header_cmd = command.to_string();
        let rendered_clone = rendered.clone();
        terminal.insert_before(total_height, move |buf| {
            // Row 0: command header.
            render_block(buf, &header_cmd, exit_code, elapsed, &[], false);
            // Rows 1+: table widget, rendered into a temp buffer then blitted.
            let mut temp = ratatui_core::buffer::Buffer::empty(
                ratatui_core::layout::Rect::new(0, 0, buf.area.width, table_height),
            );
            let _ = crate::renderer::render_table_to_buffer(
                &mut temp,
                &rendered_clone,
                buf.area.width,
                icons,
            );
            // Blit temp → buf starting at row 1 (below the header).
            for y in 0..table_height {
                for x in 0..buf.area.width {
                    let dst_y = buf.area.y + 1 + y;
                    if dst_y < buf.area.bottom() {
                        let src = temp.get(x, y).clone();
                        *buf.get_mut(buf.area.x + x, dst_y) = src;
                    }
                }
            }
        })?;
        return Ok(());
    }

    // Plan 042 M6: Code — syntax-highlighted spans carry RGB + bold/italic, so
    // render them as colored ratatui spans (like the GUI/web do) instead of
    // flattening to plain text.
    if let RenderedOutput::Code { lines, .. } = rendered {
        let height = 1u16 + lines.len() as u16;
        let header_cmd = command.to_string();
        let code_lines = lines.clone();
        terminal.insert_before(height, move |buf| {
            render_block_header(buf, &header_cmd, exit_code, elapsed);
            for (i, spans) in code_lines.iter().enumerate() {
                let y = buf.area.y + 1 + i as u16;
                if y >= buf.area.bottom() {
                    break;
                }
                let line_spans: Vec<Span> = spans
                    .iter()
                    .map(|s| {
                        let mut style = Style::default().fg(Color::Rgb(s.r, s.g, s.b));
                        if s.bold {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                        if s.italic {
                            style = style.add_modifier(Modifier::ITALIC);
                        }
                        Span::styled(s.text.clone(), style)
                    })
                    .collect();
                buf.set_line(buf.area.x, y, &Line::from(line_spans), buf.area.width);
            }
        })?;
        return Ok(());
    }

    // Non-table: text rendering (same as the plain execute path).
    let body_text = match rendered {
        RenderedOutput::Text(t) => Some(t.clone()),
        RenderedOutput::Error { message, .. } => Some(message.clone()),
        RenderedOutput::Empty => None,
        RenderedOutput::Record { fields, .. } => {
            let lines: Vec<String> = fields
                .iter()
                .map(|(k, c)| {
                    let v = match c {
                        ash_core::renderer::RenderedCell::Text(t)
                        | ash_core::renderer::RenderedCell::Tagged { text: t, .. } => t.clone(),
                    };
                    format!("{k}: {v}")
                })
                .collect();
            Some(lines.join("\n"))
        }
        RenderedOutput::Table { .. } => unreachable!(),
        // Handled by the colored-spans branch above (early return).
        RenderedOutput::Code { .. } => unreachable!(),
        // Plan 062 T11: only the GUI worker produces this variant; render the
        // suggestion as its command text if it ever reaches the CLI.
        RenderedOutput::AiSuggestion { cmd, .. } => Some(format!("AI: {cmd}")),
    };
    let body_lines: Vec<String> = body_text
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default();
    let is_error = matches!(rendered, RenderedOutput::Error { .. });
    let height = 1u16 + body_lines.len() as u16;
    let header_cmd = command.to_string();
    terminal.insert_before(height, move |buf| {
        render_block(buf, &header_cmd, exit_code, elapsed, &body_lines, is_error);
    })?;
    Ok(())
}

/// Execute a command and render it as a block (the M3 path, extracted as a
/// helper so F3's decision flow can reuse it without duplicating logic).
fn execute_and_render(
    shell: &mut auto_shell::Shell,
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
    command: &str,
) -> io::Result<()> {
    let start = std::time::Instant::now();

    // Gap 4: try the structured path first (like ash-gui's render_structured).
    // If the command goes through the atom pipeline (ls/ps/find/...), we get
    // a RenderedOutput that we can render as a ratatui table widget directly —
    // no ANSI round-trip. Non-atom commands (echo, external, aliases) return
    // None and fall through to shell.execute → text.
    if let Some(rendered) = try_render_structured(shell, command) {
        let elapsed = start.elapsed();
        let exit_code = shell.last_exit_code();
        render_structured_block(terminal, command, exit_code, elapsed, &rendered)?;
        return Ok(());
    }

    // Fallback: plain shell.execute → text body (strip ANSI for display).
    let result = shell.execute(command);
    let elapsed = start.elapsed();
    let exit_code = match &result {
        Ok(_) => shell.last_exit_code(),
        Err(_) => {
            let c = shell.last_exit_code();
            if c != 0 { c } else { 1 }
        }
    };
    let (body_text, is_error): (Option<String>, bool) = match result {
        Ok(Some(s)) => (Some(s), false),
        Ok(None) => (None, false),
        Err(e) => (Some(format!("Error: {e}")), true),
    };
    let body_lines: Vec<String> = body_text
        .as_ref()
        .map(|s| strip_ansi(s).lines().map(|l| l.to_string()).collect())
        .unwrap_or_default();
    let height = 1u16 + body_lines.len() as u16;
    let header_cmd = command.to_string();
    terminal.insert_before(height, move |buf| {
        render_block(buf, &header_cmd, exit_code, elapsed, &body_lines, is_error);
    })?;
    Ok(())
}

/// Expand history references (`!!`, `!n`, `!?string`) in a command line.
/// Uses the shared FileBackedHistory store. Returns:
/// - `Ok(Some(expanded))` if expansion occurred
/// - `Ok(None)` if no expansion was needed
/// - `Err(msg)` on expansion failure
///
/// Ported from repl.rs:320-352 (expand_line_history) but uses the live
/// FileBackedHistory (via History::search) instead of re-reading the raw file.
fn expand_history_refs(
    line: &str,
    history: &reedline::FileBackedHistory,
) -> Result<Option<String>, String> {
    use reedline::{History, SearchDirection, SearchQuery};
    let query = SearchQuery::everything(SearchDirection::Backward, None);
    let items = history.search(query).map_err(|e| format!("{e}"))?;
    let strings: Vec<String> = items.into_iter().map(|it| it.command_line).collect();
    if strings.is_empty() {
        return Ok(None);
    }
    struct FileHistory(Vec<String>);
    impl ash_core::parser::history::History for FileHistory {
        fn search(&self, _query: Option<&str>) -> Vec<String> {
            self.0.clone()
        }
    }
    let fh = FileHistory(strings);
    let expanded = ash_core::parser::history::expand_history(&line.to_string(), &fh)
        .map_err(|e| format!("{e}"))?;
    if expanded != line {
        Ok(Some(expanded))
    } else {
        Ok(None)
    }
}

/// Is the event Ctrl+R (reverse history search)?
fn is_ctrl_r(event: &ratatui_crossterm::crossterm::event::Event) -> bool {
    use ratatui_crossterm::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    if let Event::Key(ke) = event {
        matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            && ke.modifiers.contains(KeyModifiers::CONTROL)
            && ke.code == KeyCode::Char('r')
    } else {
        false
    }
}

/// Ctrl+R: interactive reverse history search. Types a query → shows matching
/// history entries (newest-first) pushed into scrollback → Enter selects one
/// (puts it in the editor buffer) / Esc cancels.
fn handle_history_search(
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
    history: &reedline::FileBackedHistory,
    editor: &mut Editor,
) -> io::Result<()> {
    use reedline::{History, SearchDirection, SearchQuery};
    let mut query = String::new();
    // Load all history once (newest-first).
    let all_history: Vec<String> = history
        .search(SearchQuery::everything(SearchDirection::Backward, None))
        .unwrap_or_default()
        .into_iter()
        .map(|it| it.command_line)
        .collect();

    loop {
        // Filter by query (case-insensitive substring).
        let matches: Vec<&String> = if query.is_empty() {
            all_history.iter().take(10).collect()
        } else {
            let q = query.to_lowercase();
            all_history
                .iter()
                .filter(|h| h.to_lowercase().contains(&q))
                .take(10)
                .collect()
        };
        // Render: query prompt + matching entries.
        let mut lines: Vec<String> = vec![format!("reverse-i-search: {query}")];
        if matches.is_empty() {
            lines.push("  (no matches)".to_string());
        } else {
            for (i, m) in matches.iter().enumerate() {
                let marker = if i == 0 { "▶" } else { " " };
                lines.push(format!("  {marker} {m}"));
            }
            lines.push("  [Enter] 选中首项  [Esc] 取消".to_string());
        }
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        push_block(terminal, &refs, Color::Cyan)?;

        // Read next key.
        let event = event::read()?;
        match key_code(&event) {
            Some(KeyCode::Enter) => {
                if let Some(first) = matches.first() {
                    editor.replace_buffer(first.to_string());
                }
                break;
            }
            Some(KeyCode::Esc) => break,
            Some(KeyCode::Backspace) => {
                query.pop();
            }
            Some(KeyCode::Char(c)) => {
                // Ignore Ctrl combos (Ctrl+C etc).
                if !event_has_ctrl(&event) {
                    query.push(c);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Does the event have the CONTROL modifier?
fn event_has_ctrl(event: &ratatui_crossterm::crossterm::event::Event) -> bool {
    use ratatui_crossterm::crossterm::event::{Event, KeyModifiers};
    if let Event::Key(ke) = event {
        ke.modifiers.contains(KeyModifiers::CONTROL)
    } else {
        false
    }
}

/// Is the event Ctrl+E (open in $EDITOR)?
fn is_ctrl_e(event: &ratatui_crossterm::crossterm::event::Event) -> bool {
    use ratatui_crossterm::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    if let Event::Key(ke) = event {
        matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            && ke.modifiers.contains(KeyModifiers::CONTROL)
            && ke.code == KeyCode::Char('e')
    } else {
        false
    }
}

/// If the event is a mode-switch key (F1/F2/Esc/Alt+1/Alt+2), apply it to
/// mode_state and return Some(true) if the mode changed (the editor should be
/// skipped). Returns None for non-mode keys (fall through to the editor).
///
/// F3 (AI suggest) and F4 (AI chat) are NOT handled here yet (M4-6/M4-7) —
/// they fall through to the editor as no-ops until wired.
fn handle_mode_key(
    event: &ratatui_crossterm::crossterm::event::Event,
    mode_state: &mut auto_shell::repl_mode::ModeState,
) -> Option<bool> {
    use ratatui_crossterm::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    let Event::Key(ke) = event else {
        return None;
    };
    if !matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }
    // F1 / Alt+1 → toggle Shell lock.
    if ke.code == KeyCode::F(1)
        || (ke.code == KeyCode::Char('1') && ke.modifiers.contains(KeyModifiers::ALT))
    {
        mode_state.toggle_lock(auto_shell::repl_mode::InputMode::Shell);
        return Some(true);
    }
    // F2 / Alt+2 → toggle AutoScript lock.
    if ke.code == KeyCode::F(2)
        || (ke.code == KeyCode::Char('2') && ke.modifiers.contains(KeyModifiers::ALT))
    {
        mode_state.toggle_lock(auto_shell::repl_mode::InputMode::AutoScript);
        return Some(true);
    }
    // Esc → unlock (unless in vi mode, where Esc enters normal mode — that's
    // handled by the EditMode parser inside the editor, not here. We only
    // intercept Esc when NOT in vi normal mode. But since we don't track vi
    // mode state here, and Esc-as-unlock is the reedline REPL behavior via
    // keybinding injection... M4 keeps it simple: Esc always unlocks. Vi
    // users use Ctrl+C or other keys. This matches the reedline REPL where
    // Esc was hard-bound to unlock via keybinding injection.)
    if ke.code == KeyCode::Esc && ke.modifiers == KeyModifiers::NONE {
        mode_state.unlock();
        return Some(true);
    }
    None
}

/// Build the prompt spans for the current mode state + the total display
/// width (for cursor positioning).
///
/// - If locked: a blue `▌` prefix + the mode symbol.
/// - In continuation: a dim `·`.
/// - Otherwise: just the mode symbol (`>` Shell green, `#` AutoScript cyan,
///   `?` AI magenta), each followed by a space.
fn prompt_spans(
    mode_state: &auto_shell::repl_mode::ModeState,
    prompt: &crate::prompt::AshPrompt,
) -> (Vec<Span<'static>>, u16) {
    use unicode_width::UnicodeWidthStr;
    // Gap 6: render the full modular prompt (directory + git + status).
    // render_all() returns (left, right, indicator) — left has the env info
    // (ANSI-styled via nu_ansi_term, which we strip for ratatui).
    let (left_ansi, _right, _indicator) = prompt.render_all();
    let left = strip_ansi(&left_ansi);

    let symbol = mode_state.prompt();
    let mut spans: Vec<Span<'static>> = Vec::new();
    // Prepend the env info (directory/git) if present.
    if !left.is_empty() {
        spans.push(Span::styled(
            format!("{left} "),
            Style::default().fg(Color::Blue),
        ));
    }
    // The prompt() string may start with `▌` (locked) — split it for coloring.
    let (prefix, main) = if let Some(rest) = symbol.strip_prefix('▌') {
        spans.push(Span::styled("▌".to_string(), Style::default().fg(Color::Blue)));
        ("▌", rest)
    } else {
        ("", symbol.as_str())
    };
    let color = if mode_state.in_continuation {
        Color::DarkGray
    } else {
        match mode_state.effective() {
            auto_shell::repl_mode::InputMode::Shell => Color::Green,
            auto_shell::repl_mode::InputMode::AutoScript => Color::Cyan,
            auto_shell::repl_mode::InputMode::AI => Color::Magenta,
        }
    };
    spans.push(Span::styled(format!("{main} "), Style::default().fg(color)));
    let left_w = UnicodeWidthStr::width(left.as_str()) as u16;
    let total = left_w
        + if !left.is_empty() { 1 } else { 0 } // space after left
        + UnicodeWidthStr::width(prefix) as u16
        + UnicodeWidthStr::width(main) as u16
        + 1; // trailing space
    (spans, total)
}

/// Render one completed block into the ratatui buffer: a status-colored
/// header line (command + duration + ✓/✗) followed by the output body.
///
/// This is the M3 replacement for the reedline REPL's `execute_with_header`
/// (which prints via `println!` in cooked mode). Here we draw directly into
/// the `insert_before` buffer with ratatui `Span::styled` — we do NOT reuse
/// `block_header::render_block_header` because it returns an ANSI string that
/// ratatui can't parse. We do reuse `block_header::format_duration` (pure
/// `Duration → String`, no ANSI).
fn render_block(
    buf: &mut ratatui_core::buffer::Buffer,
    command: &str,
    exit_code: i32,
    elapsed: std::time::Duration,
    body_lines: &[String],
    is_error: bool,
) {
    render_block_header(buf, command, exit_code, elapsed);

    // ── Body ──
    let body_style = if is_error {
        Style::default().fg(Color::Red)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    for (i, ln) in body_lines.iter().enumerate() {
        let y = buf.area.y + 1 + i as u16;
        if y >= buf.area.bottom() {
            break;
        }
        buf.set_string(buf.area.x, y, ln, body_style);
    }
}

/// The header row shared by every block: "❯ {command}  ...pad...  {duration}
/// {icon}". Split out of `render_block` so the Code path (colored spans, not
/// plain body lines) can reuse it.
fn render_block_header(
    buf: &mut ratatui_core::buffer::Buffer,
    command: &str,
    exit_code: i32,
    elapsed: std::time::Duration,
) {
    use unicode_width::UnicodeWidthStr;

    let w = buf.area.width;
    let dur = crate::block_header::format_duration(elapsed);
    let (icon, icon_color) = if exit_code == 0 {
        ("✓", Color::Green)
    } else {
        ("✗", Color::Red)
    };
    let left = format!("❯ {command}");
    let right = format!("{dur}  {icon}");
    let lw = UnicodeWidthStr::width(left.as_str()) as u16;
    let rw = UnicodeWidthStr::width(right.as_str()) as u16;
    let pad = w.saturating_sub(lw).saturating_sub(rw);
    let mut spans: Vec<Span> = vec![Span::styled(left, Style::default().fg(Color::DarkGray))];
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad as usize)));
    }
    spans.push(Span::styled(right, Style::default().fg(icon_color)));
    let header_line = Line::from(spans);
    buf.set_line(buf.area.x, buf.area.y, &header_line, w);
}

/// Strip ANSI escape sequences from a string.
///
/// `Shell::execute` returns text that may be ANSI-styled (structured commands
/// like `ls` go through `TuiRenderHook` → `rendered_to_ansi`). ratatui can't
/// parse ANSI, so for M3 we strip it and render the body as plain styled text.
/// (A ratatui-native table widget that consumes `RenderedOutput` directly is a
/// follow-up — see Plan 038 M3 §3.)
fn strip_ansi(s: &str) -> String {
    // CSI sequences: ESC [ ... letter. Also handle OSC (ESC ] ... BEL/ST).
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        // ESC seen — consume the rest of the escape sequence.
        match chars.next() {
            Some('[') => {
                // CSI: consume until a 0x40..0x7E byte (the final byte).
                while let Some(fc) = chars.next() {
                    if fc.is_ascii_alphabetic() || ('@'..='~').contains(&fc) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC: consume until BEL (\x07) or ST (ESC \).
                while let Some(fc) = chars.next() {
                    if fc == '\x07' {
                        break;
                    }
                    if fc == '\x1b' {
                        // ST = ESC \; consume the backslash.
                        let _ = chars.next();
                        break;
                    }
                }
            }
            _ => {
                // Other ESC sequences (e.g. ESC c, ESC =): consume one more.
            }
        }
    }
    out
}


///
/// Mirrors the selection in `repl.rs::Repl::new` (Plan 302 Step 3.2) with the
/// same three-layer fallback so the block TUI matches the reedline REPL:
///   1. `$ASH_EDIT_MODE` env var (`"vi"` selects Vi)
///   2. `~/.config/ash.toml` `[shell] edit_mode = "vi"`
///   3. `~/.ashrc` line `set editing-mode vi`
///
/// This matters on Windows/PowerShell where `ASH_EDIT_MODE=vi cargo run`
/// (bash-style inline env) doesn't work — users set it via ash.toml or
/// .ashrc instead. Returns Emacs by default.
fn build_edit_mode() -> Box<dyn reedline::EditMode> {
    if detect_vi_mode() {
        let insert_kb = default_vi_insert_keybindings();
        let normal_kb = default_vi_normal_keybindings();
        Box::new(Vi::new(insert_kb, normal_kb))
    } else {
        Box::new(Emacs::new(default_emacs_keybindings()))
    }
}

/// Three-layer Vi-mode detection (env > ash.toml > ~/.ashrc).
fn detect_vi_mode() -> bool {
    // 1. Environment variable.
    if std::env::var("ASH_EDIT_MODE").map(|v| v == "vi").unwrap_or(false) {
        return true;
    }
    // 2. ash.toml.
    let shell_config = auto_shell::config::AshShellConfig::load();
    if shell_config.is_vi_mode() {
        return true;
    }
    // 3. ~/.ashrc `set editing-mode vi`.
    if let Some(home) = dirs::home_dir() {
        let rc = home.join(".ashrc");
        if let Ok(content) = std::fs::read_to_string(&rc) {
            return content.lines().any(|line| line.trim() == "set editing-mode vi");
        }
    }
    false
}

/// Build the execution Shell + history/completion/hint sources for M2/M3.
///
/// Mirrors the construction in `repl.rs::Repl::new` (lines 36-103, 263-277):
/// the Shell is the REAL execution shell (not a throwaway probe) — it has the
/// `TuiRenderHook` injected so structured commands (ls/ps) render as tables,
/// and the terminal-only commands (less/more/color) registered. The pager
/// hook is intentionally NOT injected (M3: `show --pager` falls back to
/// streamed highlighting; a ratatui-native pager is M4).
///
/// The `FileBackedHistory` points at `~/.auto-shell-history` (shared with the
/// reedline REPL). The hinter is only built if autosuggestion is on in
/// `ash.toml`.
fn build_shell_and_sources()
-> (
    auto_shell::Shell,
    reedline::FileBackedHistory,
    crate::completions_reedline::ShellCompleter,
    Option<crate::term::hinter::AshHinter>,
    std::sync::Arc<std::sync::Mutex<crate::completions_reedline::CompletionState>>,
    String,
)
{
    use auto_shell::completions::CompletionSignature;
    use auto_shell::completions::definitions;
    use ash_core::completions::CompletionProvider;
    use crate::completions_reedline::{CompletionState, ShellCompleter};
    use crate::term::hinter::AshHinter;

    let mut shell = auto_shell::Shell::new();
    // M3: inject the render hook (structured output → ANSI tables) + terminal
    // commands. NO pager hook (see doc comment).
    shell.set_render_hook(Box::new(crate::renderer::TuiRenderHook));
    shell.register_commands(crate::commands::terminal_commands());
    shell.load_env_persistence();

    // M4 Gap 1: Shell initialization — mirrors Repl::new() (repl.rs:48-79) so
    // the block TUI has the same ashrc/plugins/aliases as the reedline REPL.
    // Without this, user-defined functions, config aliases, and installed
    // plugins wouldn't work in --block-tui mode.
    let shell_config = auto_shell::config::AshShellConfig::load();
    for (name, value) in &shell_config.aliases {
        shell.set_alias(name, value);
    }
    if let Some(home) = dirs::home_dir() {
        let rc_path = home.join(".ashrc");
        if rc_path.exists() {
            let _ = shell.source_file(&rc_path);
        } else if let Ok(content) = std::str::from_utf8(auto_shell::DEFAULT_ASHRC.as_bytes()) {
            let _ = std::fs::write(&rc_path, content);
            let _ = shell.source_file(&rc_path);
        }
    }
    let plugin_report = auto_shell::plugin::load_all_plugins(&mut shell)
        .unwrap_or_else(|e| {
            eprintln!("  plugin load warning: {e}");
            auto_shell::plugin::PluginLoadReport::default()
        });
    plugin_report.print_to_stderr();

    let cwd = shell.pwd().to_string_lossy().to_string();

    // Completion signatures from the command registry.
    let completion_sigs: Vec<CompletionSignature> =
        shell.registry().params().into_iter().map(Into::into).collect();
    let mut provider = CompletionProvider::new();
    definitions::register_all(&mut provider);
    let completion_state = std::sync::Arc::new(std::sync::Mutex::new(CompletionState::new(
        shell.pwd().to_path_buf(),
    )));
    let completer = ShellCompleter::new(
        completion_sigs,
        provider,
        std::sync::Arc::clone(&completion_state),
    );

    // History file (shared with the reedline REPL).
    let history_path = dirs::home_dir()
        .map(|h| h.join(".auto-shell-history"))
        .unwrap_or_else(|| std::path::PathBuf::from(".auto-shell-history"));
    let history = reedline::FileBackedHistory::with_file(10000, history_path)
        .unwrap_or_else(|_| reedline::FileBackedHistory::with_file(10000, std::path::PathBuf::from("/dev/null")).unwrap());

    // Hinter (only if autosuggestion is on in config).
    let shell_config = auto_shell::config::AshShellConfig::load();
    let hinter: Option<AshHinter> = if shell_config.autosuggestion {
        let hint_style = nu_ansi_term::Style::new()
            .fg(nu_ansi_term::Color::DarkGray)
            .italic();
        Some(
            AshHinter::default()
                .with_style(hint_style)
                .with_min_chars(shell_config.autosuggestion_min_chars),
        )
    } else {
        None
    };

    (shell, history, completer, hinter, completion_state, cwd)
}

/// If the event is an ↑ or ↓ keypress (no modifiers), return the direction:
/// `true` = up (older), `false` = down (newer). `None` otherwise.
fn history_arrow_direction(event: &ratatui_crossterm::crossterm::event::Event) -> Option<bool> {
    use ratatui_crossterm::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    if let Event::Key(ke) = event {
        if matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            && ke.modifiers == KeyModifiers::NONE
        {
            return match ke.code {
                KeyCode::Up => Some(true),
                KeyCode::Down => Some(false),
                _ => None,
            };
        }
    }
    None
}

/// Handle an ↑/↓ history navigation. Returns true if consumed (the editor
/// buffer was updated). `older=true` for ↑, `false` for ↓.
///
/// On ↑: load the next older entry (or the most recent on first press) and
/// replace the editor buffer with it. On ↓: move toward the present; past the
/// newest entry, restore the saved input and exit navigation.
fn handle_history_arrow(
    editor: &mut Editor,
    nav: &mut crate::editor::history::HistoryNav,
    history: &reedline::FileBackedHistory,
    older: bool,
) -> bool {
    let entry = if older {
        nav.older(history, editor.text())
    } else {
        match nav.younger() {
            Some(e) => Some(e),
            None => {
                // Past newest — restore saved input.
                let saved = nav.saved_input().to_string();
                editor.replace_buffer(saved);
                return true;
            }
        }
    };
    match entry {
        Some(text) => {
            editor.replace_buffer(text.to_string());
            true
        }
        None => true, // consumed but nothing to show (empty history)
    }
}

// ── Terminal setup/teardown ─────────────────────────────────────────────

/// Enter raw mode for the inline viewport.
///
/// M3: the alternate screen is NO LONGER used. The whole point of
/// `Viewport::Inline` is to push finished blocks into the *host* scrollback
/// via `insert_before`; the alt screen hid those blocks (M0/M1 used it only
/// as a temporary crash-isolation measure). Dropping it also makes subprocess
/// handoff simpler (no Leave/EnterAlternateScreen dance).
fn setup_terminal() -> io::Result<()> {
    enable_raw_mode()?;
    Ok(())
}

/// Restore the terminal to its pre-raw-mode state. Idempotent — safe to call
/// from a panic hook even if setup never completed.
fn restore_terminal() {
    let _ = disable_raw_mode();
}

/// RAII guard: setup on creation, restore on drop. Guarantees terminal
/// restoration even when `BlockTui::run` returns early or propagates an error.
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        setup_terminal()?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_csi_sequences() {
        let s = "\x1b[32mgreen\x1b[0m plain \x1b[1;31mbold red\x1b[0m";
        assert_eq!(strip_ansi(s), "green plain bold red");
    }

    #[test]
    fn strip_ansi_handles_osc_sequences() {
        let s = "before\x1b]0;title\x07after";
        assert_eq!(strip_ansi(s), "beforeafter");
    }

    #[test]
    fn strip_ansi_preserves_plain_text() {
        assert_eq!(strip_ansi("hello world"), "hello world");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn strip_ansi_handles_empty_escape() {
        // A bare ESC followed by nothing useful should not panic.
        assert_eq!(strip_ansi("\x1b"), "");
    }

    #[test]
    fn strip_ansi_multiline_body() {
        // Simulates a structured command's ANSI table output.
        let s = "\x1b[1mNAME\x1b[0m  \x1b[1mSIZE\x1b[0m\nfoo  123\nbar  \x1b[36m456\x1b[0m";
        let stripped = strip_ansi(s);
        assert_eq!(stripped, "NAME  SIZE\nfoo  123\nbar  456");
    }
}
