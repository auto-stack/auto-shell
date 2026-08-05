//! Tauri commands — the frontend calls these via `invoke`.
//!
//! - `run_command`: submit a command to the Shell worker (result comes via event).
//! - `run_smart_command`: Plan 040 M3 — execute a SmartCommand by name+args.
//! - `cancel_command`: Plan 040 M5 — cancel the running command.
//! - `history`: Plan 040 M6 — read the shared CLI history file.
//! - `command_list`: boot data — cwd, command registry, SmartCommands.

use tauri::State;

use crate::shell_worker::{read_history, BootSnapshot, BootState, CompletionItem, ShellHandle};

/// Submit a command for the given block id. Non-blocking: the result arrives as
/// a `command-result` Tauri event when the Shell worker finishes.
#[tauri::command]
pub fn run_command(
    block_id: usize,
    cmd: String,
    shell: State<'_, ShellHandle>,
) {
    shell.submit(block_id, cmd);
}

/// Plan 041 M7: produce completions for `line` at `cursor`. Routes to the
/// worker thread, which runs the shared completion engine
/// (`auto_shell::completions::engine::complete`) with the live Shell state
/// (cwd/history/aliases) — the same engine CLI/TUI use. Returns serialized
/// candidates for the frontend to render.
#[tauri::command]
pub async fn complete(
    line: String,
    cursor: usize,
    shell: State<'_, ShellHandle>,
) -> Result<Vec<CompletionItem>, String> {
    shell.complete(line, cursor).await
}

/// Plan 040 M5: cancel the currently running command. The worker checks its
/// cancel flag in the streaming-drain loop; a command blocked in
/// `shell.execute()` finishes on its own. Best-effort.
#[tauri::command]
pub fn cancel_command(shell: State<'_, ShellHandle>) {
    shell.cancel();
}

/// Plan 040 M3: run a SmartCommand by name with positional args.
///
/// SmartCommands were broken before: the sidebar injected `smart run X` into
/// the prompt, but `smart` is a CLI subcommand (`main.rs`), not a Shell command
/// — `shell.execute("smart run X")` couldn't parse it. This routes the spec
/// body through the Shell worker so it runs on the worker's *live* Shell
/// (preserving session cwd/env/functions), with `$1`/`$2`/… injected.
///
/// `block_id` attributes the body's streamed output (via the worker's
/// OutputHook) to the frontend's Running block.
#[tauri::command]
pub async fn run_smart_command(
    block_id: usize,
    name: String,
    args: Vec<String>,
    shell: State<'_, ShellHandle>,
) -> Result<String, String> {
    // The reply comes via the oneshot channel (not a command-result event).
    let result = shell.run_smart(block_id, name, args).await?;
    match result.error {
        Some(e) => Err(e),
        None => Ok(result.output),
    }
}

/// Plan 040 M6: read the shared CLI history file (`~/.auto-shell-history`),
/// oldest first. Same file the TUI/CLI REPL writes, so GUI and CLI stay in sync.
#[tauri::command]
pub fn history() -> Vec<String> {
    read_history()
}

/// Boot data: current cwd + the command list (for completion / sidebar) +
/// SmartCommands. Reads the snapshot the worker thread produced at startup,
/// waiting briefly if it isn't ready yet.
#[tauri::command]
pub async fn command_list(boot: State<'_, BootState>) -> Result<BootSnapshot, String> {
    // The worker fills this almost immediately; poll until ready (bounded).
    for _ in 0..200 {
        if let Some(snap) = boot.0.lock().await.clone() {
            return Ok(snap);
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    Err("Shell worker failed to initialize in time".into())
}

/// Open `path` with the OS default application (best-effort, detached).
/// Mirrors the iced frontend's `open_with_default` (ash-gui-bin/src/main.rs:375).
#[tauri::command]
pub fn open_path(path: String) {
    use std::process::Command;
    let _ = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "start", "", &path])
            .spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(&path).spawn()
    } else {
        Command::new("xdg-open").arg(&path).spawn()
    };
}
