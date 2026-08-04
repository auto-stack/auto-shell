//! Tauri commands — the frontend calls these via `invoke`.
//!
//! - `run_command`: submit a command to the Shell worker (result comes via event).
//! - `command_list`: boot data — cwd, command registry, SmartCommands.

use tauri::State;

use crate::shell_worker::{BootSnapshot, BootState, ShellHandle};

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
