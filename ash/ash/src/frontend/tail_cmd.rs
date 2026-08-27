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
}
