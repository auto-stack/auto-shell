//! Plan 074 E2: 运行中命令动态块 —— 资格判定 + 视口渲染。
//!
//! 单条外部命令(无管道/重定向/复合/展开等 shell 结构)在 REPL 运行期间,
//! 其输出进入尾部动态区限高滚动预览(`crate::frontend::tail` 的租约);
//! 完成后冻结为与既有路径完全一致的线性转录(块头 + 全文)。

use std::io;
use std::sync::Mutex;
use std::time::Instant;

use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::text::{Line, Span};
use ratatui_widgets::paragraph::Paragraph;

use crate::frontend::tail::{TailBuffer, TailTerminal};

/// Viewport height: 1 status + up to [`VIEW_LINES`] preview lines + margins.
pub const TAIL_HEIGHT: u16 = 10;

/// How many trailing output lines the live preview shows.
pub const VIEW_LINES: usize = 7;

/// Characters that mean the SHELL itself must parse the line (pipes,
/// redirects, chaining, substitution, expansion). Lines containing any of
/// them fall back to the normal execution path — the tail preview only
/// covers plain single commands (v1 scope; see plan 074 §4).
const TAIL_BLOCKERS: &[char] = &['|', ';', '&', '>', '<', '`', '$', '(', ')', '\n', '\r'];

/// Pure eligibility check on the LINE SHAPE only (no shell/registry lookup —
/// callers combine with `classify_is_external_pub` + the interactive list).
pub fn tail_eligible_line(input: &str) -> bool {
    let t = input.trim();
    !t.is_empty() && !t.chars().any(|c| TAIL_BLOCKERS.contains(&c))
}

/// Short label for the status line: first three words of the command.
pub fn status_label(command: &str) -> String {
    command
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Plan 076 E5: freeze policy for long outputs ─────────────────────────────

/// Outputs at or under this many lines freeze verbatim; longer ones freeze
/// as a head+tail excerpt with the full text spilled to a temp file.
/// Override with `ASH_TAIL_FREEZE_MAX`.
pub const DEFAULT_FREEZE_MAX_LINES: usize = 100;
/// Head lines kept in the summary excerpt.
pub const SUMMARY_HEAD_LINES: usize = 8;
/// Tail lines kept in the summary excerpt.
pub const SUMMARY_TAIL_LINES: usize = 24;

pub fn freeze_max_lines() -> usize {
    std::env::var("ASH_TAIL_FREEZE_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_FREEZE_MAX_LINES)
}

/// How a completed command's output freezes into the linear transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreezeText {
    /// Within the limit — freeze verbatim.
    Full,
    /// Over the limit — freeze this excerpt + stats; spill the full text.
    Summary {
        excerpt: String,
        total_lines: usize,
        total_bytes: usize,
    },
}

impl FreezeText {
    pub fn is_summary(&self) -> bool {
        matches!(self, FreezeText::Summary { .. })
    }
}

/// Decide the freeze form for `output` (trailing newline insensitive).
pub fn build_freeze_text(output: &str) -> FreezeText {
    let total_lines = output.lines().count();
    if total_lines <= freeze_max_lines() {
        return FreezeText::Full;
    }
    // Clamp head/tail to the actual size — a small ASH_TAIL_FREEZE_MAX must
    // not underflow the omitted-lines math.
    let head_n = SUMMARY_HEAD_LINES.min(total_lines);
    let tail_n = SUMMARY_TAIL_LINES.min(total_lines - head_n);
    let omitted = total_lines - head_n - tail_n;
    let head: Vec<&str> = output.lines().take(head_n).collect();
    let tail: Vec<&str> = output.lines().skip(total_lines - tail_n).collect();
    let omitted_line = if omitted > 0 {
        format!("\n…已省略 {omitted} 行…")
    } else {
        String::new()
    };
    let excerpt = format!(
        "{}{omitted_line}\n…(共 {total_lines} 行 · {} 字节)…\n{}",
        head.join("\n"),
        output.len(),
        tail.join("\n"),
    );
    FreezeText::Summary {
        excerpt,
        total_lines,
        total_bytes: output.len(),
    }
}

/// Spill the full output to a temp file (`ash-freeze-<millis>.log`); the
/// frozen summary points here so nothing is lost. Files are not actively
/// cleaned (OS temp lifecycle, DEBTS-noted).
pub fn spill_full_output(output: &str) -> io::Result<std::path::PathBuf> {
    let name = format!(
        "ash-freeze-{}.log",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, output)?;
    Ok(path)
}

/// Render one live-preview frame: a status line (`⏳ cmd · elapsed · lines`)
/// followed by the buffer's trailing lines (oldest of the kept window first).
pub fn draw_tail_frame(
    terminal: &mut TailTerminal,
    buffer: &Mutex<TailBuffer>,
    label: &str,
    start: Instant,
) -> io::Result<()> {
    let buf = match buffer.lock() {
        Ok(b) => b.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let elapsed = start.elapsed().as_secs_f32();
    let status = format!(" ⏳ {label} · {elapsed:.1}s · {} 行", buf.total());

    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        status,
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ))];
    if let Some(note) = buf.header_note() {
        lines.push(Line::from(Span::styled(
            format!(" {note}"),
            Style::default().fg(Color::DarkGray),
        )));
    }
    for l in buf.lines() {
        lines.push(Line::from(Span::styled(
            format!(" {l}"),
            Style::default().fg(Color::Gray),
        )));
    }
    terminal.draw(|f| {
        f.render_widget(Paragraph::new(lines), f.area());
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_commands_are_eligible() {
        assert!(tail_eligible_line("cargo build"));
        assert!(tail_eligible_line("ping -c 4 example.com"));
        assert!(tail_eligible_line("  git status  "));
    }

    #[test]
    fn shell_structures_block_the_tail() {
        for line in [
            "ls | head -n 2",
            "echo a > f.txt",
            "cd .. && cargo build",
            "cat a; cat b",
            "echo $HOME",
            "echo `date`",
            "(cargo build)",
            "cargo build\ncargo test",
            "",
            "   ",
        ] {
            assert!(!tail_eligible_line(line), "should block: {line:?}");
        }
    }

    #[test]
    fn label_takes_first_three_words() {
        assert_eq!(status_label("cargo build --release --quiet"), "cargo build --release");
        assert_eq!(status_label("ping"), "ping");
    }

    // ── Plan 076 E5: freeze policy ───────────────────────────────────

    #[test]
    fn within_limit_freezes_full() {
        let text: String = (0..50).map(|i| format!("line{i}\n")).collect();
        assert_eq!(build_freeze_text(&text), FreezeText::Full);
        // Exactly at the limit is still full.
        let at_limit: String = (0..DEFAULT_FREEZE_MAX_LINES).map(|i| format!("l{i}\n")).collect();
        assert_eq!(build_freeze_text(&at_limit), FreezeText::Full);
    }

    #[test]
    fn empty_output_is_full() {
        assert_eq!(build_freeze_text(""), FreezeText::Full);
    }

    #[test]
    fn over_limit_builds_head_omitted_tail_summary() {
        let n = DEFAULT_FREEZE_MAX_LINES + 50;
        let text: String = (0..n).map(|i| format!("row{i}\n")).collect();
        let FreezeText::Summary { excerpt, total_lines, total_bytes } =
            build_freeze_text(&text)
        else {
            panic!("expected summary");
        };
        assert_eq!(total_lines, n);
        assert_eq!(total_bytes, text.len());
        assert!(excerpt.contains("row0"), "head kept: {excerpt}");
        assert!(excerpt.contains(&format!("row{}", SUMMARY_HEAD_LINES - 1)));
        assert!(!excerpt.contains(&format!("row{SUMMARY_HEAD_LINES}")), "middle dropped");
        assert!(excerpt.contains(&format!("row{}", n - 1)), "tail kept: {excerpt}");
        let omitted = n - SUMMARY_HEAD_LINES - SUMMARY_TAIL_LINES;
        assert!(excerpt.contains(&format!("已省略 {omitted} 行")));
        assert!(excerpt.contains(&format!("共 {n} 行")));
    }

    #[test]
    fn env_override_changes_threshold() {
        // Scoped env mutation — serial among tests using it (fine: unique var).
        std::env::set_var("ASH_TAIL_FREEZE_MAX", "3");
        let four = "a\nb\nc\nd\n";
        assert!(build_freeze_text(four).is_summary());
        std::env::remove_var("ASH_TAIL_FREEZE_MAX");
        assert_eq!(build_freeze_text(four), FreezeText::Full);
    }

    #[test]
    fn spill_writes_readable_unique_files() {
        let a = spill_full_output("hello-freeze").expect("spill a");
        let b = spill_full_output("hello-freeze").expect("spill b");
        assert_ne!(a, b, "millis-unique names");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "hello-freeze");
        assert!(a.file_name().unwrap().to_string_lossy().starts_with("ash-freeze-"));
        let _ = std::fs::remove_file(a);
        let _ = std::fs::remove_file(b);
    }
}
