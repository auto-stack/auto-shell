//! Plan 070: the bottom-dynamic script editor modal.
//!
//! The "third layer" of the linear-dynamic CLI (auto-ai 029 model): output
//! stays in the native scrollback (linear archive), reedline keeps the inline
//! single-line tail (dynamic tail), and heavy interaction — multi-line
//! script editing — opens this rounded-border input box via a ratatui
//! [`Viewport::Inline`] on demand.
//!
//! - Enter inserts a newline; **Ctrl+Enter runs**; Esc exits the modal —
//!   with a non-empty buffer the content is dim-committed by the caller so
//!   nothing is lost (empty-buffer Esc is a plain exit).
//! - Single-shot: every outcome (Run/Cancelled/Exit) closes the modal; the
//!   F2 AutoScript lock returns to the normal inline mode afterwards.
//! - No alternate screen, no mouse capture — native copy/paste keeps working.
//! - Runs strictly BETWEEN two reedline `read_line` calls: reedline returns
//!   to cooked mode between reads (see block_tui.rs "Terminal ownership"), so
//!   terminal ownership never overlaps. The just-submitted inline input row
//!   is erased on entry so no stray `>` line stays above the box (wrapped
//!   multi-row inputs leave residue — accepted v1).

mod term;
mod view;

use ratatui_crossterm::crossterm::cursor;
use ratatui_crossterm::crossterm::event::{read, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui_crossterm::crossterm::execute;
use ratatui_crossterm::crossterm::terminal::{Clear, ClearType};
use ratatui_textarea::{CursorMove, TextArea};
use std::io;

/// Viewport height (box borders + editor rows). Fixed at creation — ratatui
/// Inline viewports cannot resize without rebuilding the Terminal (029 §5.1);
/// longer scripts scroll inside the textarea instead.
const VIEWPORT_HEIGHT: u16 = 12;

/// Key hints shown in the box's bottom border title.
const HINTS: &str = "Enter 换行 · Ctrl+Enter 运行 · Esc 取消退出";

/// What the caller should do after the modal closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorOutcome {
    /// Ctrl+Enter — run the buffer content.
    Run(String),
    /// Esc with a non-empty buffer — dim-commit the content (caller);
    /// nothing is lost and stays copyable.
    Cancelled(String),
    /// Esc (or Ctrl+C) on an empty buffer — plain exit.
    Exit,
}

/// Pure key routing for the modal loop (unit-testable without a terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyAction {
    /// Ctrl+Enter — submit the buffer.
    Submit,
    /// Esc / Ctrl+C — cancel if non-empty, exit if empty.
    Escape,
    /// Plain Enter — insert a newline.
    Newline,
    /// Everything else — hand to the textarea (movement, editing, undo…).
    Edit,
}

fn route_key(code: KeyCode, modifiers: KeyModifiers) -> KeyAction {
    if code == KeyCode::Enter && modifiers.contains(KeyModifiers::CONTROL) {
        KeyAction::Submit
    } else if code == KeyCode::Enter && modifiers.is_empty() {
        KeyAction::Newline
    } else if code == KeyCode::Esc
        || (code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL))
    {
        KeyAction::Escape
    } else {
        KeyAction::Edit
    }
}

/// Open the editor modal. `prefill` seeds the buffer (Ctrl+O passes the
/// current inline line); `mode_hint` labels the status line (e.g. `▌# AutoScript`).
///
/// On any terminal error the modal restores the terminal and reports via
/// stderr, returning `Exit` — the REPL must survive editor failures.
pub fn run_editor(prefill: &str, mode_hint: &str) -> EditorOutcome {
    match run_editor_inner(prefill, mode_hint) {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("  editor error: {e}");
            EditorOutcome::Exit
        }
    }
}

fn run_editor_inner(prefill: &str, mode_hint: &str) -> io::Result<EditorOutcome> {
    // Erase the just-submitted inline input row (the `>` line carrying only
    // the invisible mode marker) so the box doesn't leave a stray prompt
    // above it; the freed row is where the Inline viewport anchors. Nothing
    // may print between the reedline submit and here, or the wrong row gets
    // erased (see `apply_mode_switch`'s AutoScript suppression).
    let _ = execute!(
        io::stdout(),
        cursor::MoveUp(1),
        Clear(ClearType::CurrentLine),
        cursor::MoveToColumn(0),
    );

    let (guard, mut terminal) = term::TerminalGuard::enter(VIEWPORT_HEIGHT)?;

    let mut textarea = TextArea::from(prefill.lines().map(String::from).collect::<Vec<_>>());
    if !prefill.trim().is_empty() {
        textarea.move_cursor(CursorMove::End);
    }
    textarea.set_line_number_style(ratatui_core::style::Style::default().fg(
        ratatui_core::style::Color::DarkGray,
    ));

    let outcome = loop {
        let chunk = view::draw(&mut terminal, &mut textarea, mode_hint, HINTS)?;
        // Hardware cursor on the textarea's cursor cell — the IME anchor
        // (029 §2.4 manual equivalent of pi's CURSOR_MARKER).
        let sc = textarea.screen_cursor();
        let _ = execute!(
            io::stdout(),
            cursor::MoveTo(chunk.x + sc.col as u16, chunk.y + sc.row as u16),
            cursor::Show
        );

        let Event::Key(key) = read()? else {
            continue;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        match route_key(key.code, key.modifiers) {
            KeyAction::Submit => {
                let text = textarea.lines().join("\n");
                break EditorOutcome::Run(text);
            }
            KeyAction::Escape => {
                let text = textarea.lines().join("\n");
                break if text.trim().is_empty() {
                    EditorOutcome::Exit
                } else {
                    EditorOutcome::Cancelled(text)
                };
            }
            KeyAction::Newline => textarea.insert_newline(),
            KeyAction::Edit => {
                textarea.input(key);
            }
        }
    };

    term::exit_modal(&mut terminal);
    drop(guard);
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_enter_submits() {
        assert_eq!(
            route_key(KeyCode::Enter, KeyModifiers::CONTROL),
            KeyAction::Submit
        );
    }

    #[test]
    fn plain_enter_is_newline() {
        assert_eq!(route_key(KeyCode::Enter, KeyModifiers::NONE), KeyAction::Newline);
    }

    #[test]
    fn esc_and_ctrl_c_escape() {
        assert_eq!(route_key(KeyCode::Esc, KeyModifiers::NONE), KeyAction::Escape);
        assert_eq!(
            route_key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            KeyAction::Escape
        );
    }

    #[test]
    fn everything_else_edits() {
        assert_eq!(
            route_key(KeyCode::Char('a'), KeyModifiers::NONE),
            KeyAction::Edit
        );
        assert_eq!(route_key(KeyCode::Left, KeyModifiers::NONE), KeyAction::Edit);
        assert_eq!(route_key(KeyCode::F(1), KeyModifiers::NONE), KeyAction::Edit);
        // Shift/Alt+Enter stays an edit (textarea's own newline bindings).
        assert_eq!(
            route_key(KeyCode::Enter, KeyModifiers::SHIFT),
            KeyAction::Edit
        );
    }
}
