//! Terminal-dependent commands that moved out of `auto-shell` in Plan 037 M2.2.
//!
//! - `less` / `more` — interactive file pager (crossterm). Re-exported from
//!   `commands_less.rs` (the original `auto-shell` implementation, moved here).
//! - `color` — 24-bit rainbow / color-depth report (nu-ansi-term). Reimplemented
//!   here as a `Command` (it was previously a `Shell` dispatch arm).
//!
//! These register into a `Shell` via `Shell::register_commands` at the
//! composition root (this crate's `main.rs`).

mod less {
    pub use crate::frontend::commands_less::*;
}

pub use less::{LessCommand, MoreCommand};

use crate::term::color::{detect_color_depth, resolve_fg, ColorDepth};
use auto_shell::cmd::parser::ParsedArgs;
use auto_shell::cmd::{Command, PipelineData, ShellContext, Signature};
use auto_shell::shell::PagerHook;
use ash_core::pipeline::AtomPipeline;
use miette::Result;

/// Plan 037 M2.2: the interactive `show --pager` backend, injected into
/// `Shell` via `set_pager_hook`. Takes over the terminal (raw mode + alternate
/// screen via the crossterm guards from `commands_less`) and runs the lazy-
/// highlighting `CodePager`.
pub struct TuiPagerHook;

impl PagerHook for TuiPagerHook {
    fn run_code_pager(&self, lines: Vec<String>, ext: String) -> miette::Result<()> {
        let _raw = less::RawModeGuard::enter()?;
        let _alt = less::AltScreenGuard::enter()?;
        let mut pager = less::CodePager::new(lines, ext)?;
        pager.run()?;
        Ok(())
    }
}

/// `color` — print a 24-bit rainbow gradient / report terminal color depth.
///
///   color rainbow   → monospaced rainbow (each char a different hue)
///   color depth     → report the detected color depth + relevant env vars
///
/// Plan 037 M2.2: this was the `Shell::cmd_color` dispatch arm; it moved to
/// ash-tui because it depends on `term::color` (nu-ansi-term).
pub struct ColorCommand;

impl Command for ColorCommand {
    fn name(&self) -> &str {
        "color"
    }

    fn signature(&self) -> Signature {
        Signature::new("color", "24-bit color demo (rainbow) or color-depth report")
    }

    fn run(
        &self,
        args: &ParsedArgs,
        _input: PipelineData,
        _shell: &mut dyn ShellContext,
    ) -> Result<PipelineData> {
        let sub = args.get_positional(0).map(|s| s.as_str()).unwrap_or("depth");
        let out = color_dispatch(sub)?;
        Ok(PipelineData::from_text(out))
    }

    fn run_atom(
        &self,
        args: &ParsedArgs,
        _input: AtomPipeline,
        _shell: &mut dyn ShellContext,
    ) -> Result<AtomPipeline> {
        let sub = args.get_positional(0).map(|s| s.as_str()).unwrap_or("depth");
        let out = color_dispatch(sub)?;
        Ok(AtomPipeline::text(out))
    }
}

/// Shared dispatch for both `run` and `run_atom` (mirrors the original
/// `cmd_color` rainbow/depth branches).
fn color_dispatch(sub: &str) -> Result<String> {
    match sub {
        "rainbow" | "rb" => {
            let text = "Ash 24-bit Truecolor Rainbow!";
            let chars: Vec<char> = text.chars().collect();
            let n = chars.len();
            let mut out = String::new();
            for (i, ch) in chars.iter().enumerate() {
                let hue = 360.0 * (i as f64) / (n as f64);
                let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
                let color = resolve_fg(r, g, b);
                out.push_str(&color.paint(&ch.to_string()).to_string());
            }
            Ok(out)
        }
        "depth" | "info" => {
            let depth = detect_color_depth();
            let label = match depth {
                ColorDepth::True24 => "24-bit truecolor",
                ColorDepth::Index256 => "256-color",
                ColorDepth::Index16 => "16-color",
            };
            let ct = std::env::var("COLORTERM").unwrap_or_else(|_| "(unset)".into());
            let term = std::env::var("TERM").unwrap_or_else(|_| "(unset)".into());
            Ok(format!(
                "Color depth: {} (COLORTERM={}, TERM={})",
                label, ct, term
            ))
        }
        other => miette::bail!(
            "color: unknown subcommand '{}'. Use: rainbow, depth",
            other
        ),
    }
}

/// HSV to RGB conversion (h: 0-360°, s/v: 0.0-1.0) for the rainbow demo.
/// Moved from `auto-shell/src/shell.rs` in Plan 037 M2.2 (its only caller,
/// `cmd_color`, moved here too).
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let c = v * s;
    let h6 = (h / 60.0) % 6.0;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = match h6 as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// All terminal-dependent commands, ready to register via
/// `Shell::register_commands`. Called by the `ash` binary at startup.
pub fn terminal_commands() -> Vec<Box<dyn Command>> {
    vec![
        Box::new(LessCommand),
        Box::new(MoreCommand),
        Box::new(ColorCommand),
    ]
}
