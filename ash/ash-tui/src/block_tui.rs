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
    event,
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

        // M2/M3: build the execution Shell + history/completion/hint sources.
        // The Shell is the REAL execution shell (render hook + terminal
        // commands injected) — M3 uses it to run commands.
        let (mut shell, mut history_store, completer, hinter, cwd) = build_shell_and_sources();
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
                // Input line: prompt + typed text + dim hint ghost suffix.
                let mut input_spans = vec![
                    Span::styled("❯ ", Style::default().fg(Color::Green)),
                    Span::raw(&buf_text),
                ];
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
                frame.set_cursor_position((area.x + 2 + col_offset, area.y));
            })?;

            // ── Read + dispatch one event ───────────────────────────
            // Editor::handle_event takes the raw crossterm Event directly so
            // it can intercept arrow keys (which reedline's default keybindings
            // leave unbound → would be a no-op).
            let event = event::read()?;

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
                    let trimmed = line.trim().to_string();

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

                    // M3: interactive commands (vim/less/top/...) need full
                    // terminal control — tear down ratatui, run them with
                    // inherited stdio, then rebuild. Non-interactive commands
                    // go through Shell::execute and render as a block.
                    if ash_core::cmd::interactive::is_interactive_command(&trimmed) {
                        crate::subprocess::hand_off_to_interactive(
                            &mut terminal,
                            &trimmed,
                            &shell.pwd(),
                        )?;
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
                }
            }
        }

        Ok(())
    }
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
    let cwd = shell.pwd().to_string_lossy().to_string();

    // Completion signatures from the command registry.
    let completion_sigs: Vec<CompletionSignature> =
        shell.registry().params().into_iter().map(Into::into).collect();
    let mut provider = CompletionProvider::new();
    definitions::register_all(&mut provider);
    let completion_state = std::sync::Arc::new(std::sync::Mutex::new(CompletionState::new(
        shell.pwd().to_path_buf(),
    )));
    let completer = ShellCompleter::new(completion_sigs, provider, completion_state);

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

    (shell, history, completer, hinter, cwd)
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
