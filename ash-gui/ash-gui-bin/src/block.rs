//! Block — one command execution's complete record (Plan 030 M3 / design §2.2).
//!
//! The GUI main view is a scrolling list of Blocks. Each Block is an addressable
//! object (searchable, referenceable, status-colored) rather than a row in a
//! character grid — the core advantage a GUI has over a TUI.
//!
//! M3 scope: the core fields (command / cwd / status / output). The design's
//! envelope / timing / sub_blocks / ai_context fields are deferred to M5.

use std::path::PathBuf;

use ash_core::renderer::RenderedOutput;

/// One command execution. The GUI main view is a `Vec<Block>`.
#[derive(Debug, Clone)]
pub struct Block {
    /// Monotonic id (also the list index surrogate / future `@{block:id}` ref).
    pub id: usize,
    /// The command line the user entered.
    pub command: String,
    /// The working directory the command ran in.
    pub cwd: PathBuf,
    /// Outcome / current state of the execution.
    pub status: BlockStatus,
    /// The structured result, rendered to widgets by `rendered_to_iced`.
    pub output: RenderedOutput,
}

/// A Block's lifecycle state. Drives the header coloring + status icon.
#[derive(Debug, Clone)]
pub enum BlockStatus {
    /// Ran successfully (exit 0).
    Success,
    /// Ran but failed — carries the error message.
    Failed(String),
    /// Currently executing on the Shell worker thread.
    Running,
}

impl Block {
    /// Create a new Block in the `Running` state (the caller updates it to
    /// Success/Failed when the command finishes).
    pub fn running(id: usize, command: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            id,
            command: command.into(),
            cwd,
            status: BlockStatus::Running,
            output: RenderedOutput::Empty,
        }
    }

    /// One-line status label for the block header (e.g. "✓", "✗ error", "…").
    pub fn status_label(&self) -> String {
        match &self.status {
            BlockStatus::Success => "✓".to_string(),
            BlockStatus::Failed(msg) => format!("✗ {}", short_error(msg)),
            BlockStatus::Running => "…".to_string(),
        }
    }
}

/// Trim an error message to a brief header suffix.
fn short_error(msg: &str) -> String {
    let first_line = msg.lines().next().unwrap_or(msg);
    if first_line.chars().count() <= 60 {
        first_line.to_string()
    } else {
        let cut: String = first_line.chars().take(57).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_block_starts_empty() {
        let b = Block::running(0, "ls", PathBuf::from("/tmp"));
        assert_eq!(b.id, 0);
        assert_eq!(b.command, "ls");
        assert!(matches!(b.status, BlockStatus::Running));
        assert!(matches!(b.output, RenderedOutput::Empty));
        assert_eq!(b.status_label(), "…");
    }

    #[test]
    fn status_labels() {
        let mut b = Block::running(0, "x", PathBuf::from("/"));
        b.status = BlockStatus::Success;
        assert_eq!(b.status_label(), "✓");
        b.status = BlockStatus::Failed("oops".into());
        assert_eq!(b.status_label(), "✗ oops");
    }

    #[test]
    fn short_error_truncates_long_messages() {
        let long = "x".repeat(100);
        let s = short_error(&long);
        assert!(s.chars().count() <= 60);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn short_error_keeps_short_messages() {
        assert_eq!(short_error("oops"), "oops");
    }
}
