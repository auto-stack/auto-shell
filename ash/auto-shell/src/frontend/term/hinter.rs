//! History-driven ghost-text hinter (Plan 032 M1.2).
//!
//! Replaces reedline's `CwdAwareHinter` with an Ash-owned `Hinter` that adds a
//! **fuzzy** fallback: when no history entry is an exact prefix of the current
//! line (the only strategy `CwdAwareHinter` had), it tries a prefix-subsequence
//! match against the same history, so typing `gcm` can ghost-complete to
//! `git commit -m` even though no history entry *starts with* `gcm`.
//!
//! Behavior (in priority order):
//! 1. **Prefix match** — identical to `CwdAwareHinter`: find the most recent
//!    history entry whose command line starts with `line`, show its suffix.
//! 2. **Fuzzy fallback** — if (1) finds nothing, find the most recent entry
//!    that contains `line`'s characters as a subsequence (prefix-subsequence,
//!    i.e. the first char must match), and show the suffix after the last
//!    matched input char.
//! 3. Otherwise nothing.
//!
//! Design note: we deliberately do NOT call an LLM here. Real-time ghost-text
//! fires on every keystroke; an LLM round-trip (even local Ollama) is too slow
//! (>200 ms) and would lag the cursor. AI completion is Tab-triggered only
//! (Plan 032 M2). The fuzzy fallback is pure-local and effectively free.

use nu_ansi_term::Style;
use reedline::{
    Hinter, History, ReedlineError, ReedlineErrorVariants::HistoryFeatureUnsupported, SearchQuery,
};

/// Ash's history-driven hinter with a fuzzy fallback.
///
/// Construct via [`AshHinter::default`] then chain `.with_style()` /
/// `.with_min_chars()` exactly like the old `CwdAwareHinter`.
pub struct AshHinter {
    style: Style,
    current_hint: String,
    min_chars: usize,
}

impl Hinter for AshHinter {
    fn handle(
        &mut self,
        line: &str,
        #[allow(unused_variables)] _pos: usize,
        history: &dyn History,
        use_ansi_coloring: bool,
        cwd: &str,
    ) -> String {
        self.current_hint = if line.chars().count() >= self.min_chars {
            // 1. Prefix match (CwdAwareHinter-compatible behavior).
            let prefix_hint = prefix_hint(line, history, cwd);
            if !prefix_hint.is_empty() {
                prefix_hint
            } else {
                // 2. Fuzzy fallback — only when no prefix match exists.
                fuzzy_hint(line, history, cwd)
            }
        } else {
            String::new()
        };

        if use_ansi_coloring && !self.current_hint.is_empty() {
            self.style.paint(&self.current_hint).to_string()
        } else {
            self.current_hint.clone()
        }
    }

    fn complete_hint(&self) -> String {
        self.current_hint.clone()
    }

    fn next_hint_token(&self) -> String {
        // reedline keeps `get_first_token` private to its hinter module, so we
        // replicate its behavior: the first whitespace-delimited token of the
        // current hint (for incremental Ctrl+→ completion).
        self.current_hint
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()
    }
}

impl Default for AshHinter {
    fn default() -> Self {
        // Match CwdAwareHinter's defaults so the swap is invisible when the
        // style/min_chars aren't overridden by config.
        AshHinter {
            style: Style::new().fg(nu_ansi_term::Color::LightGray),
            current_hint: String::new(),
            min_chars: 1,
        }
    }
}

impl AshHinter {
    /// Set the style applied to the hint as part of the buffer.
    #[must_use]
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the minimum number of typed characters before hints appear.
    #[must_use]
    pub fn with_min_chars(mut self, min_chars: usize) -> Self {
        self.min_chars = min_chars;
        self
    }
}

// ── Hint strategies ─────────────────────────────────────────────────────

/// Prefix-match hint: find the most recent history entry (preferring the same
/// cwd) whose command line starts with `line`, return the unmatched suffix.
///
/// This is exactly what `CwdAwareHinter` did — kept verbatim so the swap is a
/// pure superset (we only ever add the fuzzy branch).
fn prefix_hint(line: &str, history: &dyn History, cwd: &str) -> String {
    // Prefer a match in the current working directory; fall back to a global
    // prefix search if cwd-scoped history isn't supported by this backend.
    let with_cwd = history
        .search(SearchQuery::last_with_prefix_and_cwd(
            line.to_string(),
            cwd.to_string(),
            history.session(),
        ))
        .or_else(|err| {
            if let ReedlineError(HistoryFeatureUnsupported { .. }) = err {
                history.search(SearchQuery::last_with_prefix(
                    line.to_string(),
                    history.session(),
                ))
            } else {
                Err(err)
            }
        })
        .unwrap_or_default();
    if !with_cwd.is_empty() {
        return with_cwd[0]
            .command_line
            .get(line.len()..)
            .unwrap_or_default()
            .to_string();
    }
    history
        .search(SearchQuery::last_with_prefix(
            line.to_string(),
            history.session(),
        ))
        .unwrap_or_default()
        .first()
        .map_or_else(String::new, |entry| {
            entry
                .command_line
                .get(line.len()..)
                .unwrap_or_default()
                .to_string()
        })
}

/// Fuzzy (prefix-subsequence) hint: when no history entry starts with `line`,
/// find the most recent entry whose command line contains `line`'s characters
/// in order (the first char must still match — a "prefix-subsequence"), and
/// return the suffix after the last matched input character.
///
/// Example: `line = "gcm"`, history has `git commit -m`. The chars g, c, m
/// appear in order in `git commit -m`; the last matched char (`m`) sits in
/// `... -m`, so we return the remainder (the part of `commit -m` after the m).
/// In practice this means typing `gcm` ghosts the rest of `git commit -m`
/// rather than showing nothing.
fn fuzzy_hint(line: &str, history: &dyn History, cwd: &str) -> String {
    if line.is_empty() {
        return String::new();
    }
    // Pull recent entries (cwd-scoped first, then global) and walk newest-first.
    let entries = match history.search(SearchQuery::last_with_prefix_and_cwd(
        String::new(),
        cwd.to_string(),
        history.session(),
    )) {
        Ok(e) if !e.is_empty() => e,
        _ => history
            .search(SearchQuery::last_with_prefix(
                String::new(),
                history.session(),
            ))
            .unwrap_or_default(),
    };
    for entry in entries {
        if let Some(suffix) = fuzzy_match_suffix(line, &entry.command_line) {
            return suffix;
        }
    }
    String::new()
}

/// If `candidate` contains `input`'s chars in order as a prefix-subsequence
/// (first char must match), return the candidate suffix after the last matched
/// input char. Otherwise `None`.
///
/// We require the first input char to match the first candidate char so the
/// fuzzy hint never "jumps tools" (e.g. `gcm` won't match `rm something`),
/// matching the semantics of the file-completion fuzzy matcher.
fn fuzzy_match_suffix(input: &str, candidate: &str) -> Option<String> {
    let input_chars: Vec<char> = input.chars().collect();
    let cand_chars: Vec<char> = candidate.chars().collect();
    if input_chars.is_empty() || cand_chars.is_empty() {
        return None;
    }
    // First char must match (prefix requirement).
    if input_chars[0] != cand_chars[0] {
        return None;
    }
    let mut input_idx = 0;
    for (i, &c) in cand_chars.iter().enumerate() {
        if input_idx < input_chars.len() && c == input_chars[input_idx] {
            input_idx += 1;
            if input_idx == input_chars.len() {
                // All input chars matched as a subsequence; return the suffix
                // after this (the last) matched char so Ctrl+F completes
                // incrementally.
                let suffix: String = cand_chars[i + 1..].iter().collect();
                return Some(suffix);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_exact_prefix_yields_full_suffix() {
        // input is a strict prefix → suffix is the remainder.
        assert_eq!(
            fuzzy_match_suffix("git c", "git commit -m"),
            Some("ommit -m".to_string())
        );
    }

    #[test]
    fn fuzzy_subsequence_matches_gcm() {
        // The headline case: g c m appear in order in `git commit -m`.
        // Indices: g(0) c(4) m(6) → suffix starts after index 6 = "mit -m".
        assert_eq!(
            fuzzy_match_suffix("gcm", "git commit -m"),
            Some("mit -m".to_string())
        );
    }

    #[test]
    fn fuzzy_requires_first_char_match() {
        // First char must agree — `x` won't fuzzy into `git ...`.
        assert_eq!(fuzzy_match_suffix("xgcm", "git commit -m"), None);
    }

    #[test]
    fn fuzzy_no_match_when_chars_out_of_order() {
        assert_eq!(fuzzy_match_suffix("mcg", "git commit -m"), None);
    }

    #[test]
    fn fuzzy_empty_input_returns_none() {
        assert_eq!(fuzzy_match_suffix("", "git commit -m"), None);
    }

    #[test]
    fn fuzzy_input_longer_than_candidate_returns_none() {
        assert_eq!(fuzzy_match_suffix("git commit -m extra", "git commit -m"), None);
    }

    #[test]
    fn ash_hinter_defaults_match_cwd_aware() {
        // The swap should be invisible without config overrides.
        let h = AshHinter::default();
        assert_eq!(h.min_chars, 1);
        assert!(h.current_hint.is_empty());
    }

    #[test]
    fn ash_hinter_builders_chain() {
        let h = AshHinter::default()
            .with_min_chars(3)
            .with_style(Style::new().bold());
        assert_eq!(h.min_chars, 3);
    }
}
