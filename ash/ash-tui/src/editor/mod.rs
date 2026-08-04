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

pub mod completion;
pub mod dispatch;
pub mod history;
pub mod hints;

use reedline::{EditMode, LineBuffer};

use completion::CompletionMenu;

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
    /// M2: the cached ghost-text hint for the current input. Computed by the
    /// outer loop (which owns the HintSource) and read by the renderer.
    current_hint: String,
    /// M2: completion menu state. `None` until `with_completion` injects a
    /// completer. When `Some`, Tab opens/navigates the menu.
    completion: Option<CompletionMenu>,
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
            current_hint: String::new(),
            completion: None,
        }
    }

    /// M2: attach a completer for Tab completion.
    pub fn with_completion(mut self, menu: CompletionMenu) -> Self {
        self.completion = Some(menu);
        self
    }

    /// M2: set the cached ghost-text hint (computed by the outer loop, which
    /// owns the `HintSource` + cwd). Read by the renderer.
    pub fn set_hint(&mut self, hint: String) {
        self.current_hint = hint;
    }

    /// The cached ghost-text hint suffix (for rendering dim gray).
    pub fn hint(&self) -> &str {
        &self.current_hint
    }

    /// A borrowed view of the completion menu state (for rendering), if any.
    pub fn completion(&self) -> Option<&CompletionMenu> {
        self.completion.as_ref()
    }

    /// The current input text.
    pub fn text(&self) -> &str {
        self.line_buffer.get_buffer()
    }

    /// Take the submitted line, leaving the editor empty for the next prompt.
    /// Also clears the hint, closes the completion menu, and exits history nav.
    pub fn take_line(&mut self) -> String {
        let text = self.line_buffer.get_buffer().to_string();
        self.line_buffer = LineBuffer::new();
        self.cut_buffer.clear();
        self.current_hint.clear();
        if let Some(m) = self.completion.as_mut() {
            m.close();
        }
        text
    }

    /// The byte offset of the cursor in the buffer (for rendering the caret).
    pub fn insertion_point(&self) -> usize {
        self.line_buffer.insertion_point()
    }

    /// Replace the entire buffer with `text` and move the cursor to the end.
    /// Used by history navigation (↑/↓) to swap in a past command.
    pub fn replace_buffer(&mut self, text: String) {
        self.line_buffer.set_buffer(text);
        self.current_hint.clear();
        if let Some(m) = self.completion.as_mut() {
            m.close();
        }
    }

    /// Feed one raw crossterm event through the keybinding parser + dispatch.
    /// Returns the outcome for the outer loop.
    ///
    /// Interaction priority (each handled BEFORE the EditMode parser):
    /// 1. Completion menu navigation: when the menu is open, ↑/↓/Tab/Esc
    ///    navigate/close it instead of moving the cursor.
    /// 2. History navigation: when the menu is closed, ↑/↓ walk history
    ///    (if a history store is attached).
    /// 3. Arrow keys + Home/End: reedline defaults don't bind them.
    /// 4. Hint acceptance: Right/End at the line end accepts the ghost hint.
    /// 5. Tab opens the completion menu (if a completer is attached).
    pub fn handle_event(&mut self, event: crossterm::event::Event) -> EditorOutcome {
        use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

        if let crossterm::event::Event::Key(ke) = &event {
            // Windows terminals emit Press + Release for each keypress; only
            // act on Press/Repeat so we don't double-handle.
            if !matches!(ke.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                return EditorOutcome::Continue;
            }
            let no_mods = ke.modifiers == KeyModifiers::NONE;
            let menu_open = self.completion.as_ref().map_or(false, |m| m.is_open());

            // ── Completion menu navigation (menu already open) ────────
            if menu_open {
                match (no_mods, ke.code) {
                    // Tab / ↓ cycle to the next candidate + apply it. BUT if
                    // the list was just refreshed (dirty), apply the current
                    // (first) candidate first instead of advancing — so after
                    // typing changes the prefix, the first Tab fills in the
                    // first new candidate rather than skipping to #2.
                    (true, KeyCode::Tab) | (true, KeyCode::Down) => {
                        if let Some(m) = self.completion.as_mut() {
                            if !m.dirty() {
                                m.next();
                            }
                            m.apply_selected(&mut self.line_buffer);
                            self.current_hint.clear();
                        }
                        return EditorOutcome::Continue;
                    }
                    (true, KeyCode::Up) => {
                        if let Some(m) = self.completion.as_mut() {
                            if !m.dirty() {
                                m.previous();
                            }
                            m.apply_selected(&mut self.line_buffer);
                            self.current_hint.clear();
                        }
                        return EditorOutcome::Continue;
                    }
                    (true, KeyCode::Esc) => {
                        if let Some(m) = self.completion.as_mut() {
                            m.close();
                        }
                        return EditorOutcome::Continue;
                    }
                    // Enter accepts the selected candidate, then submits.
                    (true, KeyCode::Enter) => {
                        self.accept_selected_completion();
                        return EditorOutcome::Submitted;
                    }
                    _ => {}
                }
            }

            // ↑/↓ with no history and no menu: single-line editor → line ends.
            // (Real history navigation is handled by the outer loop, which owns
            // the FileBackedHistory; this is the fallback when no history is
            // attached there.)
            if no_mods && ke.code == KeyCode::Up {
                dispatch::dispatch_edit_command(
                    self,
                    reedline::EditCommand::MoveToLineStart { select: false },
                );
                return EditorOutcome::Continue;
            }
            if no_mods && ke.code == KeyCode::Down {
                dispatch::dispatch_edit_command(
                    self,
                    reedline::EditCommand::MoveToLineEnd { select: false },
                );
                return EditorOutcome::Continue;
            }

            // ── Left/Right/Home/End + hint acceptance ────────────────
            if no_mods {
                match ke.code {
                    KeyCode::Left => {
                        dispatch::dispatch_edit_command(
                            self,
                            reedline::EditCommand::MoveLeft { select: false },
                        );
                        return EditorOutcome::Continue;
                    }
                    KeyCode::Right | KeyCode::End => {
                        // At line end with a hint → accept it; else move.
                        let at_end =
                            self.line_buffer.insertion_point() >= self.line_buffer.len();
                        if at_end && !self.current_hint.is_empty() {
                            let hint = std::mem::take(&mut self.current_hint);
                            self.line_buffer.insert_str(&hint);
                            return EditorOutcome::Continue;
                        }
                        if ke.code == KeyCode::End {
                            dispatch::dispatch_edit_command(
                                self,
                                reedline::EditCommand::MoveToLineEnd { select: false },
                            );
                        } else {
                            dispatch::dispatch_edit_command(
                                self,
                                reedline::EditCommand::MoveRight { select: false },
                            );
                        }
                        return EditorOutcome::Continue;
                    }
                    KeyCode::Home => {
                        dispatch::dispatch_edit_command(
                            self,
                            reedline::EditCommand::MoveToLineStart { select: false },
                        );
                        return EditorOutcome::Continue;
                    }
                    // Tab: quick-completion (bash/fish style). First press opens
                    // the menu AND immediately applies the first candidate to the
                    // buffer; subsequent Tab presses cycle to the next candidate
                    // and update the buffer. The menu stays open so the user can
                    // keep cycling; Enter/Esc/any other key closes it.
                    KeyCode::Tab => {
                        if let Some(m) = self.completion.as_mut() {
                            if !m.is_open() {
                                m.open(
                                    self.line_buffer.get_buffer(),
                                    self.line_buffer.insertion_point(),
                                );
                            } else {
                                m.next();
                            }
                            if m.is_open() {
                                // Apply the (newly-selected) candidate to the buffer.
                                m.apply_selected(&mut self.line_buffer);
                                self.current_hint.clear();
                                return EditorOutcome::Continue;
                            }
                        }
                        // No completer or no candidates: fall through to edit_mode.
                    }
                    _ => {}
                }
            }
        }

        // Non-arrow keys: route through the reedline EditMode parser.
        let outcome = match reedline::ReedlineRawEvent::try_from(event) {
            Ok(raw) => {
                let ev = self.edit_mode.parse_event(raw);
                self.dispatch_reedline_event(ev)
            }
            // KeyRelease and a few other event kinds are rejected by reedline's
            // TryFrom — they're a no-op for us.
            Err(()) => EditorOutcome::Continue,
        };

        // M3: if the completion menu is open and this key mutated the buffer
        // (typed a char/space, backspace, etc.), re-query the completer so the
        // candidate list reflects the NEW prefix. Without this, typing "ls "
        // after Tab-completing "ls" would leave the stale [ls/less] menu
        // active, and the next Tab would cycle it instead of listing ls's
        // argument completions (-a, -l, ...).
        //
        // We refresh on any non-navigation key (navigation = Tab/↓/↑/Esc/Enter,
        // which are handled above and return early). If the refreshed query
        // returns no candidates, close the menu.
        if let Some(m) = self.completion.as_mut() {
            if m.is_open() {
                m.open(self.line_buffer.get_buffer(), self.line_buffer.insertion_point());
            }
        }

        outcome
    }

    /// Accept the currently-selected candidate (apply it + close the menu).
    /// Used by Enter to finalize a completion before submitting.
    fn accept_selected_completion(&mut self) {
        if let Some(m) = self.completion.as_mut() {
            m.apply_selected(&mut self.line_buffer);
            m.close();
        }
        self.current_hint.clear();
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
            // Arrow keys surfaced as ReedlineEvents (Vi may emit these; Emacs
            // returns None for unbound arrows, handled earlier in handle_event).
            // Map them to the same cursor moves as the direct-key fallback.
            ReedlineEvent::Left => {
                dispatch::dispatch_edit_command(
                    self,
                    reedline::EditCommand::MoveLeft { select: false },
                );
                EditorOutcome::Continue
            }
            ReedlineEvent::Right => {
                dispatch::dispatch_edit_command(
                    self,
                    reedline::EditCommand::MoveRight { select: false },
                );
                EditorOutcome::Continue
            }
            ReedlineEvent::Up => {
                // Single-line editor: Up goes to line start (history is M2).
                dispatch::dispatch_edit_command(
                    self,
                    reedline::EditCommand::MoveToLineStart { select: false },
                );
                EditorOutcome::Continue
            }
            ReedlineEvent::Down => {
                dispatch::dispatch_edit_command(
                    self,
                    reedline::EditCommand::MoveToLineEnd { select: false },
                );
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

    /// Helper: feed a crossterm char-press into the editor (Press kind).
    fn type_char(editor: &mut Editor, c: char) {
        editor.handle_event(key_press(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        ));
    }

    /// Build a KeyEvent with explicit `KeyEventKind::Press` (the kind Windows
    /// Terminal emits for the key-down half; crossterm's `KeyEvent::new`
    /// defaults to Press, but being explicit documents the assumption).
    fn key_press(
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> crossterm::event::Event {
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new_with_kind_and_state(
            code,
            modifiers,
            crossterm::event::KeyEventKind::Press,
            crossterm::event::KeyEventState::NONE,
        ))
    }

    /// Build a Release-kind key event (the key-up half on Windows terminals).
    fn key_release(
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> crossterm::event::Event {
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new_with_kind_and_state(
            code,
            modifiers,
            crossterm::event::KeyEventKind::Release,
            crossterm::event::KeyEventState::NONE,
        ))
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
        e.handle_event(key_press(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(e.insertion_point(), 0);
    }

    #[test]
    fn ctrl_e_moves_to_line_end() {
        let mut e = emacs_editor();
        for c in "abc".chars() {
            type_char(&mut e, c);
        }
        // Move to start first, then C-e back to end.
        e.handle_event(key_press(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(e.insertion_point(), 0);
        e.handle_event(key_press(
            crossterm::event::KeyCode::Char('e'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(e.insertion_point(), 3);
    }

    #[test]
    fn backspace_deletes_left() {
        let mut e = emacs_editor();
        for c in "abc".chars() {
            type_char(&mut e, c);
        }
        e.handle_event(key_press(
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.text(), "ab");
    }

    #[test]
    fn enter_submits() {
        let mut e = emacs_editor();
        for c in "ls".chars() {
            type_char(&mut e, c);
        }
        let outcome = e.handle_event(key_press(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(outcome, EditorOutcome::Submitted);
    }

    #[test]
    fn ctrl_d_on_empty_exits() {
        let mut e = emacs_editor();
        let outcome = e.handle_event(key_press(
            crossterm::event::KeyCode::Char('d'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(outcome, EditorOutcome::Exit);
    }

    #[test]
    fn ctrl_d_on_nonempty_deletes_right() {
        let mut e = emacs_editor();
        for c in "ab".chars() {
            type_char(&mut e, c);
        }
        // Move cursor to start so there's a char to the right.
        e.handle_event(key_press(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        e.handle_event(key_press(
            crossterm::event::KeyCode::Char('d'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(e.text(), "b");
    }

    #[test]
    fn left_arrow_moves_cursor_left() {
        let mut e = emacs_editor();
        for c in "abc".chars() {
            type_char(&mut e, c);
        }
        assert_eq!(e.insertion_point(), 3);
        e.handle_event(key_press(
            crossterm::event::KeyCode::Left,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.insertion_point(), 2);
    }

    #[test]
    fn right_arrow_moves_cursor_right() {
        let mut e = emacs_editor();
        for c in "abc".chars() {
            type_char(&mut e, c);
        }
        // Move to start, then right once.
        e.handle_event(key_press(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(e.insertion_point(), 0);
        e.handle_event(key_press(
            crossterm::event::KeyCode::Right,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.insertion_point(), 1);
    }

    #[test]
    fn home_end_arrows_work() {
        let mut e = emacs_editor();
        for c in "abc".chars() {
            type_char(&mut e, c);
        }
        e.handle_event(key_press(
            crossterm::event::KeyCode::Home,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.insertion_point(), 0);
        e.handle_event(key_press(
            crossterm::event::KeyCode::End,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.insertion_point(), 3);
    }

    #[test]
    fn key_release_is_ignored_for_arrows() {
        // Windows terminals emit Press + Release for each keypress; Release
        // must not trigger a second move.
        let mut e = emacs_editor();
        for c in "ab".chars() {
            type_char(&mut e, c);
        }
        assert_eq!(e.insertion_point(), 2);
        // Press left once → cursor at 1.
        e.handle_event(key_press(
            crossterm::event::KeyCode::Left,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.insertion_point(), 1);
        // Release left → must NOT move again (would go to 0).
        e.handle_event(key_release(
            crossterm::event::KeyCode::Left,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.insertion_point(), 1);
    }

    /// A stateful completer for the menu-refresh test: returns command-name
    /// candidates for a bare prefix, and argument candidates once the line
    /// contains a space (simulating `ls ` → [-a, -l]).
    struct PrefixCompleter;
    impl reedline::Completer for PrefixCompleter {
        fn complete(&mut self, line: &str, _pos: usize) -> Vec<reedline::Suggestion> {
            use reedline::{Span, Suggestion};
            let sug = |v: &str, start: usize| Suggestion {
                value: v.to_string(),
                description: None,
                style: None,
                extra: None,
                span: Span::new(start, line.len()),
                append_whitespace: false,
                match_indices: None,
            };
            if line.contains(' ') {
                // Argument completion: suggest flags after the space.
                let space = line.find(' ').unwrap_or(line.len());
                vec![sug("-a", space + 1), sug("-l", space + 1)]
            } else {
                // Command-name completion.
                vec![sug("ls", 0), sug("less", 0)]
            }
        }
    }

    fn editor_with_completer() -> Editor {
        let mut e = Editor::new(Box::new(Emacs::new(default_emacs_keybindings())));
        e = e.with_completion(crate::editor::completion::CompletionMenu::new(
            Box::new(PrefixCompleter),
        ));
        e
    }

    #[test]
    fn typing_after_tab_refreshes_candidates() {
        // The bug: Tab completes "l"→"ls" (menu shows [ls,less]); typing a
        // space should refresh the menu to argument candidates [-a,-l], but
        // before the fix the menu stayed stale and Tab cycled [ls,less].
        let mut e = editor_with_completer();
        type_char(&mut e, 'l');
        // Tab → opens menu + applies first candidate ("ls").
        e.handle_event(key_press(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.text(), "ls");
        // Menu now holds [ls, less].
        let m = e.completion().unwrap();
        assert_eq!(m.suggestions().len(), 2);
        assert_eq!(m.suggestions()[0].value, "ls");

        // Type a space — buffer becomes "ls ".
        type_char(&mut e, ' ');
        assert_eq!(e.text(), "ls ");
        // Menu should have refreshed to argument candidates [-a, -l].
        let m = e.completion().unwrap();
        assert!(m.is_open());
        assert_eq!(m.suggestions().len(), 2);
        assert_eq!(m.suggestions()[0].value, "-a");
        assert_eq!(m.suggestions()[1].value, "-l");
    }

    #[test]
    fn tab_after_space_applies_argument_candidate() {
        let mut e = editor_with_completer();
        type_char(&mut e, 'l');
        e.handle_event(key_press(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        )); // "ls"
        type_char(&mut e, ' '); // "ls "
        // Tab again → should apply the first argument candidate "-a".
        e.handle_event(key_press(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(e.text(), "ls -a");
    }
}
