//! Block TUI — ratatui inline-viewport REPL skeleton (Plan 038 M0).
//!
//! This is the **experimental** ratatui-owned terminal path, the counterpart to
//! the reedline-driven [`crate::Repl`]. Plan 038 explores replacing reedline's
//! hold on the terminal with a ratatui `Viewport::Inline`, so that finished
//! commands can be pushed into the host scrollback as blocks while a fixed
//! bottom editor stays visible (something reedline 0.44.0 cannot do).
//!
//! ## M0 scope
//! M0 is deliberately minimal — it only proves the skeleton:
//!   - a ratatui `Viewport::Inline(3)` anchored at the bottom,
//!   - a crossterm event loop (no editing yet; it just echoes pressed keys),
//!   - `Terminal::insert_before` pushing a one-line "block" into the scrollback
//!     on each keypress.
//!
//! Everything beyond this (editor, history, completion, block rendering,
//! subprocess handoff, orchestration) lands in M1-M4. See
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
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui_core::layout::Rect;
use ratatui_core::terminal::{Terminal, TerminalOptions, Viewport};
use ratatui_core::text::{Line, Span};
use ratatui_core::style::{Color, Style};
use ratatui_crossterm::CrosstermBackend;

/// The fixed height of the inline viewport (in rows). M0 uses 3: one border
/// row, one input row, one status row. Later milestones grow this to fit the
/// completion menu / hints.
const VIEWPORT_HEIGHT: u16 = 3;

/// The block-TUI experimental REPL. Construct with [`BlockTui::new`], run with
/// [`BlockTui::run`]. Returns when the user exits (Ctrl+D / Ctrl+C / `q`).
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

        // RAII guard: no matter how we leave this function (early return,
        // error, panic), the terminal is restored. `drop` is the only method.
        // Setup happens here (not in the panic hook); the hook is a
        // best-effort belt-and-suspenders for the case where a panic unwinds
        // *before* this guard's drop runs.
        let _guard = TerminalGuard::new()?;

        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(stdout()),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT_HEIGHT),
            },
        )?;

        let mut key_count: u64 = 0;
        loop {
            // Draw the fixed-bottom viewport: a label line + the last key seen.
            terminal.draw(|frame| {
                let area: Rect = frame.area();
                let label = Line::from(vec![
                    Span::styled(
                        "block-tui M0 ",
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw("— press keys (pushed to scrollback); "),
                    Span::styled(
                        "Ctrl+D / Ctrl+C / q",
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(" to exit"),
                ]);
                let status = Line::from(format!("  keys seen: {key_count}"));
                frame.render_widget(label, Rect::new(area.x, area.y, area.width, 1));
                frame.render_widget(status, Rect::new(area.x, area.y + 1, area.width, 1));
            })?;

            // Block on the next crossterm event. M0 only reacts to keys; M1
            // will route them through EditMode::parse_event.
            let Event::Key(KeyEvent { code, modifiers, .. }) = event::read()? else {
                continue;
            };

            // Exit conditions.
            if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                break;
            }
            if code == KeyCode::Char('d') && modifiers.contains(KeyModifiers::CONTROL) {
                break;
            }
            if code == KeyCode::Char('q') {
                break;
            }

            key_count += 1;
            // Push a one-line "block" into the host scrollback above the
            // viewport. This is the Plan 038 §2.1 primitive — M3 will replace
            // this test line with the real command header + rendered output.
            terminal.insert_before(1, |buf| {
                let line = Line::from(vec![
                    Span::styled("❯ ", Style::default().fg(Color::Green)),
                    Span::raw(format!("key #{key_count}: ")),
                    Span::styled(
                        key_label(code, modifiers),
                        Style::default().fg(Color::Blue),
                    ),
                ]);
                // set_line renders a `Line` into the buffer at (x, y), which is
                // exactly what we need for a single-row insert. (Line does
                // implement Widget, but set_line avoids needing the Widget
                // trait import and is the idiomatic buffer-level call here.)
                buf.set_line(buf.area.x, buf.area.y, &line, buf.area.width);
            })?;
        }

        Ok(())
    }
}

/// Render a friendly label for a keypress (for the M0 echo line).
fn key_label(code: KeyCode, modifiers: KeyModifiers) -> String {
    let mods = if modifiers.contains(KeyModifiers::CONTROL) {
        "Ctrl+"
    } else if modifiers.contains(KeyModifiers::ALT) {
        "Alt+"
    } else {
        ""
    };
    match code {
        KeyCode::Char(c) => format!("{mods}{c:?}"),
        KeyCode::Enter => format!("{mods}Enter"),
        KeyCode::Backspace => format!("{mods}Backspace"),
        KeyCode::Left => format!("{mods}Left"),
        KeyCode::Right => format!("{mods}Right"),
        KeyCode::Up => format!("{mods}Up"),
        KeyCode::Down => format!("{mods}Down"),
        other => format!("{mods}{other:?}"),
    }
}

// ── Terminal setup/teardown ─────────────────────────────────────────────

/// Enter raw mode + the alternate screen. Paired with [`restore_terminal`].
///
/// Note: M0 uses the alternate screen for isolation during the experiment so
/// that a crash cannot corrupt the host scrollback. Later milestones (M3) will
/// drop the alt screen — the whole point of `Viewport::Inline` is to push
/// blocks into the *host* scrollback, which the alt screen hides. For M0's
/// "does inline work at all?" question, isolation is safer.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_label_plain_char() {
        let l = key_label(KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(l, "'a'");
    }

    #[test]
    fn key_label_ctrl_enter() {
        let l = key_label(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(l, "Ctrl+Enter");
    }

    #[test]
    fn key_label_alt_up() {
        let l = key_label(KeyCode::Up, KeyModifiers::ALT);
        assert_eq!(l, "Alt+Up");
    }
}
