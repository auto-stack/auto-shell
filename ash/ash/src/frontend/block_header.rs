//! Block status rendering — Plan 037 M3 (degraded "Block UX").
//!
//! reedline 0.44.0 is an immediate-mode line editor with no scroll-region /
//! alternate-screen / sticky-header API, so a true Warp-style block (header
//! pinned while the body scrolls) is not achievable without forking the line
//! editor. This module is what remains of that fallback in the reedline CLI:
//! a single red right-aligned status marker printed before a FAILED
//! command's output.
//!
//! Success is silent — the user's typed input line sits directly above the
//! result (so echoing the command is noise), and a happy-path "0ms  ✓" line
//! carries no information. Failures keep the marker because silent non-zero
//! exits (e.g. `grep` with no match) would otherwise be invisible;
//! slow-command feedback lives in the next prompt via the `$cmd_duration`
//! module (default threshold: 2s).
//!
//! Layout (marker right-aligned within the terminal width):
//!
//! ```text
//!   > cat missing       ← the user's typed input line (reedline leaves it here)
//!                                              3ms  ✗
//!   cat: missing: No such file or directory
//! ```
//! All functions here are pure (no I/O) so they unit-test without a terminal.

use nu_ansi_term::{Color, Style};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

/// Render the failure status marker (WITHOUT a trailing newline), or `None`
/// on success — the caller prints nothing in that case.
///
/// - `exit_code`  — the command's exit code; 0 = success (→ `None`), anything
///                   else = failure (→ red right-aligned marker).
/// - `duration`   — how long the command took.
/// - `term_width` — current terminal column count (for right-aligning the
///                   marker). When 0 (width unknown), the marker is emitted
///                   bare (no padding).
pub fn render_failure_status(
    exit_code: i32,
    duration: Duration,
    term_width: u16,
) -> Option<String> {
    if exit_code == 0 {
        return None;
    }

    // "{duration}  ✗" in red. It carries ANSI codes; measure display width
    // on the *plain* text so the on-screen columns line up.
    let right_plain = format!("{}  ✗", format_duration(duration));
    let right = Style::new().fg(Color::Red).paint(&right_plain).to_string();
    let right_width = UnicodeWidthStr::width(right_plain.as_str()) as usize;

    let w = term_width as usize;
    Some(if w == 0 || right_width >= w {
        // Width unknown (or terminal narrower than the marker) — no padding.
        right
    } else {
        format!("{}{}", " ".repeat(w - right_width), right)
    })
}

/// Format a duration compactly. Mirrors the three-tier logic of the prompt's
/// `cmd_duration` module (`prompt/modules/cmd_duration.rs:40-48`), but with
/// no minimum-time filter — the block header always shows the duration.
///
///   < 1 s   → "Nms"
///   < 60 s  → "N.Ns"
///   else    → "NmNs"
pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis() as u64;
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{}m{}s", mins, secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_is_silent() {
        // Exit 0 → nothing to print; a happy-path "Nms ✓" line would be noise.
        assert!(render_failure_status(0, Duration::from_millis(12), 80).is_none());
    }

    #[test]
    fn test_failure_has_red_cross() {
        let h = render_failure_status(1, Duration::from_millis(3), 80).unwrap();
        assert!(h.contains("✗"), "failure marker should contain ✗: {}", h);
        assert!(
            h.contains("\x1b[31m") || h.contains("\x1b[38;5;1m"),
            "marker should be red-ish (ANSI 31 or 38;5;1): {}",
            h
        );
    }

    #[test]
    fn test_duration_format() {
        assert_eq!(format_duration(Duration::from_millis(0)), "0ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999ms");
        assert_eq!(format_duration(Duration::from_millis(1000)), "1.0s");
        assert_eq!(format_duration(Duration::from_millis(2500)), "2.5s");
        assert_eq!(format_duration(Duration::from_millis(60_000)), "1m0s");
        assert_eq!(format_duration(Duration::from_millis(125_000)), "2m5s");
    }

    #[test]
    fn test_zero_width_unknown_terminal() {
        // term_width = 0 means "unknown" — the marker is returned bare (no
        // padding math, no panic).
        let h = render_failure_status(2, Duration::from_millis(5), 0).unwrap();
        assert!(h.contains("5ms"));
        assert!(h.contains("✗"));
    }

    #[test]
    fn test_right_alignment_padding() {
        // With a known width the marker is pushed to the right edge: it
        // starts with a run of spaces and ends with the colored marker
        // (ANSI reset sequence included).
        let h = render_failure_status(1, Duration::from_millis(5), 100).unwrap();
        let pad = h.len() - h.trim_start_matches(' ').len();
        assert!(pad > 0, "expected leading padding to right-align: {:?}", h);
        assert!(h.contains("5ms  ✗"));
        assert!(h.ends_with("\x1b[0m"), "marker should be the last span: {:?}", h);
    }

    #[test]
    fn test_narrow_terminal_no_panic() {
        // A terminal narrower than the marker itself — must not panic; the
        // marker is emitted without padding.
        let h = render_failure_status(1, Duration::from_millis(12345), 3).unwrap();
        assert!(h.contains("12.3s"));
        assert!(h.contains("✗"));
    }
}
