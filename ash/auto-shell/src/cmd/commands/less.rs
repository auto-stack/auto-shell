//! `less` / `more` command — interactive file pager.
//!
//! Reads from a file or pipeline and displays content one screen at a time
//! with keyboard navigation (vim-style + arrow keys), search, and a status
//! line.  Takes over the terminal (alternate screen + raw mode) while active
//! and restores everything on exit.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use miette::{IntoDiagnostic, Result};

use crate::cmd::parser::ParsedArgs;
use crate::cmd::{Command, PipelineData, Signature};
use crate::shell::Shell;

// ── RAII terminal guards ────────────────────────────────────────────

pub(crate) struct RawModeGuard;

impl RawModeGuard {
    pub(crate) fn enter() -> Result<Self> {
        terminal::enable_raw_mode().into_diagnostic()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

pub(crate) struct AltScreenGuard;

impl AltScreenGuard {
    pub(crate) fn enter() -> Result<Self> {
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen).into_diagnostic()?;
        Ok(Self)
    }
}

impl Drop for AltScreenGuard {
    fn drop(&mut self) {
        let mut stdout = std::io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
        let _ = execute!(stdout, cursor::Show);
    }
}

// ── Pager ───────────────────────────────────────────────────────────

struct Pager {
    lines: Vec<String>,
    scroll: usize, // first visible line index
    rows: u16,     // terminal height
    cols: u16,     // terminal width
    /// Current search term (empty = no active search).
    search_pattern: String,
    /// Line indices that match the current search pattern.
    search_hits: Vec<usize>,
    /// Index into `search_hits` for the "current" highlighted match.
    search_idx: usize,
}

impl Pager {
    fn new(lines: Vec<String>) -> Result<Self> {
        let (cols, rows) = terminal::size().into_diagnostic()?;
        Ok(Self {
            lines,
            scroll: 0,
            rows,
            cols,
            search_pattern: String::new(),
            search_hits: Vec::new(),
            search_idx: 0,
        })
    }

    /// Number of rows available for content (excluding 1 status line).
    fn page_rows(&self) -> usize {
        (self.rows.saturating_sub(1) as usize).max(1)
    }

    /// Maximum valid scroll offset.
    fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(self.page_rows())
    }

    // ── rendering ────────────────────────────────────────────────

    fn render(&self) -> Result<()> {
        let mut stdout = std::io::stdout();
        let page = self.page_rows();
        let end = (self.scroll + page).min(self.lines.len());

        // Clear and draw content area.
        execute!(stdout, cursor::MoveTo(0, 0)).into_diagnostic()?;

        for i in self.scroll..end {
            // Highlight the current search hit.
            let is_search_hit = !self.search_pattern.is_empty()
                && self.search_hits.get(self.search_idx) == Some(&i);

            let line = &self.lines[i];
            let display = truncate_to_width(line, self.cols as usize);
            if is_search_hit {
                execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;
                write!(stdout, "{}", display).into_diagnostic()?;
                execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
            } else {
                write!(stdout, "{}", display).into_diagnostic()?;
            }

            // Clear the rest of this line (shorter content may leave old
            // characters from a previously-rendered longer line).
            execute!(stdout, crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine)).into_diagnostic()?;
            execute!(stdout, crossterm::style::ResetColor).into_diagnostic()?;

            // In raw mode `\n` does a line-feed *without* carriage-return,
            // so the cursor would drift right on each line.  Use `\r\n`.
            if i < end - 1 {
                write!(stdout, "\r\n").into_diagnostic()?;
            }
        }

        // Clear remaining rows if content is shorter than page.
        for _ in end.saturating_sub(self.scroll)..page {
            execute!(stdout, cursor::MoveToNextLine(1)).into_diagnostic()?;
            // Clear the line.
            write!(stdout, "\r").into_diagnostic()?;
            execute!(stdout, crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine)).into_diagnostic()?;
        }

        // Status line.
        self.render_status()?;

        stdout.flush().into_diagnostic()?;
        Ok(())
    }

    fn render_status(&self) -> Result<()> {
        let mut stdout = std::io::stdout();
        let bottom = self.rows.saturating_sub(1);
        execute!(stdout, cursor::MoveTo(0, bottom)).into_diagnostic()?;

        // Reverse-video status bar.
        execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;

        let total = self.lines.len();
        let start = if total == 0 { 0 } else { self.scroll + 1 };
        let page = self.page_rows();
        let end = (self.scroll + page).min(total);
        let pct = if total > 0 {
            (end * 100) / total
        } else {
            0
        };

        let status = if !self.search_pattern.is_empty() {
            let cur = if self.search_hits.is_empty() {
                0
            } else {
                self.search_idx + 1
            };
            format!(
                "lines {}-{}/{}  ({}%)  search: \"{}\"  [{}/{}]  q=quit",
                start,
                end,
                total,
                pct,
                self.search_pattern,
                cur,
                self.search_hits.len(),
            )
        } else if total == 0 {
            "(empty)  q=quit".to_string()
        } else {
            format!(
                "lines {}-{}/{}  ({}%)  q=quit  /=search",
                start, end, total, pct,
            )
        };

        // Pad/truncate to terminal width.
        let w = self.cols as usize;
        if status.len() < w {
            write!(stdout, "{: <width$}", status, width = w).into_diagnostic()?;
        } else {
            write!(stdout, "{}", &status[..w]).into_diagnostic()?;
        }

        execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
        Ok(())
    }

    // ── search ────────────────────────────────────────────────────

    /// Rebuild search hits for the current pattern.
    fn update_search(&mut self) {
        self.search_hits.clear();
        self.search_idx = 0;
        if self.search_pattern.is_empty() {
            return;
        }
        let pat = self.search_pattern.to_lowercase();
        for (i, line) in self.lines.iter().enumerate() {
            if line.to_lowercase().contains(&pat) {
                self.search_hits.push(i);
            }
        }
    }

    /// Jump to the next search hit after the current viewport.
    fn next_search_hit(&mut self) {
        if self.search_hits.is_empty() {
            return;
        }
        self.search_idx = (self.search_idx + 1) % self.search_hits.len();
        self.scroll_to_hit();
    }

    /// Jump to the previous search hit.
    fn prev_search_hit(&mut self) {
        if self.search_hits.is_empty() {
            return;
        }
        self.search_idx = self
            .search_idx
            .checked_sub(1)
            .unwrap_or(self.search_hits.len() - 1);
        self.scroll_to_hit();
    }

    fn scroll_to_hit(&mut self) {
        if let Some(&line_idx) = self.search_hits.get(self.search_idx) {
            let page = self.page_rows();
            if line_idx < self.scroll || line_idx >= self.scroll + page {
                self.scroll = line_idx.saturating_sub(page / 2);
                self.clamp_scroll();
            }
        }
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    // ── event loop ────────────────────────────────────────────────

    fn run(&mut self) -> Result<()> {
        // Hide cursor.
        execute!(std::io::stdout(), cursor::Hide).into_diagnostic()?;

        self.render()?;

        // Input buffer for search prompt.
        let mut search_buf = String::new();
        // Whether we're in search-input mode.
        let mut searching = false;

        loop {
            match event::read().into_diagnostic()? {
                Event::Key(KeyEvent { code, modifiers, .. }) => {
                    // Ctrl+C always exits.
                    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                        break;
                    }

                    if searching {
                        match code {
                            KeyCode::Esc => {
                                // Cancel search.
                                searching = false;
                                search_buf.clear();
                            }
                            KeyCode::Enter => {
                                // Execute search.
                                searching = false;
                                self.search_pattern = search_buf.clone();
                                search_buf.clear();
                                self.update_search();
                                if !self.search_hits.is_empty() {
                                    self.search_idx = 0;
                                    self.scroll_to_hit();
                                }
                            }
                            KeyCode::Backspace => {
                                search_buf.pop();
                            }
                            KeyCode::Char(c) => {
                                search_buf.push(c);
                            }
                            _ => {}
                        }
                        // Show search prompt in status area while typing.
                        if searching {
                            let mut status_text = format!("/{}", search_buf);
                            if !self.search_pattern.is_empty() {
                                status_text.push_str(&format!(
                                    "  (prev: \"{}\" [{}/{}])",
                                    self.search_pattern,
                                    if self.search_hits.is_empty() { 0 } else { self.search_idx + 1 },
                                    self.search_hits.len(),
                                ));
                            }
                            let mut stdout = std::io::stdout();
                            let bottom = self.rows.saturating_sub(1);
                            execute!(stdout, cursor::MoveTo(0, bottom)).into_diagnostic()?;
                            execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;
                            write!(stdout, "{: <width$}", status_text, width = self.cols as usize).into_diagnostic()?;
                            execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
                            stdout.flush().into_diagnostic()?;
                        } else {
                            self.render()?;
                        }
                        continue;
                    }

                    match code {
                        KeyCode::Char('q') | KeyCode::Esc => break,

                        // Scroll down one line.
                        KeyCode::Char('j') | KeyCode::Down => {
                            self.scroll += 1;
                            self.clamp_scroll();
                        }
                        // Scroll up one line.
                        KeyCode::Char('k') | KeyCode::Up => {
                            self.scroll = self.scroll.saturating_sub(1);
                        }
                        // Page down.
                        KeyCode::Char(' ') | KeyCode::Char('f') | KeyCode::PageDown => {
                            self.scroll += self.page_rows();
                            self.clamp_scroll();
                        }
                        // Page up.
                        KeyCode::Char('b') | KeyCode::PageUp => {
                            self.scroll = self.scroll.saturating_sub(self.page_rows());
                        }
                        // Go to top.
                        KeyCode::Char('g') | KeyCode::Home => {
                            self.scroll = 0;
                        }
                        // Go to bottom.
                        KeyCode::Char('G') | KeyCode::End => {
                            self.scroll = self.max_scroll();
                        }
                        // Begin search.
                        KeyCode::Char('/') => {
                            searching = true;
                            search_buf.clear();
                            // Immediately show search prompt.
                            let mut stdout = std::io::stdout();
                            let bottom = self.rows.saturating_sub(1);
                            execute!(stdout, cursor::MoveTo(0, bottom)).into_diagnostic()?;
                            execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;
                            let prompt = if self.search_pattern.is_empty() {
                                "/".to_string()
                            } else {
                                format!("/ (prev: \"{}\")", self.search_pattern)
                            };
                            write!(stdout, "{: <width$}", prompt, width = self.cols as usize).into_diagnostic()?;
                            execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
                            stdout.flush().into_diagnostic()?;
                            continue;
                        }
                        // Next search match.
                        KeyCode::Char('n') => {
                            self.next_search_hit();
                        }
                        // Previous search match.
                        KeyCode::Char('N') => {
                            self.prev_search_hit();
                        }
                        _ => {}
                    }
                    self.render()?;
                }
                Event::Resize(cols, rows) => {
                    self.cols = cols;
                    self.rows = rows;
                    self.clamp_scroll();
                    self.render()?;
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// ── CodePager: syntax-highlighting pager with lazy/cached rendering ─

/// A pager that lazily syntax-highlights only the lines being viewed.
/// Used by `show --pager file.rs` so the first screen appears in
/// milliseconds regardless of file size.
pub(crate) struct CodePager {
    /// Raw (un-highlighted) file lines.
    lines: Vec<String>,
    /// File extension, for syntax highlighting.
    ext: String,
    /// Cache: line index → highlighted ANSI string.
    cache: std::collections::HashMap<usize, String>,
    scroll: usize,
    rows: u16,
    cols: u16,
    search_pattern: String,
    search_hits: Vec<usize>,
    search_idx: usize,
}

impl CodePager {
    pub(crate) fn new(lines: Vec<String>, ext: String) -> Result<Self> {
        let (cols, rows) = terminal::size().into_diagnostic()?;
        Ok(Self {
            lines,
            ext,
            cache: std::collections::HashMap::new(),
            scroll: 0,
            rows,
            cols,
            search_pattern: String::new(),
            search_hits: Vec::new(),
            search_idx: 0,
        })
    }

    fn page_rows(&self) -> usize {
        (self.rows.saturating_sub(1) as usize).max(1)
    }

    fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(self.page_rows())
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    /// Return the highlighted version of `lines[idx]`, computing and
    /// caching it on first access.
    fn highlighted(&mut self, idx: usize) -> &str {
        if !self.cache.contains_key(&idx) {
            let raw = self.lines[idx].as_str();
            let hl = super::code_highlight::highlight_code(raw, &self.ext);
            self.cache.insert(idx, hl);
        }
        // Safe: we just ensured the key exists.
        self.cache.get(&idx).unwrap().as_str()
    }

    // ── search ──
    fn update_search(&mut self) {
        self.search_hits.clear();
        self.search_idx = 0;
        if self.search_pattern.is_empty() {
            return;
        }
        let pat = self.search_pattern.to_lowercase();
        for (i, line) in self.lines.iter().enumerate() {
            if line.to_lowercase().contains(&pat) {
                self.search_hits.push(i);
            }
        }
    }

    fn next_search_hit(&mut self) {
        if self.search_hits.is_empty() {
            return;
        }
        self.search_idx = (self.search_idx + 1) % self.search_hits.len();
        self.scroll_to_hit();
    }

    fn prev_search_hit(&mut self) {
        if self.search_hits.is_empty() {
            return;
        }
        self.search_idx = self
            .search_idx
            .checked_sub(1)
            .unwrap_or(self.search_hits.len() - 1);
        self.scroll_to_hit();
    }

    fn scroll_to_hit(&mut self) {
        if let Some(&line_idx) = self.search_hits.get(self.search_idx) {
            let page = self.page_rows();
            if line_idx < self.scroll || line_idx >= self.scroll + page {
                self.scroll = line_idx.saturating_sub(page / 2);
                self.clamp_scroll();
            }
        }
    }

    // ── rendering ──
    fn render(&mut self) -> Result<()> {
        let mut stdout = std::io::stdout();
        let page = self.page_rows();
        let end = (self.scroll + page).min(self.lines.len());

        execute!(stdout, cursor::MoveTo(0, 0)).into_diagnostic()?;

        for i in self.scroll..end {
            let is_search_hit = !self.search_pattern.is_empty()
                && self.search_hits.get(self.search_idx) == Some(&i);

            let cols = self.cols as usize;
            let display = truncate_to_width(self.highlighted(i), cols);
            if is_search_hit {
                execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;
                write!(stdout, "{}", display).into_diagnostic()?;
                execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
            } else {
                write!(stdout, "{}", display).into_diagnostic()?;
            }
            execute!(stdout, crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine)).into_diagnostic()?;
            execute!(stdout, crossterm::style::ResetColor).into_diagnostic()?;
            if i < end - 1 {
                write!(stdout, "\r\n").into_diagnostic()?;
            }
        }

        // Clear any remaining rows.
        for row in (end - self.scroll)..page {
            let _ = row;
            execute!(stdout, cursor::MoveToNextLine(1)).into_diagnostic()?;
            write!(stdout, "\r").into_diagnostic()?;
            execute!(stdout, crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine)).into_diagnostic()?;
        }

        // Status line.
        self.render_status()?;
        stdout.flush().into_diagnostic()?;
        Ok(())
    }

    fn render_status(&self) -> Result<()> {
        let mut stdout = std::io::stdout();
        let bottom = self.rows.saturating_sub(1);
        execute!(stdout, cursor::MoveTo(0, bottom)).into_diagnostic()?;
        execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;

        let total = self.lines.len();
        let start = if total == 0 { 0 } else { self.scroll + 1 };
        let page = self.page_rows();
        let end = (self.scroll + page).min(total);
        let pct = if total > 0 { (end * 100) / total } else { 0 };

        let status = if !self.search_pattern.is_empty() {
            let cur = if self.search_hits.is_empty() { 0 } else { self.search_idx + 1 };
            format!(
                "lines {}-{}/{}  ({}%)  search: \"{}\"  [{}/{}]  q=quit",
                start, end, total, pct, self.search_pattern, cur, self.search_hits.len(),
            )
        } else if total == 0 {
            "(empty)  q=quit".to_string()
        } else {
            format!("lines {}-{}/{}  ({}%)  q=quit  /=search", start, end, total, pct,)
        };

        let w = self.cols as usize;
        if status.len() < w {
            write!(stdout, "{: <width$}", status, width = w).into_diagnostic()?;
        } else {
            write!(stdout, "{}", &status[..w]).into_diagnostic()?;
        }
        execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
        Ok(())
    }

    // ── event loop ──
    pub(crate) fn run(&mut self) -> Result<()> {
        execute!(std::io::stdout(), cursor::Hide).into_diagnostic()?;
        self.render()?;

        let mut search_buf = String::new();
        let mut searching = false;

        loop {
            match event::read().into_diagnostic()? {
                Event::Key(KeyEvent { code, modifiers, .. }) => {
                    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                        break;
                    }
                    if searching {
                        match code {
                            KeyCode::Esc => {
                                searching = false;
                                search_buf.clear();
                            }
                            KeyCode::Enter => {
                                searching = false;
                                self.search_pattern = search_buf.clone();
                                search_buf.clear();
                                self.update_search();
                                if !self.search_hits.is_empty() {
                                    self.search_idx = 0;
                                    self.scroll_to_hit();
                                }
                            }
                            KeyCode::Backspace => {
                                search_buf.pop();
                            }
                            KeyCode::Char(c) => {
                                search_buf.push(c);
                            }
                            _ => {}
                        }
                        if searching {
                            let mut s = std::io::stdout();
                            let bottom = self.rows.saturating_sub(1);
                            execute!(s, cursor::MoveTo(0, bottom)).into_diagnostic()?;
                            execute!(s, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;
                            write!(s, "/{: <width$}", search_buf, width = self.cols as usize).into_diagnostic()?;
                            execute!(s, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
                            s.flush().into_diagnostic()?;
                        } else {
                            self.render()?;
                        }
                        continue;
                    }

                    match code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('j') | KeyCode::Down => {
                            self.scroll += 1;
                            self.clamp_scroll();
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            self.scroll = self.scroll.saturating_sub(1);
                        }
                        KeyCode::Char(' ') | KeyCode::Char('f') | KeyCode::PageDown => {
                            self.scroll += self.page_rows();
                            self.clamp_scroll();
                        }
                        KeyCode::Char('b') | KeyCode::PageUp => {
                            self.scroll = self.scroll.saturating_sub(self.page_rows());
                        }
                        KeyCode::Char('g') | KeyCode::Home => self.scroll = 0,
                        KeyCode::Char('G') | KeyCode::End => self.scroll = self.max_scroll(),
                        KeyCode::Char('/') => {
                            searching = true;
                            search_buf.clear();
                        }
                        KeyCode::Char('n') => self.next_search_hit(),
                        KeyCode::Char('N') => self.prev_search_hit(),
                        _ => {}
                    }
                    self.render()?;
                }
                Event::Resize(cols, rows) => {
                    self.cols = cols;
                    self.rows = rows;
                    self.clamp_scroll();
                    self.render()?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

// ── StreamingPager: consumes ExternalStream with background buffering ─

/// A pager that reads lines incrementally from an OS pipe while the user
/// interacts.  A background thread fills `lines`; the main thread renders
/// whatever has arrived so far.  This makes `show big.rs | less` show the
/// first page in milliseconds instead of waiting for the whole file.
pub(crate) struct StreamingPager {
    /// Shared line buffer, filled by background reader thread.
    lines: Arc<Mutex<Vec<String>>>,
    /// Set by the reader thread when the stream is exhausted.
    done: Arc<AtomicBool>,
    scroll: usize,
    rows: u16,
    cols: u16,
    search_pattern: String,
    search_hits: Vec<usize>,
    search_idx: usize,
}

impl StreamingPager {
    /// Spawn the pager.  `lines` and `done` should already be connected to a
    /// background thread that reads from the ExternalStream.
    pub(crate) fn new(lines: Arc<Mutex<Vec<String>>>, done: Arc<AtomicBool>) -> Result<Self> {
        let (cols, rows) = terminal::size().into_diagnostic()?;
        Ok(Self {
            lines,
            done,
            scroll: 0,
            rows,
            cols,
            search_pattern: String::new(),
            search_hits: Vec::new(),
            search_idx: 0,
        })
    }

    fn page_rows(&self) -> usize {
        (self.rows.saturating_sub(1) as usize).max(1)
    }

    /// Current number of buffered lines (may grow over time).
    fn line_count(&self) -> usize {
        self.lines.lock().unwrap().len()
    }

    fn max_scroll(&self) -> usize {
        self.line_count().saturating_sub(self.page_rows())
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    // ── rendering ──
    fn render(&self) -> Result<()> {
        let mut stdout = std::io::stdout();
        let page = self.page_rows();
        let total = self.line_count();
        let end = (self.scroll + page).min(total);

        // Snapshot visible lines under the lock — quick, no I/O while locked.
        let visible: Vec<String> = {
            let guard = self.lines.lock().unwrap();
            guard[self.scroll..end].to_vec()
        };

        execute!(stdout, cursor::MoveTo(0, 0)).into_diagnostic()?;

        for (offset, line) in visible.iter().enumerate() {
            let abs_i = self.scroll + offset;
            let is_search_hit = !self.search_pattern.is_empty()
                && self.search_hits.get(self.search_idx) == Some(&abs_i);

            let display = truncate_to_width(line, self.cols as usize);
            if is_search_hit {
                execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;
                write!(stdout, "{}", display).into_diagnostic()?;
                execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
            } else {
                write!(stdout, "{}", display).into_diagnostic()?;
            }
            execute!(stdout, crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine)).into_diagnostic()?;
            execute!(stdout, crossterm::style::ResetColor).into_diagnostic()?;
            if offset < page - 1 {
                write!(stdout, "\r\n").into_diagnostic()?;
            }
        }

        // Clear remaining rows if content shorter than page.
        for _ in (end - self.scroll)..page {
            write!(stdout, "\r\n").into_diagnostic()?;
            execute!(stdout, crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine)).into_diagnostic()?;
        }

        self.render_status()?;
        stdout.flush().into_diagnostic()?;
        Ok(())
    }

    fn render_status(&self) -> Result<()> {
        let mut stdout = std::io::stdout();
        let bottom = self.rows.saturating_sub(1);
        execute!(stdout, cursor::MoveTo(0, bottom)).into_diagnostic()?;
        execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;

        let total = self.line_count();
        let finished = self.done.load(Ordering::Relaxed);
        let start = if total == 0 { 0 } else { self.scroll + 1 };
        let page = self.page_rows();
        let end = (self.scroll + page).min(total);

        let status = if total == 0 && !finished {
            "(loading...)  q=quit".to_string()
        } else if !finished {
            // Still receiving — show count without percentage.
            format!("lines {}-{}/{}  (loading...)  q=quit", start, end, total)
        } else if !self.search_pattern.is_empty() {
            let cur = if self.search_hits.is_empty() { 0 } else { self.search_idx + 1 };
            format!(
                "lines {}-{}/{}  search: \"{}\"  [{}/{}]  q=quit",
                start, end, total, self.search_pattern, cur, self.search_hits.len(),
            )
        } else if total == 0 {
            "(empty)  q=quit".to_string()
        } else {
            let pct = (end * 100) / total;
            format!("lines {}-{}/{}  ({}%)  q=quit  /=search", start, end, total, pct)
        };

        let w = self.cols as usize;
        if status.len() < w {
            write!(stdout, "{: <width$}", status, width = w).into_diagnostic()?;
        } else {
            write!(stdout, "{}", &status[..w]).into_diagnostic()?;
        }
        execute!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
        Ok(())
    }

    // ── search ──
    fn update_search(&mut self) {
        self.search_hits.clear();
        self.search_idx = 0;
        if self.search_pattern.is_empty() {
            return;
        }
        let pat = self.search_pattern.to_lowercase();
        let guard = self.lines.lock().unwrap();
        for (i, line) in guard.iter().enumerate() {
            if line.to_lowercase().contains(&pat) {
                self.search_hits.push(i);
            }
        }
    }

    fn next_search_hit(&mut self) {
        if self.search_hits.is_empty() {
            return;
        }
        self.search_idx = (self.search_idx + 1) % self.search_hits.len();
        self.scroll_to_hit();
    }

    fn prev_search_hit(&mut self) {
        if self.search_hits.is_empty() {
            return;
        }
        self.search_idx = self
            .search_idx
            .checked_sub(1)
            .unwrap_or(self.search_hits.len() - 1);
        self.scroll_to_hit();
    }

    fn scroll_to_hit(&mut self) {
        if let Some(&line_idx) = self.search_hits.get(self.search_idx) {
            let page = self.page_rows();
            if line_idx < self.scroll || line_idx >= self.scroll + page {
                self.scroll = line_idx.saturating_sub(page / 2);
                self.clamp_scroll();
            }
        }
    }

    // ── event loop ──
    pub(crate) fn run(&mut self) -> Result<()> {
        execute!(std::io::stdout(), cursor::Hide).into_diagnostic()?;
        self.render()?;

        let mut search_buf = String::new();
        let mut searching = false;
        let mut prev_count = self.line_count();

        loop {
            match event::read().into_diagnostic()? {
                Event::Key(KeyEvent { code, modifiers, .. }) => {
                    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                        break;
                    }
                    if searching {
                        match code {
                            KeyCode::Esc => {
                                searching = false;
                                search_buf.clear();
                            }
                            KeyCode::Enter => {
                                searching = false;
                                self.search_pattern = search_buf.clone();
                                search_buf.clear();
                                self.update_search();
                                if !self.search_hits.is_empty() {
                                    self.search_idx = 0;
                                    self.scroll_to_hit();
                                }
                            }
                            KeyCode::Backspace => {
                                search_buf.pop();
                            }
                            KeyCode::Char(c) => {
                                search_buf.push(c);
                            }
                            _ => {}
                        }
                        if searching {
                            let mut s = std::io::stdout();
                            let bottom = self.rows.saturating_sub(1);
                            execute!(s, cursor::MoveTo(0, bottom)).into_diagnostic()?;
                            execute!(s, crossterm::style::SetAttribute(crossterm::style::Attribute::Reverse)).into_diagnostic()?;
                            write!(s, "/{: <width$}", search_buf, width = self.cols as usize).into_diagnostic()?;
                            execute!(s, crossterm::style::SetAttribute(crossterm::style::Attribute::NoReverse)).into_diagnostic()?;
                            s.flush().into_diagnostic()?;
                        } else {
                            self.render()?;
                        }
                        continue;
                    }

                    match code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('j') | KeyCode::Down => {
                            self.scroll += 1;
                            self.clamp_scroll();
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            self.scroll = self.scroll.saturating_sub(1);
                        }
                        KeyCode::Char(' ') | KeyCode::Char('f') | KeyCode::PageDown => {
                            self.scroll += self.page_rows();
                            self.clamp_scroll();
                        }
                        KeyCode::Char('b') | KeyCode::PageUp => {
                            self.scroll = self.scroll.saturating_sub(self.page_rows());
                        }
                        KeyCode::Char('g') | KeyCode::Home => self.scroll = 0,
                        KeyCode::Char('G') | KeyCode::End => {
                            // If still loading, jump to current bottom.
                            self.scroll = self.max_scroll();
                        }
                        KeyCode::Char('/') => {
                            searching = true;
                            search_buf.clear();
                        }
                        KeyCode::Char('n') => self.next_search_hit(),
                        KeyCode::Char('N') => self.prev_search_hit(),
                        _ => {}
                    }
                    self.render()?;
                }
                Event::Resize(cols, rows) => {
                    self.cols = cols;
                    self.rows = rows;
                    self.clamp_scroll();
                    self.render()?;
                }
                _ => {}
            }

            // Re-render if new lines arrived while we were waiting for input.
            let cur_count = self.line_count();
            if cur_count != prev_count {
                prev_count = cur_count;
                self.clamp_scroll();
                self.render()?;
            }
        }
        Ok(())
    }
}

// ── helpers ─────────────────────────────────────────────────────────

/// Truncate a string to fit within `max_width` terminal columns.
/// ANSI escape sequences are preserved but counted as zero-width.
fn truncate_to_width(s: &str, max_width: usize) -> &str {
    let mut visible = 0usize;
    let mut in_escape = false;
    for (i, c) in s.char_indices() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        visible += 1;
        if visible > max_width {
            return &s[..i];
        }
    }
    s
}

/// Read text content: file path takes precedence, then pipeline input.
fn read_input(
    args: &ParsedArgs,
    input: PipelineData,
    shell: &mut Shell,
) -> Result<Vec<String>> {
    let text = if let Some(path) = args.positionals.first() {
        let resolved = shell.resolve_path(path, false)?;
        if !resolved.exists() {
            miette::bail!("less: {}: No such file or directory", path);
        }
        std::fs::read_to_string(&resolved)
            .into_diagnostic()
            .map_err(|e| miette::miette!("less: {}: {}", path, e))?
    } else {
        match input {
            PipelineData::Text(s) => s,
            PipelineData::Value(auto_val::Value::Str(s)) => s.to_string(),
            _ => miette::bail!("less: no input (provide a file or pipe text)"),
        }
    };

    Ok(text.lines().map(|l| l.to_string()).collect())
}

// ── Command impl ────────────────────────────────────────────────────

pub struct LessCommand;

impl Command for LessCommand {
    fn name(&self) -> &str {
        "less"
    }

    fn signature(&self) -> Signature {
        Signature::new(
            "less",
            "Interactive file pager — view content one screen at a time",
        )
        .optional("file", "Path to the file to view (default: read from pipeline)")
        .extra_help(
            "Navigation:\n  \
             j / Down       scroll down 1 line\n  \
             k / Up         scroll up 1 line\n  \
             Space / f      page down\n  \
             b              page up\n  \
             g / Home       go to top\n  \
             G / End        go to bottom\n  \
             /              search forward\n  \
             n / N          next / previous search match\n  \
             q / Esc        quit",
        )
    }

    fn run(
        &self,
        args: &ParsedArgs,
        input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        run_less(args, input, shell)
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        input: ash_core::pipeline::AtomPipeline,
        shell: &mut Shell,
    ) -> Result<ash_core::pipeline::AtomPipeline> {
        run_less_atom(args, input, shell)
    }
}

/// `more` is the POSIX-standard pager; on this system it behaves identically
/// to `less` (which is a superset of POSIX `more`).
pub struct MoreCommand;

impl Command for MoreCommand {
    fn name(&self) -> &str {
        "more"
    }

    fn signature(&self) -> Signature {
        Signature::new(
            "more",
            "Interactive file pager — POSIX more command (alias for less)",
        )
        .optional("file", "Path to the file to view (default: read from pipeline)")
        .extra_help(
            "Navigation:\n  \
             j / Down       scroll down 1 line\n  \
             k / Up         scroll up 1 line\n  \
             Space / f      page down\n  \
             b              page up\n  \
             g / Home       go to top\n  \
             G / End        go to bottom\n  \
             /              search forward\n  \
             n / N          next / previous search match\n  \
             q / Esc        quit",
        )
    }

    fn run(
        &self,
        args: &ParsedArgs,
        input: PipelineData,
        shell: &mut Shell,
    ) -> Result<PipelineData> {
        run_less(args, input, shell)
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        input: ash_core::pipeline::AtomPipeline,
        shell: &mut Shell,
    ) -> Result<ash_core::pipeline::AtomPipeline> {
        run_less_atom(args, input, shell)
    }
}

/// Shared `run_atom` for both `less` and `more`.
///
/// When the input is an `ExternalStream` (e.g. from `show file.rs | less`
/// where `show` was spawned as a subprocess), we stream lines incrementally:
/// a background thread reads the pipe while the pager displays what's arrived.
fn run_less_atom(
    args: &ParsedArgs,
    input: ash_core::pipeline::AtomPipeline,
    shell: &mut Shell,
) -> Result<ash_core::pipeline::AtomPipeline> {
    // File argument → read file directly (no streaming needed).
    let has_file = args.positionals.first().is_some();

    // ExternalStream input without a file arg → streaming pager.
    if !has_file {
        if let ash_core::pipeline::AtomPipeline::ExternalStream(es) = input {
            // Only pager when stdout is a terminal and we're pipeline-final.
            if std::io::stdout().is_terminal() && shell.is_pipeline_last() {
                let lines = Arc::new(Mutex::new(Vec::new()));
                let done = Arc::new(AtomicBool::new(false));

                // Background reader thread: drains the OS pipe into `lines`.
                {
                    let lines = lines.clone();
                    let done = done.clone();
                    std::thread::Builder::new()
                        .name("less-pipe-reader".into())
                        .spawn(move || {
                            for line in es.lines() {
                                match line {
                                    Ok(l) => lines.lock().unwrap().push(l),
                                    Err(_) => break,
                                }
                            }
                            done.store(true, Ordering::Relaxed);
                        })
                        .ok();
                }

                let _raw = RawModeGuard::enter()?;
                let _alt = AltScreenGuard::enter()?;
                let mut pager = StreamingPager::new(lines, done)?;
                pager.run()?;
                return Ok(ash_core::pipeline::AtomPipeline::Empty);
            }

            // Non-TTY: materialize the stream to text and dump (like cat).
            let text = es.read_all().unwrap_or_default();
            return Ok(ash_core::pipeline::AtomPipeline::Text(text));
        }
    }

    // Non-streaming path: bridge to legacy run().
    let legacy_in = crate::cmd::pipeline_convert::atom_to_pipeline_data(input);
    let legacy_out = run_less(args, legacy_in, shell)?;
    Ok(crate::cmd::pipeline_convert::pipeline_data_to_atom(legacy_out))
}

/// Shared implementation for both `less` and `more`.
fn run_less(
    args: &ParsedArgs,
    input: PipelineData,
    shell: &mut Shell,
) -> Result<PipelineData> {
    if args.positionals.len() > 1 {
        miette::bail!("less: only one file argument is supported");
    }

    let lines = read_input(args, input, shell)?;

    // Only enter pager mode when stdout is a terminal AND this command is
    // the final (or only) one in its pipeline.  In-memory registry pipelines
    // don't redirect stdout, so is_terminal() alone isn't sufficient.
    if !std::io::stdout().is_terminal() || !shell.is_pipeline_last() {
        // Non-TTY: just dump the content (like cat).
        let text = lines.join("\n");
        return Ok(PipelineData::from_text(text));
    }

    // Take over the terminal.
    let _raw = RawModeGuard::enter()?;
    let _alt = AltScreenGuard::enter()?;

    let mut pager = Pager::new(lines)?;
    pager.run()?;

    // Guards drop here, restoring terminal.
    Ok(PipelineData::empty())
}
