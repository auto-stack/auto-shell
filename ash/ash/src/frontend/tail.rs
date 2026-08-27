//! 尾部租约(Plan 073 / 071 §6 E1)— 「线性输出 + 尾部动态」的统一机制。
//!
//! 核心不变式:线性转录永远固定、累计、可原生复制;尾部动态区只在"进行中"
//! 存在,完成即冻结回归线性。租约四段:
//!
//! 1. [`TailLease::erase_inline_row`] + [`TailLease::acquire`] — 占用尾部
//!    (擦除 reedline 刚提交的内联行,进 raw 模式,建 Inline viewport);
//! 2. [`TailLease::draw`] — 动态帧(消费者自渲染);
//! 3. [`TailLease::begin_freeze`] — 清视口行,冻结内容随后由消费者在
//!    cooked 模式线性打印进回滚区;
//! 4. `Drop` — 释放(恢复 cooked 模式;panic 展开同样生效)。
//!
//! 不进 alternate screen、不捕获鼠标 —— 原生复制全程可用。运行严格处于
//! 两次 reedline `read_line` 之间(reedline 在读间隙回 cooked 模式,终端
//! 所有权不重叠)。
//!
//! [`TailBuffer`] 是配套的纯逻辑有界行缓冲(keep-last-N + 溢出计数),
//! 为 E2(运行中命令动态块)等消费者预置数据结构。

use std::collections::VecDeque;
use std::io::{self, Stdout};

use ratatui_core::layout::Rect;
use ratatui_core::terminal::{Terminal, TerminalOptions, Viewport};
use ratatui_crossterm::crossterm::cursor::{MoveTo, MoveToColumn, MoveUp};
use ratatui_crossterm::crossterm::execute;
use ratatui_crossterm::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType,
};

/// The Inline-viewport terminal a lease renders into.
pub type TailTerminal = Terminal<ratatui_crossterm::CrosstermBackend<Stdout>>;

/// A lease on the dynamic tail of the terminal.
///
/// Acquire between two reedline `read_line` calls; render dynamic frames with
/// [`Self::draw`]; when the work completes, call [`Self::begin_freeze`] and
/// let the lease drop, then print the frozen content linearly (plain
/// `println!` — cooked mode) starting where the viewport was. Dropping
/// without `begin_freeze` (error paths) still restores the terminal; the
/// viewport rows may hold the last drawn frame (acceptable for aborted work).
pub struct TailLease {
    terminal: TailTerminal,
    /// Field order matters: `_raw` drops after `terminal`.
    _raw: RawGuard,
}

impl TailLease {
    /// Erase the just-submitted inline input row (the prompt line carrying
    /// only the invisible mode marker) so the viewport anchors where it was.
    ///
    /// Must run before anything prints between the reedline submit and the
    /// [`Self::acquire`] — otherwise the wrong row gets erased. Wrapped-line
    /// inputs leave residue beyond one row (accepted v1, 070 precedent).
    pub fn erase_inline_row() -> io::Result<()> {
        execute!(
            io::stdout(),
            MoveUp(1),
            Clear(ClearType::CurrentLine),
            MoveToColumn(0),
        )
    }

    /// Enter raw mode and create an Inline viewport of `height` rows.
    ///
    /// The height is fixed at creation — ratatui Inline viewports cannot
    /// resize without rebuilding the Terminal (029 §5.1); consumers scroll
    /// inside their widget instead.
    pub fn acquire(height: u16) -> io::Result<Self> {
        enable_raw_mode()?;
        let backend = ratatui_crossterm::CrosstermBackend::new(io::stdout());
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Inline(height) },
        )?;
        Ok(Self { terminal, _raw: RawGuard })
    }

    /// Render one frame; returns the drawn area (for hardware-cursor math).
    pub fn draw(
        &mut self,
        f: impl FnOnce(&mut ratatui_core::terminal::Frame),
    ) -> io::Result<Rect> {
        let mut area = Rect::default();
        self.terminal.draw(|frame| {
            area = frame.area();
            f(frame);
        })?;
        Ok(area)
    }

    /// Begin freezing: move the cursor to the viewport's top row and clear
    /// from there down (the rows below were already overwritten by draws).
    /// The caller then prints the frozen content linearly starting at that
    /// row, so no blank gap remains between the transcript and the content.
    pub fn begin_freeze(&mut self) {
        freeze_viewport(&mut self.terminal);
    }

    /// The leased terminal (consumers render widgets through it when they
    /// need more than [`Self::draw`]'s area, e.g. textarea cursor state).
    pub fn terminal(&mut self) -> &mut TailTerminal {
        &mut self.terminal
    }

    /// Plan 074: split the lease — the terminal moves to the render thread
    /// (single-writer discipline: the REPL thread is blocked inside
    /// `Shell::execute` while the tail runs and never touches it), the
    /// raw-mode guard stays with the caller and restores cooked mode on
    /// drop. The render thread calls [`freeze_viewport`] before exiting.
    pub fn into_parts(self) -> (TailTerminal, RawGuard) {
        (self.terminal, self._raw)
    }
}

/// The raw-mode half of a [`TailLease`] (`into_parts`). Restores cooked mode
/// on drop, including panic unwinding.
pub struct RawGuard;

impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

/// Freezing for a terminal handed to a render thread via
/// [`TailLease::into_parts`]: cursor to the viewport's top row, clear to the
/// end of screen — the frozen content is then printed linearly from there.
pub fn freeze_viewport(terminal: &mut TailTerminal) {
    let area = terminal.get_frame().area();
    let _ = execute!(
        io::stdout(),
        MoveTo(0, area.y),
        Clear(ClearType::FromCursorDown),
    );
}

/// Bounded line buffer for dynamic tail views (E2 consumers): keeps the last
/// `capacity` lines plus overflow accounting, so the live view shows the
/// tail while the frozen summary can say how much scrolled past.
#[derive(Debug, Clone)]
pub struct TailBuffer {
    lines: VecDeque<String>,
    capacity: usize,
    /// Lines dropped from the front (scrolled past the window).
    dropped: usize,
    /// Total lines ever pushed.
    total: usize,
}

impl TailBuffer {
    pub fn new(capacity: usize) -> Self {
        TailBuffer {
            lines: VecDeque::new(),
            capacity: capacity.max(1),
            dropped: 0,
            total: 0,
        }
    }

    /// Append a line; once full, the oldest line scrolls out and is counted
    /// in `dropped`.
    pub fn push(&mut self, line: impl Into<String>) {
        let line = line.into();
        self.total += 1;
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
            self.dropped += 1;
        }
        self.lines.push_back(line);
    }

    /// The visible (kept) lines, oldest first.
    pub fn lines(&self) -> &VecDeque<String> {
        &self.lines
    }

    /// Lines that scrolled past the window.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Total lines ever pushed.
    pub fn total(&self) -> usize {
        self.total
    }

    /// Summary header for the frozen view: `None` when nothing scrolled past
    /// (the full content is visible); otherwise `…前 K 行未展示(共 N 行)`.
    pub fn header_note(&self) -> Option<String> {
        if self.dropped == 0 {
            None
        } else {
            Some(format!("…前 {} 行未展示(共 {} 行)", self.dropped, self.total))
        }
    }

    /// Reset for reuse (same capacity).
    pub fn clear(&mut self) {
        self.lines.clear();
        self.dropped = 0;
        self.total = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_capacity_keeps_everything() {
        let mut b = TailBuffer::new(5);
        for i in 1..=3 {
            b.push(format!("line{i}"));
        }
        assert_eq!(b.lines().len(), 3);
        assert_eq!(b.dropped(), 0);
        assert_eq!(b.total(), 3);
        assert_eq!(b.header_note(), None);
        assert_eq!(b.lines().front().unwrap(), "line1");
        assert_eq!(b.lines().back().unwrap(), "line3");
    }

    #[test]
    fn overflow_keeps_tail_and_counts_dropped() {
        let mut b = TailBuffer::new(2);
        for i in 1..=5 {
            b.push(format!("line{i}"));
        }
        // Keeps the LAST 2; 3 scrolled past; 5 total.
        assert_eq!(b.lines().iter().map(String::as_str).collect::<Vec<_>>(), ["line4", "line5"]);
        assert_eq!(b.dropped(), 3);
        assert_eq!(b.total(), 5);
        assert_eq!(b.header_note().unwrap(), "…前 3 行未展示(共 5 行)");
    }

    #[test]
    fn capacity_zero_is_clamped_to_one() {
        let mut b = TailBuffer::new(0);
        b.push("a");
        b.push("b");
        assert_eq!(b.lines().len(), 1);
        assert_eq!(b.lines().front().unwrap(), "b");
        assert_eq!(b.dropped(), 1);
    }

    #[test]
    fn empty_has_no_header() {
        let b = TailBuffer::new(4);
        assert_eq!(b.total(), 0);
        assert_eq!(b.header_note(), None);
    }

    #[test]
    fn clear_resets_but_keeps_capacity() {
        let mut b = TailBuffer::new(2);
        for i in 1..=4 {
            b.push(format!("x{i}"));
        }
        b.clear();
        assert_eq!(b.total(), 0);
        assert_eq!(b.dropped(), 0);
        assert!(b.lines().is_empty());
        b.push("y");
        assert_eq!(b.lines().front().unwrap(), "y");
        // capacity still 2: three pushes drop one.
        b.push("z");
        b.push("w");
        assert_eq!(b.dropped(), 1);
    }

    #[test]
    fn exactly_full_has_no_header() {
        let mut b = TailBuffer::new(3);
        for i in 1..=3 {
            b.push(format!("l{i}"));
        }
        assert_eq!(b.header_note(), None, "full-but-not-overflowing is fully visible");
    }
}
