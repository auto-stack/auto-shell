//! History navigation for the block-TUI editor (Plan 038 M2).
//!
//! reedline's `HistoryCursor` is `pub(crate)`-gated, so we build a minimal
//! equivalent: on first ↑, snapshot the full history into a `Vec<String>`
//! (newest first) and track a cursor into it. ↓ moves back toward the present.
//! This avoids re-querying `FileBackedHistory` on every keystroke and sidesteps
//! the `SearchQuery.start_id` anchoring edge cases.
//!
//! The history store itself is reedline's `FileBackedHistory` (public) — shared
//! with the reedline REPL, so both frontends read/write the same `~/.auto-shell-history`.

use reedline::{FileBackedHistory, History, SearchDirection, SearchQuery};

/// A live history cursor attached to a `FileBackedHistory`.
///
/// Idle state: `entries == None` (not navigating). The first `older()` call
/// loads the snapshot and positions at index 0 (the most recent entry).
/// `younger()` moves toward the present; going past the newest clears
/// navigation (back to live input).
pub struct HistoryNav {
    /// Lazily-loaded snapshot, newest-first.
    entries: Option<Vec<String>>,
    /// Current index into `entries`. 0 = most recent.
    cursor: usize,
    /// The input the user had before entering history navigation — restored
    /// when they navigate back past the newest entry (like fish/bash).
    saved_input: String,
}

impl HistoryNav {
    pub fn new() -> Self {
        Self {
            entries: None,
            cursor: 0,
            saved_input: String::new(),
        }
    }

    /// Navigate to an older entry (↑). Returns the entry to show, if any.
    /// `history` is queried only on the first call (when idle).
    ///
    /// Semantics: the first ↑ from idle loads the snapshot and shows the most
    /// recent entry (index 0). Each subsequent ↑ advances the cursor toward
    /// older entries, clamped at the oldest.
    pub fn older(&mut self, history: &FileBackedHistory, current_input: &str) -> Option<&str> {
        if self.entries.is_none() {
            // Load newest-first snapshot. Backward direction yields most-recent first.
            let query = SearchQuery::everything(SearchDirection::Backward, None);
            let items = history.search(query).ok().unwrap_or_default();
            self.entries = Some(
                items
                    .into_iter()
                    .map(|it| it.command_line)
                    .collect::<Vec<_>>(),
            );
            self.cursor = 0;
            self.saved_input = current_input.to_string();
            // Fall through: return the entry at cursor 0 (most recent).
        } else {
            // Already navigating: advance toward older, clamped.
            self.cursor = self.cursor.saturating_add(1);
        }

        let entries = self.entries.as_ref()?;
        if entries.is_empty() {
            return None;
        }
        // Clamp to the last (oldest) entry.
        if self.cursor >= entries.len() {
            self.cursor = entries.len() - 1;
        }
        entries.get(self.cursor).map(|s| s.as_str())
    }

    /// Navigate to a newer entry (↓). Returns `Some(entry)` to show, or `None`
    /// when we've navigated back past the newest entry (restore saved input).
    pub fn younger(&mut self) -> Option<&str> {
        // Check the reset condition first, before any immutable borrow, so
        // `self.reset()` (mutable borrow) doesn't conflict with `entries`.
        let entries_empty = self
            .entries
            .as_ref()
            .map_or(true, |e| e.is_empty());
        let navigating = self.entries.is_some();
        if !navigating {
            return None;
        }
        if entries_empty || self.cursor == 0 {
            // Empty history, or already at newest — leave navigation,
            // restore saved input.
            self.reset();
            return None;
        }
        self.cursor -= 1;
        // Now safe to borrow immutably — no more mutable access needed.
        self.entries.as_ref()?.get(self.cursor).map(|s| s.as_str())
    }

    /// Leave history navigation (on any edit / submit / Esc). Clears the
    /// snapshot but PRESERVES `saved_input` so the caller can restore the
    /// user's pre-navigation input when leaving via `younger()` past newest.
    pub fn reset(&mut self) {
        self.entries = None;
        self.cursor = 0;
        // NOTE: saved_input is intentionally retained — read via saved_input()
        // after reset to restore the editor buffer.
    }

    /// Clear everything including saved_input (used on submit / new prompt).
    pub fn full_reset(&mut self) {
        self.entries = None;
        self.cursor = 0;
        self.saved_input.clear();
    }

    /// The input to restore when navigating past the newest entry.
    pub fn saved_input(&self) -> &str {
        &self.saved_input
    }

    /// Whether we're actively navigating history.
    pub fn is_navigating(&self) -> bool {
        self.entries.is_some()
    }
}

impl Default for HistoryNav {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a FileBackedHistory in a temp file with the given entries
    /// (oldest first — they're saved in order).
    fn history_with(entries: &[&str]) -> FileBackedHistory {
        use reedline::HistoryItem;
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "ash-038-hist-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.clone();
        let mut h = FileBackedHistory::with_file(1000, path).unwrap();
        for e in entries {
            let _ = h.save(HistoryItem::from_command_line((*e).to_string()));
        }
        // NOTE: file is leaked to the temp dir; OS cleans up eventually.
        let _ = path;
        h
    }

    #[test]
    fn older_returns_most_recent_first() {
        let h = history_with(&["ls", "cd /tmp", "echo hi"]);
        let mut nav = HistoryNav::new();
        // First ↑ from empty input → most recent ("echo hi").
        let first = nav.older(&h, "").unwrap();
        assert_eq!(first, "echo hi");
    }

    #[test]
    fn older_then_older_goes_further_back() {
        let h = history_with(&["ls", "cd /tmp", "echo hi"]);
        let mut nav = HistoryNav::new();
        let _ = nav.older(&h, ""); // echo hi
        // Second ↑: the cursor-advance guard fires on cursor>0, so call again.
        let second = nav.older(&h, "echo hi").unwrap();
        assert_eq!(second, "cd /tmp");
    }

    #[test]
    fn younger_at_newest_resets() {
        let h = history_with(&["ls", "cd /tmp"]);
        let mut nav = HistoryNav::new();
        nav.older(&h, "current");
        // ↓ at newest → None (restore saved input).
        assert!(nav.younger().is_none());
        assert!(!nav.is_navigating());
        assert_eq!(nav.saved_input(), "current");
    }

    #[test]
    fn reset_clears_state() {
        let h = history_with(&["ls"]);
        let mut nav = HistoryNav::new();
        nav.older(&h, "x");
        assert!(nav.is_navigating());
        nav.reset();
        assert!(!nav.is_navigating());
    }

    #[test]
    fn empty_history_returns_none() {
        let h = history_with(&[]);
        let mut nav = HistoryNav::new();
        assert!(nav.older(&h, "").is_none());
    }
}
