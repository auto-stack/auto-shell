//! Plan 075 E4: AI 回合动态渲染 —— 回合状态(纯逻辑)。
//!
//! 一个 chat 回合期间,流式回复与工具事件按**到达序**累积到
//! [`TurnTailState`];渲染线程取 `visible_tail` 在尾部视口重绘,回合结束
//! `frozen_lines` 整体落线性转录。行分类 [`LineKind`] 同时驱动视口样式
//! (ratatui Style)与冻结打印的 ANSI 前缀,两处观感一致。

use std::sync::Mutex;

use ratatui_core::style::{Color, Modifier, Style};
use ratatui_core::text::{Line, Span};

use crate::frontend::tail::TailTerminal;

/// How many trailing lines the live preview shows (plus the status line).
pub const CHAT_VIEW_LINES: usize = 8;

/// Viewport height: 1 status + [`CHAT_VIEW_LINES`] + 1 margin.
pub const CHAT_TAIL_HEIGHT: u16 = 10;

/// Arrival-order classification of one display line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Streaming reply text (plain).
    Reply,
    /// Tool start/result, warnings — dimmed.
    Tool,
    /// Stream error — red.
    Error,
}

impl LineKind {
    fn ratatui_style(self) -> Style {
        match self {
            LineKind::Reply => Style::default().fg(Color::Gray),
            LineKind::Tool => Style::default().fg(Color::DarkGray),
            LineKind::Error => Style::default().fg(Color::Red),
        }
    }

    /// ANSI prefix for the frozen (linear) print; empty for plain reply.
    fn ansi_prefix(self) -> &'static str {
        match self {
            LineKind::Reply => "",
            LineKind::Tool => "\x1b[2m",
            LineKind::Error => "\x1b[31m",
        }
    }

    fn ansi_suffix(self) -> &'static str {
        if self.ansi_prefix().is_empty() {
            ""
        } else {
            "\x1b[0m"
        }
    }
}

/// State of one chat turn, updated by the stream-event callback (REPL
/// thread) and read by the render thread.
#[derive(Debug, Default)]
pub struct TurnTailState {
    lines: Vec<(String, LineKind)>,
    /// The current not-yet-newlined reply fragment (always `Reply` kind).
    partial: String,
    /// Total reply characters emitted (status line metric).
    chars: usize,
}

impl TurnTailState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Streaming reply delta: split on newlines — completed pieces become
    /// lines, the remainder stays partial until the next delta or flush.
    pub fn push_delta(&mut self, text: &str) {
        self.chars += text.chars().count();
        let mut pieces = text.split('\n');
        if let Some(first) = pieces.next() {
            self.partial.push_str(first);
        }
        for piece in pieces {
            let complete = std::mem::take(&mut self.partial);
            self.lines.push((complete, LineKind::Reply));
            self.partial.push_str(piece);
        }
    }

    /// A complete non-reply line (tool/warning/error): flushes the pending
    /// partial first so the arrival order is preserved.
    pub fn push_line(&mut self, text: impl Into<String>, kind: LineKind) {
        self.flush_partial();
        self.lines.push((text.into(), kind));
    }

    /// Flush the pending partial reply fragment as a complete line.
    pub fn flush_partial(&mut self) {
        if !self.partial.is_empty() {
            let complete = std::mem::take(&mut self.partial);
            self.lines.push((complete, LineKind::Reply));
        }
    }

    /// Total reply characters emitted so far.
    pub fn reply_chars(&self) -> usize {
        self.chars
    }

    /// Trailing lines (+ pending partial as the last line) for the live view.
    pub fn visible_tail(&self, n: usize) -> Vec<(String, LineKind)> {
        let mut v: Vec<(String, LineKind)> = self.lines.clone();
        if !self.partial.is_empty() {
            v.push((self.partial.clone(), LineKind::Reply));
        }
        if v.len() > n {
            v.split_off(v.len() - n)
        } else {
            v
        }
    }

    /// The complete arrival-order transcript for the frozen linear print.
    /// Flushes the partial into the returned list (call at turn end).
    pub fn frozen_lines(&mut self) -> Vec<(String, LineKind)> {
        self.flush_partial();
        std::mem::take(&mut self.lines)
    }
}

/// Render one live-preview frame of a chat turn: status line
/// (`⏳ AI · N 字 · elapsed`) + the trailing lines.
pub fn draw_chat_frame(
    terminal: &mut TailTerminal,
    state: &Mutex<TurnTailState>,
    start: std::time::Instant,
) -> std::io::Result<()> {
    let state = match state.lock() {
        Ok(s) => s,
        Err(poisoned) => poisoned.into_inner(),
    };
    let elapsed = start.elapsed().as_secs_f32();
    let status = format!(" ⏳ AI · {} 字 · {elapsed:.1}s", state.reply_chars());

    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        status,
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ))];
    for (text, kind) in state.visible_tail(CHAT_VIEW_LINES) {
        // Truncate over-wide lines so the viewport never wraps.
        let shown: String = text.chars().take(100).collect();
        lines.push(Line::from(Span::styled(
            format!(" {shown}"),
            kind.ratatui_style(),
        )));
    }
    terminal.draw(|f| {
        f.render_widget(ratatui_widgets::paragraph::Paragraph::new(lines), f.area());
    })?;
    Ok(())
}

/// Print the frozen transcript linearly (cooked mode, after the viewport
/// freeze): each line with its kind's ANSI styling.
pub fn print_frozen_turn(lines: &[(String, LineKind)]) {
    for (text, kind) in lines {
        println!("{}{text}{}", kind.ansi_prefix(), kind.ansi_suffix());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_accumulate_partial_then_complete_on_newline() {
        let mut s = TurnTailState::new();
        s.push_delta("Hel");
        s.push_delta("lo w");
        assert_eq!(s.visible_tail(10).len(), 1, "partial shows as one line");
        s.push_delta("orld\nsecond");
        let v = s.visible_tail(10);
        assert_eq!(v[0].0, "Hello world");
        assert_eq!(v[0].1, LineKind::Reply);
        assert_eq!(v[1].0, "second");
    }

    #[test]
    fn tool_lines_preserve_arrival_order_with_flush() {
        let mut s = TurnTailState::new();
        s.push_delta("thinking ");
        s.push_line("  ⚙ ls .", LineKind::Tool);
        s.push_delta("more reply");
        s.push_line("  ← ls: ok", LineKind::Tool);
        let frozen = s.frozen_lines();
        let texts: Vec<&str> = frozen.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            texts,
            ["thinking ", "  ⚙ ls .", "more reply", "  ← ls: ok"],
            "interleaved arrival order with partial flushed before tool lines"
        );
    }

    #[test]
    fn chars_counter_counts_all_deltas() {
        let mut s = TurnTailState::new();
        s.push_delta("abc");
        s.push_delta("de\nfg");
        // 3 + 5: the newline is a character too (emitted by the model).
        assert_eq!(s.reply_chars(), 8);
    }

    #[test]
    fn frozen_flushes_and_empties() {
        let mut s = TurnTailState::new();
        s.push_delta("tail");
        let frozen = s.frozen_lines();
        assert_eq!(frozen.len(), 1);
        assert_eq!(frozen[0].0, "tail");
        // State is consumed: a second freeze yields nothing.
        assert!(s.frozen_lines().is_empty());
    }

    #[test]
    fn visible_tail_bounded_and_includes_partial_last() {
        let mut s = TurnTailState::new();
        for i in 0..20 {
            s.push_line(format!("line{i}"), LineKind::Tool);
        }
        s.push_delta("partial!");
        let v = s.visible_tail(5);
        assert_eq!(v.len(), 5);
        assert_eq!(v[4].0, "partial!");
        assert_eq!(v[4].1, LineKind::Reply);
        assert_eq!(v[3].0, "line19");
    }

    #[test]
    fn error_kind_styles_differ() {
        let mut s = TurnTailState::new();
        s.push_line("boom", LineKind::Error);
        s.push_line("tool", LineKind::Tool);
        s.push_delta("reply");
        let frozen = s.frozen_lines();
        assert_eq!(frozen[0].1.ansi_prefix(), "\x1b[31m");
        assert_eq!(frozen[1].1.ansi_prefix(), "\x1b[2m");
        assert_eq!(frozen[2].1.ansi_prefix(), "");
    }

    #[test]
    fn empty_turn_is_empty() {
        let mut s = TurnTailState::new();
        assert!(s.visible_tail(8).is_empty());
        assert!(s.frozen_lines().is_empty());
    }
}
