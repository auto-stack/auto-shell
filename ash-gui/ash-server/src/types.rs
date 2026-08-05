//! Serializable types shared between the Shell backend and all frontends.
//!
//! These are frontend-agnostic (no `tauri::` types) — axum serializes them to
//! JSON for the browser, Tauri serializes them via `invoke`/`emit` for the
//! desktop app. The TypeScript mirrors live in `ash-gui-vue/src/types/shell.ts`.
//!
//! Plan 042 M1: extracted from `ash-gui-vue/src-tauri/src/shell_worker.rs`.

use serde::Serialize;

// ── Boot snapshot (returned by `command_list`) ──────────────────────────────

/// Boot-time snapshot: cwd, home, the command registry, and SmartCommands.
/// Mirrors the TS `CommandListResult` type.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct BootSnapshot {
    pub cwd: String,
    /// Home directory, for the frontend to abbreviate paths with `~`.
    pub home: String,
    pub commands: Vec<ToolEntry>,
    pub smart_commands: Vec<SmartCommandEntry>,
}

#[derive(Serialize, Clone)]
pub struct ToolEntry {
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Clone)]
pub struct SmartCommandEntry {
    pub name: String,
    pub description: String,
}

// ── Completion (Plan 041 M7) ─────────────────────────────────────────────────

/// One completion candidate, serialized for the frontend. Mirrors the core
/// `auto_shell::completions::Completion` type.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CompletionItem {
    pub replacement: String,
    pub display: String,
    pub description: Option<String>,
    pub kind: String,
}

// ── Prompt context (Plan 041 M5: git branch/status) ─────────────────────────

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct PromptContext {
    pub git_branch: Option<String>,
    pub git_status: Option<GitStatusInfo>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub struct GitStatusInfo {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub ahead: usize,
    pub behind: usize,
}

// ── SmartCommand result ─────────────────────────────────────────────────────

/// Reply to a SmartCommand execution request.
pub struct SmartResult {
    pub output: String,
    pub error: Option<String>,
}

// ── Command result + streaming events ───────────────────────────────────────

/// The final result of a command, emitted as a `ShellEvent::CommandResult`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CommandResult {
    pub block_id: usize,
    pub cwd: String,
    pub status: CommandStatus,
    pub output: CommandOutputPayload,
    pub duration_ms: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub enum CommandStatus {
    Success,
    Failed(String),
}

/// The output payload — mirrors `ash_core::renderer::RenderedOutput` serialized
/// via the existing serde derives on that type.
pub type CommandOutputPayload = ash_core::renderer::RenderedOutput;

/// A streaming output chunk, emitted as a `ShellEvent::CommandOutput`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CommandOutput {
    pub block_id: usize,
    pub chunk: String,
}

/// Events the Shell worker pushes to subscribers (frontends). The HTTP transport
/// serializes these as SSE frames; the Tauri transport emits them as Tauri events.
///
/// Plan 042 M1: unifies the two emit sites in the original `shell_worker.rs`
/// (`command-result` and `command-output`) into one enum.
#[derive(Serialize, Clone)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ShellEvent {
    /// A chunk of streamed output from a long external command (Plan 040 M4).
    CommandOutput { block_id: usize, chunk: String },
    /// The final result of a command (success or failure).
    CommandResult(CommandResult),
}
