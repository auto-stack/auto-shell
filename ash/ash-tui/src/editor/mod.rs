//! Editor — the bottom-line input editor for the block TUI (Plan 038 M1).
//!
//! Holds a reedline [`LineBuffer`] (grapheme/word-aware buffer) + a reedline
//! [`EditMode`] (Emacs or Vi keybinding parser). Each crossterm key event is
//! routed through `EditMode::parse_event` → `ReedlineEvent` → this module's
//! dispatch, which mutates the `LineBuffer`.
//!
//! ## M1 scope (deliberate minimalism)
//! reedline's `Editor::run_edit_command` dispatches **88** `EditCommand`
//! variants with heavy selection + undo machinery, all of it `pub(crate)`.
//! M1 implements only the **9 variants** needed for "type + move + basic
//! edit" (see [`dispatch`]), and skips selection / undo entirely:
//!
//! | Capability | M1 | Why |
//! |---|---|---|
//! | selection (`selection_anchor`/`get_selection`) | ❌ | coupled to Vi inclusive mode; M2 |
//! | undo (`EditStack` + `UndoBehavior`) | ❌ | ~150 lines; `Undo`/`Redo` no-op in M1 |
//! | vi text objects / pair matching | ❌ | needs 8 `pub(crate)` LineBuffer methods; M2+ |
//! | system clipboard | ❌ | `LocalClipboard` is `pub(crate)`; M1 uses `String` |
//!
//! All `Move*{select}` variants ignore the `select` flag (treat as `false`),
//! which is equivalent to "plain cursor movement" and needs no selection
//! state. `Undo`/`Redo`/`SelectAll`/`CutSelection`/`CopySelection` fall
//! through to the `_ => {}` arm (no key binds them in M1).
//!
//! See `docs/plans/038-block-tui-migration.md` §2.4 and the M1 research notes.

pub mod dispatch;

use reedline::{EditMode, LineBuffer};

/// The bottom-line input editor. Owns the editable buffer + keybinding parser.
pub struct Editor {
    /// The editable text buffer (grapheme/word-aware). Public reedline type.
    pub(crate) line_buffer: LineBuffer,
    /// Emacs or Vi keybinding parser. Translates raw key events into
    /// `ReedlineEvent`s that [`dispatch`] consumes.
    pub(crate) edit_mode: Box<dyn EditMode>,
    /// The kill-ring / cut buffer. reedline's `LocalClipboard` is
    /// `pub(crate)`-gated, so M1 uses a plain `String` (set by `CutWordLeft`
    /// / `KillLine`; not yet yanked back by a paste command — that's M2).
    pub(crate) cut_buffer: String,
}

/// What the editor reports back to the event loop after handling one
/// `ReedlineEvent`. Drives the outer block-TUI loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorOutcome {
    /// Keep editing — redraw the editor. The buffer may have changed.
    Continue,
    /// The user submitted the current line (Enter on a complete line).
    /// The caller takes the buffer via [`Editor::take_line`].
    Submitted,
    /// The user requested an exit (Ctrl+D on empty, or Ctrl+C).
    Exit,
}

impl Editor {
    /// Build an editor with the given keybinding mode.
    pub fn new(edit_mode: Box<dyn EditMode>) -> Self {
        Self {
            line_buffer: LineBuffer::new(),
            edit_mode,
            cut_buffer: String::new(),
        }
    }

    /// The current input text.
    pub fn text(&self) -> &str {
        self.line_buffer.get_buffer()
    }

    /// Take the submitted line, leaving the editor empty for the next prompt.
    pub fn take_line(&mut self) -> String {
        let text = self.line_buffer.get_buffer().to_string();
        self.line_buffer = LineBuffer::new();
        self.cut_buffer.clear();
        text
    }

    /// The byte offset of the cursor in the buffer (for rendering the caret).
    pub fn insertion_point(&self) -> usize {
        self.line_buffer.insertion_point()
    }

    /// Feed one raw crossterm event through the keybinding parser + dispatch.
    /// Returns the outcome for the outer loop.
    ///
    /// `raw_event` ownership is transferred into `parse_event` (reedline
    /// consumes it by value). Returns `Continue` for events that don't map to
    /// a recognized key (mouse/focus/unknown) — the editor is unchanged.
    pub fn handle_raw(
        &mut self,
        raw_event: reedline::ReedlineRawEvent,
    ) -> EditorOutcome {
        let event = self.edit_mode.parse_event(raw_event);
        self.dispatch_reedline_event(event)
    }

    /// Recursively dispatch a `ReedlineEvent`. `Multiple` is unwrapped so that
    /// composite bindings (e.g. F1 = `[InsertString(prefix), Submit]`) work.
    fn dispatch_reedline_event(&mut self, event: reedline::ReedlineEvent) -> EditorOutcome {
        use reedline::ReedlineEvent;
        match event {
            ReedlineEvent::None | ReedlineEvent::Repaint | ReedlineEvent::Mouse => {
                EditorOutcome::Continue
            }
            ReedlineEvent::Edit(cmds) => {
                for cmd in cmds {
                    dispatch::dispatch_edit_command(self, cmd);
                }
                EditorOutcome::Continue
            }
            // Composite bindings (F1-F4/Esc/Alt in the reedline REPL all use
            // Multiple). Recurse so each sub-event is handled in turn.
            ReedlineEvent::Multiple(inner) => {
                let mut outcome = EditorOutcome::Continue;
                for sub in inner {
                    outcome = self.dispatch_reedline_event(sub);
                    // Stop early on a terminal outcome (Exit/Submitted).
                    if outcome != EditorOutcome::Continue {
                        break;
                    }
                }
                outcome
            }
            // UntilFound: M1 treats it as "try each in order, first non-None
            // wins." Since M1 has no menu/hint fallbacks yet, this is
            // equivalent to expanding Multiple for our purposes.
            ReedlineEvent::UntilFound(inner) => {
                for sub in inner {
                    let outcome = self.dispatch_reedline_event(sub);
                    if outcome != EditorOutcome::Continue {
                        return outcome;
                    }
                }
                EditorOutcome::Continue
            }
            // Submit family — M1 treats Enter/Submit/SubmitOrNewline as submit.
            // (Multi-line input continuation is M4 orchestration; for M1 a
            // newline just submits the single line.)
            ReedlineEvent::Submit | ReedlineEvent::Enter | ReedlineEvent::SubmitOrNewline => {
                EditorOutcome::Submitted
            }
            ReedlineEvent::CtrlD => {
                // Match reedline: Ctrl+D on empty line exits; on non-empty
                // deletes the char to the right (like Delete).
                if self.line_buffer.get_buffer().is_empty() {
                    EditorOutcome::Exit
                } else {
                    dispatch::dispatch_edit_command(self, reedline::EditCommand::Delete);
                    EditorOutcome::Continue
                }
            }
            ReedlineEvent::CtrlC => {
                // Abort: clear the line and stay (like reedline's Signal).
                // M1 does not propagate a Ctrl+C *exit* — that's the outer
                // loop's job via its own Ctrl+C handling. Here we just clear.
                self.line_buffer = LineBuffer::new();
                EditorOutcome::Continue
            }
            ReedlineEvent::Esc => {
                // M1: Esc is a no-op at the editor level (Vi mode switching is
                // handled inside `EditMode::parse_event`, which emits the
                // relevant Edit/Move commands). The orchestration-level Esc
                // (unlock mode) is M4.
                EditorOutcome::Continue
            }
            ReedlineEvent::Resize(_, _) => {
                // ratatui's Terminal::draw handles resize on the next frame;
                // nothing for the editor to do.
                EditorOutcome::Continue
            }
            ReedlineEvent::ClearScreen | ReedlineEvent::ClearScrollback => {
                // M3/M4 territory (clearing the ratatui viewport). No-op for M1.
                EditorOutcome::Continue
            }
            // Everything below is M2+ (menu/history/hints/host). No-op in M1.
            ReedlineEvent::Menu(_)
            | ReedlineEvent::MenuNext
            | ReedlineEvent::MenuPrevious
            | ReedlineEvent::MenuUp
            | ReedlineEvent::MenuDown
            | ReedlineEvent::MenuLeft
            | ReedlineEvent::MenuRight
            | ReedlineEvent::MenuPageNext
            | ReedlineEvent::MenuPagePrevious
            | ReedlineEvent::HistoryHintComplete
            | ReedlineEvent::HistoryHintWordComplete
            | ReedlineEvent::PreviousHistory
            | ReedlineEvent::NextHistory
            | ReedlineEvent::SearchHistory
            | ReedlineEvent::Up
            | ReedlineEvent::Down
            | ReedlineEvent::Left
            | ReedlineEvent::Right
            | ReedlineEvent::OpenEditor
            | ReedlineEvent::ExecuteHostCommand(_)
            | ReedlineEvent::ViChangeMode(_) => EditorOutcome::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reedline::{default_emacs_keybindings, Emacs};

    fn emacs_editor() -> Editor {
        Editor::new(Box::new(Emacs::new(default_emacs_keybindings())))
    }

    /// Helper: feed a crossterm char-press into the editor.
    fn type_char(editor: &mut Editor, c: char) {
        use reedline::ReedlineRawEvent;
        let ev = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
        let raw = ReedlineRawEvent::try_from(ev).unwrap();
        editor.handle_raw(raw);
    }

    #[test]
    fn typing_builds_up_text() {
        let mut e = emacs_editor();
        for c in "hello".chars() {
            type_char(&mut e, c);
        }
        assert_eq!(e.text(), "hello");
        assert_eq!(e.insertion_point(), 5);
    }

    #[test]
    fn take_line_resets_buffer() {
        let mut e = emacs_editor();
        for c in "ls".chars() {
            type_char(&mut e, c);
        }
        let line = e.take_line();
        assert_eq!(line, "ls");
        assert!(e.text().is_empty());
    }

    #[test]
    fn ctrl_a_moves_to_line_start() {
        let mut e = emacs_editor();
        for c in "abc".chars() {
            type_char(&mut e, c);
        }
        // C-a
        let ev = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        let raw = reedline::ReedlineRawEvent::try_from(ev).unwrap();
        e.handle_raw(raw);
        assert_eq!(e.insertion_point(), 0);
    }

    #[test]
    fn ctrl_e_moves_to_line_end() {
        let mut e = emacs_editor();
        for c in "abc".chars() {
            type_char(&mut e, c);
        }
        // Move to start first, then C-e back to end.
        let start = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        e.handle_raw(reedline::ReedlineRawEvent::try_from(start).unwrap());
        assert_eq!(e.insertion_point(), 0);
        let end = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('e'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        e.handle_raw(reedline::ReedlineRawEvent::try_from(end).unwrap());
        assert_eq!(e.insertion_point(), 3);
    }

    #[test]
    fn backspace_deletes_left() {
        let mut e = emacs_editor();
        for c in "abc".chars() {
            type_char(&mut e, c);
        }
        let bs = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        ));
        e.handle_raw(reedline::ReedlineRawEvent::try_from(bs).unwrap());
        assert_eq!(e.text(), "ab");
    }

    #[test]
    fn enter_submits() {
        let mut e = emacs_editor();
        for c in "ls".chars() {
            type_char(&mut e, c);
        }
        let enter = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        let outcome = e.handle_raw(reedline::ReedlineRawEvent::try_from(enter).unwrap());
        assert_eq!(outcome, EditorOutcome::Submitted);
    }

    #[test]
    fn ctrl_d_on_empty_exits() {
        let mut e = emacs_editor();
        let ctrl_d = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('d'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        let outcome = e.handle_raw(reedline::ReedlineRawEvent::try_from(ctrl_d).unwrap());
        assert_eq!(outcome, EditorOutcome::Exit);
    }

    #[test]
    fn ctrl_d_on_nonempty_deletes_right() {
        let mut e = emacs_editor();
        for c in "ab".chars() {
            type_char(&mut e, c);
        }
        // Move cursor to start so there's a char to the right.
        let ctrl_a = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        e.handle_raw(reedline::ReedlineRawEvent::try_from(ctrl_a).unwrap());
        let ctrl_d = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('d'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        e.handle_raw(reedline::ReedlineRawEvent::try_from(ctrl_d).unwrap());
        assert_eq!(e.text(), "b");
    }
}
