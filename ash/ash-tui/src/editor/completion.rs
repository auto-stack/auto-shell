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
    /// The original input line captured at open() time. Each suggestion's
    /// `span` refers to positions in THIS line, so when cycling candidates
    /// (Tab/↓) we must replace against the original — not the already-mutated
    /// buffer — otherwise the span offsets drift and corrupt the text.
    original_line: String,
    /// True when the candidate list was just (re)queried and the current
    /// selection (index 0) has NOT yet been applied to the buffer. The first
    /// Tab after a refresh applies the current candidate instead of advancing,
    /// so the user sees the first new candidate filled in. Cleared on apply.
    dirty: bool,
}

impl CompletionMenu {
    pub fn new(completer: Box<dyn Completer>) -> Self {
        Self {
            completer,
            suggestions: Vec::new(),
            selected: 0,
            original_line: String::new(),
            dirty: false,
        }
    }

    /// Open the menu: query the completer for the current line + position.
    /// Replaces any prior candidates. No-op (closes) if no candidates.
    pub fn open(&mut self, line: &str, pos: usize) {
        // Snapshot the line BEFORE completing — suggestions' spans index into it.
        self.original_line = line.to_string();
        self.suggestions = self.completer.complete(line, pos);
        self.selected = 0;
        // Mark dirty: the first Tab after (re)open should apply candidate 0,
        // not advance to candidate 1.
        self.dirty = !self.suggestions.is_empty();
    }

    /// Close the menu (discard candidates). Called on Esc / submit / any edit
    /// that isn't menu navigation.
    pub fn close(&mut self) {
        self.suggestions.clear();
        self.selected = 0;
        self.original_line.clear();
        self.dirty = false;
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

    /// Whether the candidate list was just (re)queried and candidate 0 hasn't
    /// been applied yet. The first Tab after a refresh applies the current
    /// candidate instead of advancing (so the user sees the first new result).
    pub fn dirty(&self) -> bool {
        self.dirty
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

    /// Apply the currently-selected candidate to `line_buffer`.
    ///
    /// This is the quick-completion primitive: it reconstructs the line from
    /// the ORIGINAL input (captured at `open`) with the selected candidate's
    /// value spliced into its span, then writes it into `line_buffer`. This
    /// makes cycling (Tab/↓) correct even after prior applies — every apply
    /// starts from the unchanged original, so span offsets never drift.
    ///
    /// Returns true if a candidate was applied. Clears the dirty flag (the
    /// current selection has now been applied; subsequent Tabs cycle).
    pub fn apply_selected(&mut self, line_buffer: &mut reedline::LineBuffer) -> bool {
        let s = match self.suggestions.get(self.selected) {
            Some(s) => s,
            None => return false,
        };
        let span = s.span;
        let append_ws = s.append_whitespace;
        let value = s.value.clone();
        self.dirty = false;
        // Rebuild: original[..span.start] + value + original[span.end..]
        let mut new_line = String::with_capacity(self.original_line.len() + value.len());
        new_line.push_str(&self.original_line[..span.start.min(self.original_line.len())]);
        new_line.push_str(&value);
        let end = span.end.min(self.original_line.len());
        new_line.push_str(&self.original_line[end..]);
        line_buffer.set_buffer(new_line);
        // Cursor to end of inserted value + optional trailing space.
        if append_ws {
            line_buffer.insert_char(' ');
        }
        true
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

    /// A suggestion that replaces span [0..1] with `value` (simulates typing
    /// "l" and getting "ls"/"less" back — span covers the typed prefix).
    fn sug_span(value: &str, start: usize, end: usize) -> Suggestion {
        let mut s = sug(value);
        s.span = Span::new(start, end);
        s
    }

    #[test]
    fn apply_selected_writes_first_candidate_to_buffer() {
        // User typed "l" (span 0..1); candidates ls/less replace 0..1.
        let mut m = menu(vec![sug_span("ls", 0, 1), sug_span("less", 0, 1)]);
        let mut lb = reedline::LineBuffer::from("l");
        m.open("l", 1);
        // First candidate applied on open.
        assert!(m.apply_selected(&mut lb));
        assert_eq!(lb.get_buffer(), "ls");
    }

    #[test]
    fn apply_selected_after_next_cycles_correctly() {
        // Cycle to the 2nd candidate: buffer must reflect "less", not corrupt.
        let mut m = menu(vec![sug_span("ls", 0, 1), sug_span("less", 0, 1)]);
        let mut lb = reedline::LineBuffer::from("l");
        m.open("l", 1);
        m.apply_selected(&mut lb); // → "ls"
        assert_eq!(lb.get_buffer(), "ls");
        m.next();
        m.apply_selected(&mut lb); // → "less" (rebuilt from ORIGINAL "l")
        assert_eq!(lb.get_buffer(), "less");
    }

    #[test]
    fn apply_selected_preserves_text_after_span() {
        // User typed "l file" (span 0..1); candidate replaces only the prefix.
        let mut m = menu(vec![sug_span("ls", 0, 1)]);
        let mut lb = reedline::LineBuffer::from("l file");
        m.open("l file", 1);
        m.apply_selected(&mut lb);
        assert_eq!(lb.get_buffer(), "ls file");
    }

    #[test]
    fn apply_selected_appends_whitespace_when_requested() {
        let mut s = sug_span("cd", 0, 1);
        s.append_whitespace = true;
        let mut m = menu(vec![s]);
        let mut lb = reedline::LineBuffer::from("c");
        m.open("c", 1);
        m.apply_selected(&mut lb);
        assert_eq!(lb.get_buffer(), "cd ");
    }

    #[test]
    fn apply_selected_returns_false_when_no_candidates() {
        let mut m = menu(vec![]);
        let mut lb = reedline::LineBuffer::from("x");
        m.open("x", 1); // StubCompleter returns the empty vec
        assert!(!m.apply_selected(&mut lb));
        assert_eq!(lb.get_buffer(), "x");
    }
}
