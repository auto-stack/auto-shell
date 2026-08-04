//! Completion menu state for the block-TUI editor (Plan 038 M2).
//!
//! Wraps the existing [`ShellCompleter`] (which implements reedline's
//! `Completer` trait and already powers the reedline REPL's Tab completion).
//! When the user hits Tab, `CompletionMenu` calls `completer.complete(line,
//! pos)` and caches the resulting `Vec<Suggestion>` plus a selection cursor.
//! The block-TUI renderer reads `suggestions()` / `selected()` to draw the
//! floating menu.
//!
//! reedline's own `ColumnarMenu` needs a `&Painter` (pub(crate)) to lay out,
//! so we can't reuse its rendering — but the *data* path is fully reusable.

use reedline::{Completer, Suggestion};

/// The completion menu state. Idle when `suggestions` is empty.
///
/// Holds the completer as `Box<dyn Completer>` so the `Editor` can own it
/// without a second type parameter. (The original design used a generic
/// `C: Completer`, but `Editor` already has one type param via the edit mode
/// and `CompletionMenu<dyn Completer>` is not valid — `dyn Trait` is unsized.)
pub struct CompletionMenu {
    /// The wrapped completer (ShellCompleter in practice).
    completer: Box<dyn Completer>,
    /// Current candidate list (empty = menu closed).
    suggestions: Vec<Suggestion>,
    /// Selected index into `suggestions`. 0 = first.
    selected: usize,
}

impl CompletionMenu {
    pub fn new(completer: Box<dyn Completer>) -> Self {
        Self {
            completer,
            suggestions: Vec::new(),
            selected: 0,
        }
    }

    /// Open the menu: query the completer for the current line + position.
    /// Replaces any prior candidates. No-op (closes) if no candidates.
    pub fn open(&mut self, line: &str, pos: usize) {
        self.suggestions = self.completer.complete(line, pos);
        self.selected = 0;
    }

    /// Close the menu (discard candidates). Called on Esc / submit / any edit
    /// that isn't menu navigation.
    pub fn close(&mut self) {
        self.suggestions.clear();
        self.selected = 0;
    }

    /// Whether the menu is currently open (has candidates).
    pub fn is_open(&self) -> bool {
        !self.suggestions.is_empty()
    }

    /// The current candidate list (for rendering).
    pub fn suggestions(&self) -> &[Suggestion] {
        &self.suggestions
    }

    /// The index of the highlighted candidate.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Move selection to the next candidate (wraps). No-op if closed.
    pub fn next(&mut self) {
        if !self.suggestions.is_empty() {
            self.selected = (self.selected + 1) % self.suggestions.len();
        }
    }

    /// Move selection to the previous candidate (wraps). No-op if closed.
    pub fn previous(&mut self) {
        if !self.suggestions.is_empty() {
            if self.selected == 0 {
                self.selected = self.suggestions.len() - 1;
            } else {
                self.selected -= 1;
            }
        }
    }

    /// Take the currently-selected suggestion (for applying to the buffer).
    /// Returns the suggestion's `value` and replacement `span` and whether to
    /// append a trailing space. Does NOT close the menu (caller decides).
    pub fn selected_suggestion(&self) -> Option<(&str, reedline::Span, bool)> {
        let s = self.suggestions.get(self.selected)?;
        Some((&s.value, s.span, s.append_whitespace))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reedline::{Span, Suggestion};

    /// A stub completer that returns a fixed list regardless of input.
    struct StubCompleter(Vec<Suggestion>);
    impl Completer for StubCompleter {
        fn complete(&mut self, _line: &str, _pos: usize) -> Vec<Suggestion> {
            self.0.clone()
        }
    }

    fn menu(sugs: Vec<Suggestion>) -> CompletionMenu {
        CompletionMenu::new(Box::new(StubCompleter(sugs)))
    }

    fn sug(value: &str) -> Suggestion {
        Suggestion {
            value: value.to_string(),
            description: None,
            style: None,
            extra: None,
            span: Span::new(0, 0),
            append_whitespace: false,
            match_indices: None,
        }
    }

    #[test]
    fn open_loads_candidates() {
        let mut m = menu(vec![sug("ls"), sug("cd")]);
        assert!(!m.is_open());
        m.open("l", 1);
        assert!(m.is_open());
        assert_eq!(m.suggestions().len(), 2);
        assert_eq!(m.selected(), 0);
    }

    #[test]
    fn next_wraps_around() {
        let mut m = menu(vec![sug("a"), sug("b"), sug("c")]);
        m.open("", 0);
        assert_eq!(m.selected(), 0);
        m.next();
        assert_eq!(m.selected(), 1);
        m.next();
        assert_eq!(m.selected(), 2);
        m.next(); // wraps
        assert_eq!(m.selected(), 0);
    }

    #[test]
    fn previous_wraps_around() {
        let mut m = menu(vec![sug("a"), sug("b")]);
        m.open("", 0);
        m.previous(); // at 0 → wraps to last
        assert_eq!(m.selected(), 1);
    }

    #[test]
    fn close_clears() {
        let mut m = menu(vec![sug("a")]);
        m.open("", 0);
        assert!(m.is_open());
        m.close();
        assert!(!m.is_open());
    }

    #[test]
    fn selected_suggestion_returns_value_and_span() {
        let mut s = sug("git");
        s.span = Span::new(0, 1);
        s.append_whitespace = true;
        let mut m = menu(vec![s]);
        m.open("", 0);
        let (val, span, aws) = m.selected_suggestion().unwrap();
        assert_eq!(val, "git");
        assert_eq!(span, Span::new(0, 1));
        assert!(aws);
    }
}
