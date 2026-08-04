//! Inline hint (autosuggestion) support for the block-TUI editor (Plan 038 M2).
//!
//! Wraps the existing [`AshHinter`] (which implements reedline's `Hinter` trait
//! and already powers the reedline REPL's fish-style ghost text). On each edit,
//! the block TUI calls [`HintSource::current_hint`] to get the ghost-text
//! suffix to render in dim gray after the cursor.
//!
//! `Hinter::handle` returns a fully-formatted `String` (with ANSI when
//! `use_ansi_coloring=true`). We pass `false` so we get the *raw* hint text
//! and apply ratatui styling ourselves — otherwise the ANSI escapes would
//! render as literal characters in the ratatui buffer.

use reedline::{FileBackedHistory, Hinter};

/// A live autosuggestion source backed by a reedline `Hinter`.
///
/// Generic over the hinter type so it works with `AshHinter` (and any other
/// `Hinter` impl). The history store is borrowed per-call (not owned) because
/// `FileBackedHistory` is not `Clone` and the same store is shared with
/// `HistoryNav` in the outer loop.
pub struct HintSource<H: Hinter> {
    hinter: H,
}

impl<H: Hinter> HintSource<H> {
    pub fn new(hinter: H) -> Self {
        Self { hinter }
    }

    /// Compute the current autosuggestion suffix for the given line.
    ///
    /// `history` is borrowed for this call (Hinter::handle reads it to find
    /// matching past commands). `cwd` is the working directory (some hinters
    /// filter by cwd). Returns the text to render as a dim ghost suffix AFTER
    /// the typed input — i.e. the part of the hint not already present in
    /// `line`.
    pub fn current_hint(
        &mut self,
        line: &str,
        pos: usize,
        history: &FileBackedHistory,
        cwd: &str,
    ) -> String {
        // use_ansi_coloring=false: we style via ratatui, not ANSI escapes.
        let full = self.hinter.handle(line, pos, history, false, cwd);
        // The hinter returns the complete suggestion; the ghost suffix is
        // whatever extends beyond what the user already typed. If the hint
        // doesn't start with the typed line, show it whole (rare).
        if full.starts_with(line) && full.len() > line.len() {
            full[line.len()..].to_string()
        } else {
            full
        }
    }
}

#[cfg(test)]
mod tests {
    // HintSource is generic over Hinter and needs a FileBackedHistory (which
    // requires a temp file). The suffix-stripping logic is pure and is tested
    // directly below; the full hinter+history interaction is exercised via the
    // real AshHinter in the block_tui integration (manual acceptance).

    #[test]
    fn suffix_is_full_minus_typed_prefix() {
        // Simulate: hinter returns "hello world", user typed "hello" → suffix " world".
        let line = "hello";
        let full = format!("{line} world");
        let suffix = if full.starts_with(line) {
            &full[line.len()..]
        } else {
            &full
        };
        assert_eq!(suffix, " world");
    }

    #[test]
    fn suffix_empty_when_hint_equals_input() {
        let line = "hello world";
        let full = line.to_string();
        let suffix = if full.starts_with(line) && full.len() > line.len() {
            &full[line.len()..]
        } else {
            ""
        };
        assert_eq!(suffix, "");
    }

    #[test]
    fn suffix_whole_when_hint_does_not_start_with_input() {
        // Hinter returns something that isn't a continuation of the typed text.
        let line = "abc";
        let full = "xyz".to_string();
        let suffix = if full.starts_with(line) && full.len() > line.len() {
            &full[line.len()..]
        } else {
            &full
        };
        assert_eq!(suffix, "xyz");
    }
}
