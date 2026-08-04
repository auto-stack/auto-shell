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

        // M2/M3: build the execution Shell + history/completion/hint sources.
        // The Shell is the REAL execution shell (render hook + terminal
        // commands injected) — M3 uses it to run commands.
        let (mut shell, mut history_store, completer, hinter, completion_state, cwd) =
            build_shell_and_sources();
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
                let (prompt_spans, prompt_width) = prompt_spans(&mode_state);
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

            // M4-6: F3 / Alt+3 — AI NL→command (translate natural language to
            // an ash command, then offer execute/edit/cancel). Synchronous flow
            // (blocks the event loop while the AI thinks, same as reedline REPL).
            if is_ai_suggest_key(&event) {
                handle_ai_suggest(&mut editor, &mut shell, &mut terminal)?;
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

                    // ── Execute + time (mirrors repl.rs execute_with_header) ──
                    let start = std::time::Instant::now();
                    let result = shell.execute(&trimmed);
                    let elapsed = start.elapsed();
                    let exit_code = match &result {
                        Ok(_) => shell.last_exit_code(),
                        Err(_) => {
                            let c = shell.last_exit_code();
                            if c != 0 { c } else { 1 }
                        }
                    };

                    // ── Build the block body ──
                    // Shell::execute returns Option<String>:
                    //   Some(s) — the command's text output. For structured
                    //     commands (ls/ps) this is ALREADY ANSI-styled via the
                    //     TuiRenderHook. ratatui can't parse ANSI, so we strip
                    //     escapes for the M3 body (tables render as plain text
                    //     for now; a ratatui-native table widget is a follow-up).
                    //   None — no output.
                    let (body_text, is_error): (Option<String>, bool) = match result {
                        Ok(Some(s)) => (Some(s), false),
                        Ok(None) => (None, false),
                        Err(e) => (Some(format!("Error: {e}")), true),
                    };

                    // Capture a snippet for suggest-next BEFORE body_text is
                    // consumed by the map below (first 200 chars, like repl.rs).
                    let output_snippet: String = body_text
                        .as_deref()
                        .map(|s| s.chars().take(200).collect())
                        .unwrap_or_default();

                    // Split body into lines (strip ANSI so it doesn't render as
                    // literal escape gibberish in the ratatui buffer).
                    let body_lines: Vec<String> = body_text
                        .map(|s| strip_ansi(&s).lines().map(|l| l.to_string()).collect())
                        .unwrap_or_default();

                    // ── insert_before: header (1 row) + body (N rows) ──
                    let height = 1u16 + body_lines.len() as u16;
                    let header_cmd = trimmed.clone();
                    terminal.insert_before(height, move |buf| {
                        render_block(buf, &header_cmd, exit_code, elapsed, &body_lines, is_error);
                    })?;

                    // M4: async-suggest next command (opt-in, never blocks).
                    // Fires a background fetch; the result shows before the next
                    // prompt if it arrived in time.
                    if auto_shell::ai::suggest::is_enabled() {
                        auto_shell::ai::suggest::suggest_next_async(
                            shell.pwd().to_string_lossy().to_string(),
                            trimmed.clone(),
                            output_snippet,
                        );
                    }

                    // M4 Gap 1: sync completion state (cwd may have changed
                    // after cd/pushd; last-command/exit-code/aliases update
                    // context-aware ranking). Mirrors repl.rs:1004-1005.
                    if let Ok(mut state) = completion_state.lock() {
                        state.current_dir = shell.pwd().to_path_buf();
                        state.last_command = shell.last_command_line().map(String::from);
                        state.last_exit_code = Some(shell.last_exit_code());
                        state.aliases = shell.aliases().clone();
                    }
                }
            }
        }

        Ok(())
    }
}

/// Ask the AI to translate a natural-language question into a single ash
/// command. Ported from `repl.rs::Repl::ask_ai` (line 388) as a free function
/// (the original was `&self`, using `self.shell`; here we pass `&Shell`).
///
/// Returns the suggested command string, or an error message. Uses `tier:mid`
/// (the daemon resolves it to a concrete model). Must be called in a sync
/// context (it builds a blocking tokio runtime internally).
fn ask_ai(shell: &auto_shell::Shell, question: &str) -> Result<String, String> {
    use auto_ai_client::{AiClient, CompletionRequest};

    let context = auto_shell::ai::context::build_context_block(shell);
    let system = format!(
        "You are an AI assistant for Ash (AutoShell), a shell similar to bash/fish.\n\
         {context}\n\
         The user will describe what they want to do in natural language.\n\
         Translate it into a SINGLE ash shell command (or pipeline).\n\
         Rules:\n\
         - Respond with ONLY the command, no explanation, no markdown.\n\
         - Use standard Unix commands (ls, grep, find, etc.).\n\
         - For Ash-specific features, use: ls | .size > 10.mb | sort .name\n\
         - If multiple steps are needed, use && to chain them.\n\
         - If you're unsure, give your best single-command guess."
    );

    let client = AiClient::new().map_err(|e| format!("AI client init: {}", e))?;
    let model = "tier:mid".to_string();
    let req = CompletionRequest::single(&model, question)
        .with_system(&system)
        .with_max_tokens(256)
        .with_temperature(0.3);

    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("runtime: {}", e))?;
    let response = rt.block_on(async { client.complete(&req).await });

    match response {
        Ok(resp) if resp.is_ok() => {
            let cmd = resp.content.trim().to_string();
            let cmd = cmd
                .trim_start_matches("```bash\n")
                .trim_start_matches("```sh\n")
                .trim_start_matches("```\n")
                .trim_end_matches("\n```")
                .trim()
                .to_string();
            Ok(cmd)
        }
        Ok(resp) => Err(format!("AI returned error: {:?}", resp.error)),
        Err(e) => Err(format!("{e}")),
    }
}

/// Is the event F3 or Alt+3 (AI NL→command)?
fn is_ai_suggest_key(event: &ratatui_crossterm::crossterm::event::Event) -> bool {
    use ratatui_crossterm::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    if let Event::Key(ke) = event {
        matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            && (ke.code == KeyCode::F(3)
                || (ke.code == KeyCode::Char('3') && ke.modifiers.contains(KeyModifiers::ALT)))
    } else {
        false
    }
}

/// Is the event F4 or Alt+4 (AI chat)?
fn is_ai_chat_key(event: &ratatui_crossterm::crossterm::event::Event) -> bool {
    use ratatui_crossterm::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    if let Event::Key(ke) = event {
        matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            && (ke.code == KeyCode::F(4)
                || (ke.code == KeyCode::Char('4') && ke.modifiers.contains(KeyModifiers::ALT)))
    } else {
        false
    }
}

/// F3: translate the current input (or a prompted question) into an ash
/// command via the AI, then offer execute / step-by-step / edit / cancel.
///
/// Synchronous — blocks the event loop while the AI thinks. The viewport is
/// not redrawn during the AI call (the terminal shows the last frame). The
/// result + decision prompt are pushed as blocks into the scrollback.
fn handle_ai_suggest(
    editor: &mut Editor,
    shell: &mut auto_shell::Shell,
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
) -> io::Result<()> {
    // The question: use the current editor text, or prompt for one.
    let question = {
        let text = editor.text().trim().to_string();
        if text.is_empty() {
            // Read a question line directly (no editor — simple blocking read).
            push_block(terminal, &["AI: type your question, then Enter:", ""], Color::Cyan)?;
            let mut buf = String::new();
            read_raw_line(terminal, &mut buf)?;
            if buf.trim().is_empty() {
                return Ok(()); // cancelled
            }
            buf.trim().to_string()
        } else {
            editor.take_line();
            text
        }
    };

    // Ask the AI (blocking).
    push_block(terminal, &["  ⏳ asking AI..."], Color::DarkGray)?;
    let suggestion = match ask_ai(shell, &question) {
        Ok(cmd) => cmd,
        Err(e) => {
            push_block(
                terminal,
                &[&format!("  AI error: {e}"), "  (set ZHIPU_API_KEY / start aaid daemon)"],
                Color::Red,
            )?;
            return Ok(());
        }
    };

    // Validate (danger/warning detection).
    let findings = auto_shell::ai::validate_suggestion(&suggestion);
    let steps = auto_shell::ai::split_steps(&suggestion);
    let multi = steps.len() > 1;

    let mut lines: Vec<String> = vec![format!("  AI: {suggestion}")];
    for f in &findings {
        match f {
            auto_shell::ai::ValidationFinding::Danger(msg) => {
                lines.push(format!("  ⚠ DANGER: {msg}"));
            }
            auto_shell::ai::ValidationFinding::Warning(msg) => {
                lines.push(format!("  ⚠ warning: {msg}"));
            }
        }
    }
    if multi {
        lines.push(format!(
            "  [Enter] 全部执行  [s] 分步执行({}步)  [e] 编辑  [Esc/其他] 取消",
            steps.len()
        ));
    } else {
        lines.push("  [Enter] 执行  [e] 编辑  [Esc/其他] 取消".to_string());
    }
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    push_block(terminal, &refs, Color::Cyan)?;

    // Read the decision key.
    let event = event::read()?;
    if is_enter(&event) {
        execute_and_render(shell, terminal, &suggestion)?;
    } else if let Some(KeyCode::Char(c)) = key_code(&event) {
        match c {
            's' if multi => {
                for (i, step) in steps.iter().enumerate() {
                    push_block(
                        terminal,
                        &[&format!("  [{}/{}] {}", i + 1, steps.len(), step)],
                        Color::DarkGray,
                    )?;
                    let ev = event::read()?;
                    if !is_enter(&ev) {
                        push_block(
                            terminal,
                            &[&format!("  已中止 (剩余 {} 步)", steps.len() - i - 1)],
                            Color::DarkGray,
                        )?;
                        return Ok(());
                    }
                    execute_and_render(shell, terminal, step)?;
                    if shell.last_exit_code() != 0 {
                        return Ok(()); // abort on failure (&& semantics)
                    }
                }
            }
            'e' => {
                push_block(
                    terminal,
                    &[&format!("  编辑命令 (当前: {suggestion})")],
                    Color::DarkGray,
                )?;
                let mut edited = String::new();
                read_raw_line(terminal, &mut edited)?;
                if !edited.trim().is_empty() {
                    execute_and_render(shell, terminal, edited.trim())?;
                }
            }
            _ => {} // cancel
        }
    }
    Ok(())
}

/// F4: persistent AI chat (ReAct loop with tool use). Blocks per turn.
fn handle_ai_chat(
    shell: &mut auto_shell::Shell,
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
) -> io::Result<()> {
    // Lazy-load the chat session (must be sync context — daemon probe).
    let mut session = match auto_shell::ai::ChatSession::load() {
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
    if turns > 0 {
        push_block(terminal, &[&format!("  * 已恢复 {} 轮对话 *", turns / 2)], Color::Cyan)?;
    } else {
        push_block(
            terminal,
            &["  * 开始新对话 *  (/clear 清空  /exit 退出  Esc/F4 离开)"],
            Color::Cyan,
        )?;
    }

    loop {
        // Read a chat line (simple blocking read, no editor).
        let mut input = String::new();
        push_block(terminal, &["▌? "], Color::Magenta)?;
        read_raw_line(terminal, &mut input)?;
        let line = input.trim().to_string();

        // Ctrl+D (empty EOF) → exit chat.
        if line.is_empty() {
            let _ = session.save();
            break;
        }

        // Slash commands.
        if let Some(cmd) = auto_shell::ai::parse_slash_command(&line) {
            match cmd {
                auto_shell::ai::SlashCommand::Exit => {
                    let _ = session.save();
                    break;
                }
                auto_shell::ai::SlashCommand::Clear => {
                    session.clear();
                    let _ = session.save();
                    push_block(terminal, &["  * 对话已清空 *"], Color::DarkGray)?;
                    continue;
                }
            }
        }
        if line.starts_with('/') {
            push_block(terminal, &[&format!("  未知命令: {line} (可用: /clear /exit)")], Color::DarkGray)?;
            continue;
        }

        // Send a turn (blocking — the streamed events are captured into a
        // Vec and pushed as a block after the turn completes; we can't draw
        // during the sync block_on).
        session.update_context(shell);
        let captured: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let cap = captured.clone();
        let on_event: std::sync::Arc<dyn Fn(auto_ai_agent::agent::StreamEvent) + Send + Sync> =
            std::sync::Arc::new(move |ev| {
                let line = match ev {
                    auto_ai_agent::agent::StreamEvent::Delta { text } => {
                        // Accumulate delta text; we'll push it as a block later.
                        use std::io::Write;
                        let _ = std::io::stdout().write_all(text.as_bytes());
                        let _ = std::io::stdout().flush();
                        None
                    }
                    auto_ai_agent::agent::StreamEvent::ToolStart { tool, args } => Some(format!(
                        "\n  ⚙ {tool} {}",
                        auto_shell::ai::brief::brief_args(&args)
                    )),
                    auto_ai_agent::agent::StreamEvent::Tool { tool, result, .. } => Some(format!(
                        "\n  ← {tool}: {}",
                        auto_shell::ai::brief::brief_result(&result)
                    )),
                    auto_ai_agent::agent::StreamEvent::Warning { text } => {
                        Some(format!("\n  ⚠ {text}"))
                    }
                    auto_ai_agent::agent::StreamEvent::Error { message } => {
                        Some(format!("\n  [error] {message}"))
                    }
                    auto_ai_agent::agent::StreamEvent::Cancelled { .. } => {
                        Some("\n  [cancelled]".to_string())
                    }
                    auto_ai_agent::agent::StreamEvent::Done { .. } => None,
                };
                if let Some(l) = line {
                    cap.lock().unwrap().push(l);
                }
            });
        let result = auto_shell::ai::block_on_async(session.send_turn_streaming(&line, on_event));
        // Print a newline after the streamed delta (it was written directly to
        // stdout via the callback, bypassing ratatui — this works because we're
        // NOT in raw mode's owned-terminal during... actually we ARE. The
        // stream writes go to stdout which ratatui owns. This is a known M4
        // limitation: the chat stream output is raw stdout, which may interleave
        // badly with the ratatui viewport. A proper fix needs a background
        // thread + channel + draw rendering (deferred). For now the direct-write
        // approach works on most terminals when the viewport is at the bottom.)
        match result {
            Ok(_) => {
                let _ = session.save();
                // Push captured tool/error events as a block.
                let events = captured.lock().unwrap();
                if !events.is_empty() {
                    let refs: Vec<&str> = events.iter().map(|s| s.as_str()).collect();
                    push_block(terminal, &refs, Color::DarkGray)?;
                }
                // Ensure a newline after the streamed reply.
                push_block(terminal, &[""], Color::Reset)?;
            }
            Err(e) => {
                push_block(
                    terminal,
                    &[&format!("  AI error: {e}"), "  (check API key / daemon)"],
                    Color::Red,
                )?;
            }
        }
    }
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

/// Execute a command and render it as a block (the M3 path, extracted as a
/// helper so F3's decision flow can reuse it without duplicating logic).
fn execute_and_render(
    shell: &mut auto_shell::Shell,
    terminal: &mut Terminal<ratatui_crossterm::CrosstermBackend<std::io::Stdout>>,
    command: &str,
) -> io::Result<()> {
    let start = std::time::Instant::now();
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
fn prompt_spans(mode_state: &auto_shell::repl_mode::ModeState) -> (Vec<Span<'static>>, u16) {
    use unicode_width::UnicodeWidthStr;
    let symbol = mode_state.prompt();
    let mut spans: Vec<Span<'static>> = Vec::new();
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
    let total = UnicodeWidthStr::width(prefix) as u16 + UnicodeWidthStr::width(main) as u16 + 1;
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
    use ratatui_core::style::{Color, Modifier, Style};
    use ratatui_core::text::{Line, Span};
    use unicode_width::UnicodeWidthStr;

    let w = buf.area.width;
    // ── Header: "❯ {command}  ...pad...  {duration}  {icon}" ──
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
