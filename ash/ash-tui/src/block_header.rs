//! Block header rendering — Plan 037 M3 (degraded "Block UX").
//!
//! reedline 0.44.0 is an immediate-mode line editor with no scroll-region /
//! alternate-screen / sticky-header API, so a true Warp-style block (header
//! pinned while the body scrolls) is not achievable without forking the line
//! editor. This module implements the plan's documented fallback: print a
//! single colored header line before each command's output.
//!
//! The header mirrors ash-gui's block color convention
//! (`ash-gui-bin/src/renderer.rs:193-199`):
//!   - success (exit 0)  → green `✓`
//!   - failure (exit≠0)  → red   `✗`
//!
//! Layout (right-aligned status within the terminal width):
//!
//! ```text
//!   ❯ ls -la                              12ms  ✓
//!   ❯ cat missing                          3ms  ✗
//! ```
//!
//! All functions here are pure (no I/O) so they unit-test without a terminal.

use nu_ansi_term::{Color, Style};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

/// Render the block header line (WITHOUT a trailing newline). The caller is
/// expected to `println!` it.
///
/// - `command`    — the (post-expansion) command text to echo.
/// - `exit_code`  — the command's exit code; 0 = success, anything else = failure.
/// - `duration`   — how long the command took.
/// - `term_width` — current terminal column count (for right-aligning the
///                   status). When 0 (width unknown), the status is printed
///                   left-aligned after a single space.
pub fn render_block_header(
    command: &str,
    exit_code: i32,
    duration: Duration,
    term_width: u16,
) -> String {
    let ok = exit_code == 0;
    let (status_icon, status_color) = if ok {
        ("✓", Color::Green)
    } else {
        ("✗", Color::Red)
    };
    let duration_str = format_duration(duration);

    // Left side: "❯ {command}" in dim gray (echoes the prompt indicator).
    let left = format!("❯ {}", command);
    // Right side: "{duration}  {icon}" colored by status.
    let right_plain = format!("{}  {}", duration_str, status_icon);
    let right = Style::new().fg(status_color).paint(&right_plain).to_string();

    // Right-align the status within term_width. Both `left` and `right_plain`
    // carry ANSI codes when colored; measure display width on the *plain*
    // text so the on-screen columns line up.
    let left_width = UnicodeWidthStr::width(left.as_str()) as usize;
    let right_width = UnicodeWidthStr::width(right_plain.as_str()) as usize;

    let w = term_width as usize;
    if w == 0 {
        // Width unknown — just join with a space.
        format!("{} {}", dim(&left), right)
    } else if left_width + right_width >= w {
        // Not enough room for both side by side — drop the padding so the
        // status follows on the same line tight against the command. If even
        // that overflows the terminal will wrap naturally; no panic.
        format!("{} {}", dim(&left), right)
    } else {
        let pad = w - left_width - right_width;
        format!("{}{}{}", dim(&left), " ".repeat(pad), right)
    }
}

/// Dim/gray styling for the echoed command + indicator (mirrors the prompt's
/// use of DarkGray for secondary text).
fn dim(s: &str) -> String {
    Style::new().fg(Color::DarkGray).paint(s).to_string()
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
    fn test_success_header_has_green_check() {
        let h = render_block_header("ls", 0, Duration::from_millis(12), 80);
        assert!(h.contains("✓"), "success header should contain ✓: {}", h);
        assert!(h.contains("ls"), "should echo the command");
        assert!(
            h.contains("\x1b[32m") || h.contains("\x1b[38;5;2m"),
            "status should be green-ish (ANSI 32 or 38;5;2): {}",
            h
        );
    }

    #[test]
    fn test_failure_header_has_red_cross() {
        let h = render_block_header("cat missing", 1, Duration::from_millis(3), 80);
        assert!(h.contains("✗"), "failure header should contain ✗: {}", h);
        assert!(
            h.contains("\x1b[31m") || h.contains("\x1b[38;5;1m"),
            "status should be red-ish (ANSI 31 or 38;5;1): {}",
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
    fn test_long_command_does_not_panic() {
        // A command far wider than any terminal — must not panic and must
        // still contain the command text and a status icon.
        let long = "x".repeat(500);
        let h = render_block_header(&long, 0, Duration::from_millis(1), 80);
        assert!(h.contains(&long));
        assert!(h.contains("✓"));
    }

    #[test]
    fn test_zero_width_unknown_terminal() {
        // term_width = 0 means "unknown" — header should still render with
        // both sides present (joined by a space), no padding math.
        let h = render_block_header("ls", 0, Duration::from_millis(5), 0);
        assert!(h.contains("❯ ls"));
        assert!(h.contains("✓"));
    }

    #[test]
    fn test_right_alignment_padding() {
        // With a wide terminal, there should be a run of spaces pushing the
        // status to the right edge.
        let h = render_block_header("ls", 0, Duration::from_millis(5), 100);
        // left "❯ ls" = 5 cols; right "5ms  ✓" = 7 cols → pad = 100-5-7 = 88
        assert!(h.contains(&" ".repeat(88)), "expected 88 spaces of padding");
    }
}
