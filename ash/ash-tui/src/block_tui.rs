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
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
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

/// The fixed height of the inline viewport (rows). M1 uses 3: one input row
/// (prompt + buffer), one status row, one margin. M2 will grow this to fit the
/// completion menu / hints.
const VIEWPORT_HEIGHT: u16 = 3;

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

        loop {
            // ── Draw the fixed-bottom viewport ──────────────────────
            // Row 0: prompt indicator + the editor's current buffer.
            // Row 1: a status line (M1 hint text; M2 will show hints/menu).
            // The cursor is positioned within this same draw call via
            // `frame.set_cursor_position`, so ratatui applies it after the
            // buffer diff — no separate cursor write needed.
            let ip = editor.insertion_point();
            let buf_text = editor.text().to_string();
            terminal.draw(|frame| {
                let area: Rect = frame.area();
                let input_line = Line::from(vec![
                    Span::styled("❯ ", Style::default().fg(Color::Green)),
                    Span::raw(&buf_text),
                ]);
                let status = Line::from(vec![
                    Span::styled(
                        "block-tui M1 ",
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        "Enter=submit  Ctrl+D=exit  (emacs: C-a/e/k/w  M-b/f)",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                frame.render_widget(input_line, Rect::new(area.x, area.y, area.width, 1));
                frame.render_widget(status, Rect::new(area.x, area.y + 1, area.width, 1));

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
            let outcome = editor.handle_event(event);

            match outcome {
                EditorOutcome::Continue => continue,
                EditorOutcome::Exit => break,
                EditorOutcome::Submitted => {
                    let line = editor.take_line();
                    // M1: recognize `exit`/`quit` to leave the loop. Real
                    // command dispatch + block rendering is M3.
                    let trimmed = line.trim();
                    if trimmed == "exit" || trimmed == "quit" || trimmed == "q" {
                        break;
                    }
                    // Push a one-line "block" into the host scrollback above
                    // the viewport. M3 replaces this with the real command
                    // header + rendered output.
                    let display = if line.is_empty() {
                        "(empty)".to_string()
                    } else {
                        line.clone()
                    };
                    terminal.insert_before(1, |buf| {
                        let rendered = Line::from(vec![
                            Span::styled("❯ ", Style::default().fg(Color::Green)),
                            Span::styled(display, Style::default().add_modifier(Modifier::BOLD)),
                        ]);
                        buf.set_line(buf.area.x, buf.area.y, &rendered, buf.area.width);
                    })?;
                }
            }
        }

        Ok(())
    }
}

/// Build the reedline `EditMode` (keybinding parser).
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

// ── Terminal setup/teardown ─────────────────────────────────────────────

/// Enter raw mode + the alternate screen. Paired with [`restore_terminal`].
///
/// Note: M0/M1 use the alternate screen for isolation during the experiment so
/// that a crash cannot corrupt the host scrollback. M3 will drop the alt
/// screen — the whole point of `Viewport::Inline` is to push blocks into the
/// *host* scrollback, which the alt screen hides.
fn setup_terminal() -> io::Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    Ok(())
}

/// Restore the terminal to its pre-raw-mode state. Idempotent — safe to call
/// from a panic hook even if setup never completed.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = stdout().execute(LeaveAlternateScreen);
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
