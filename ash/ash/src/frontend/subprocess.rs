//! Subprocess handoff for the block TUI (Plan 038 M3).
//!
//! The block TUI owns the terminal (raw mode + inline viewport) for its entire
//! run, unlike the reedline REPL which drops raw mode between `read_line`
//! calls. So when the user runs a fullscreen program (`vim`/`less`/`top`/...),
//! we must **tear down** ratatui's hold, let the subprocess inherit stdio,
//! then **rebuild** on return.
//!
//! ## The two non-obvious ordering constraints (from M3 research)
//! 1. Rebuild must `enable_raw_mode()` BEFORE `terminal.resize()` — the inline
//!    viewport re-anchor calls `crossterm::cursor::position()` (a DSR query)
//!    which is only reliable in raw mode.
//! 2. We do NOT use the alt screen (M3 dropped it), so there's no
//!    Leave/EnterAlternateScreen dance — simpler and the inline viewport can
//!    push blocks into the host scrollback (its whole purpose).
//!
//! ## Why not reuse the `less` pager's RawModeGuard?
//! crossterm's raw mode on Unix is guarded by a process-wide single slot
//! (`TERMINAL_MODE_PRIOR_RAW_MODE`). If the pager's guard double-acquires and
//! drops inside the block TUI, it clears that slot, leaving the block TUI
//! unable to correctly disable raw mode later. So this module does its own
//! symmetric disable/enable instead of nesting guards.

use std::io::{self, stdout};

use ratatui_core::terminal::Terminal;
use ratatui_crossterm::crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui_crossterm::CrosstermBackend;

/// Tear down ratatui, run a fullscreen interactive command with inherited
/// stdio, then rebuild. Returns when the subprocess exits.
///
/// `line` is the full command string (e.g. `vim file.txt`). `cwd` is the
/// working directory to run it in.
pub fn hand_off_to_interactive(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    line: &str,
    cwd: &std::path::Path,
) -> io::Result<()> {
    // ── Tear down ──
    // No alt screen to leave (M3 dropped it). Just disable raw mode so the
    // subprocess and the terminal driver operate in cooked mode.
    disable_raw_mode()?;
    // Move the cursor below the viewport so the subprocess's output doesn't
    // overwrite the ratatui frame. Print a newline to roll the viewport up.
    {
        use std::io::Write;
        let mut out = stdout();
        let _ = writeln!(out);
        let _ = out.flush();
    }

    // ── Run the subprocess (inherit stdio) ──
    if let Err(e) = ash_core::cmd::external::execute_external(line, cwd, false) {
        // execute_external returns miette::Report; print to stderr.
        eprintln!("Error: {e}");
    }

    // ── Rebuild ──
    // 1. Re-enable raw mode FIRST (DSR cursor query in resize needs it).
    enable_raw_mode()?;
    // 2. Re-anchor the inline viewport: resize() reads the current cursor
    //    position via crossterm::cursor::position() and recomputes the
    //    viewport origin, then clears + resets the back buffer so the next
    //    draw is a full repaint. (resize takes a Rect but for Viewport::Inline
    //    it recomputes the origin from the live cursor — only the size matters.)
    let size = terminal.size()?;
    terminal.resize(ratatui_core::layout::Rect::new(0, 0, size.width, size.height))?;
    Ok(())
}

/// Tear down ratatui (disable raw mode), run `f` in cooked mode, then rebuild.
///
/// Unlike `hand_off_to_interactive` (which spawns an external process with
/// inherited stdio), this takes a closure — for the built-in `less`/`more`
/// pager commands which are Shell commands that manage their own terminal
/// (alt-screen + raw mode) internally. Running them from a clean cooked state
/// avoids the crossterm raw-mode double-acquire bug (Unix single-slot guard).
pub fn run_with_handoff<F>(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, f: F) -> io::Result<()>
where
    F: FnOnce(),
{
    // ── Tear down ──
    disable_raw_mode()?;
    {
        use std::io::Write;
        let mut out = stdout();
        let _ = writeln!(out);
        let _ = out.flush();
    }

    // ── Run the closure (e.g. shell.execute("less file")) ──
    f();

    // ── Rebuild ──
    enable_raw_mode()?;
    let size = terminal.size()?;
    terminal.resize(ratatui_core::layout::Rect::new(0, 0, size.width, size.height))?;
    Ok(())
}

/// Run an external `$EDITOR` on a temp file with the same teardown/rebuild.
/// Returns the edited content (or the original on editor failure).
///
/// (Stub for Ctrl+E — M3 wires the teardown; the editor invocation itself is
/// M4 orchestration, since the reedline REPL's edit_in_editor is bound to a
/// reedline keybinding that the block TUI doesn't have yet.)
pub fn run_external_editor(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    initial_content: &str,
) -> io::Result<String> {
    use std::io::Write;
    let tmp_dir = std::env::temp_dir();
    let tmp_file = tmp_dir.join("ash_block_tui_edit.txt");
    std::fs::write(&tmp_file, initial_content)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(windows) { "notepad".to_string() } else { "vim".to_string() }
        });
    let parts: Vec<&str> = editor.split_whitespace().collect();
    let (cmd, extra_args) = match parts.split_first() {
        Some((c, args)) => (*c, args.to_vec()),
        None => ("vim", vec![]),
    };
    let mut command = std::process::Command::new(cmd);
    command.args(&extra_args).arg(&tmp_file);

    // Teardown.
    disable_raw_mode()?;
    {
        let mut out = stdout();
        let _ = writeln!(out);
        let _ = out.flush();
    }

    let status = command
        .status()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("editor: {e}")))?;

    // Rebuild.
    enable_raw_mode()?;
    let size = terminal.size()?;
    terminal.resize(ratatui_core::layout::Rect::new(0, 0, size.width, size.height))?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("editor: exited with status {}", status),
        ));
    }

    let content = std::fs::read_to_string(&tmp_file)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let _ = std::fs::remove_file(&tmp_file);
    Ok(content.trim().to_string())
}
