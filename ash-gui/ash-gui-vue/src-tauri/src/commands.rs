//! Tauri commands — thin wrappers over `ash_server::ShellHandle`.
//!
//! Plan 042 M3: all backend logic now lives in the `ash-server` crate. These
//! commands just forward to the shared `ShellHandle` and relay events from the
//! broadcast channel to the Tauri event bus.

use tauri::State;

use ash_server::ShellHandle;

/// Submit a command. Non-blocking: the result arrives as a `command-result`
/// Tauri event (bridged from the worker's broadcast channel in `lib.rs`).
#[tauri::command]
pub fn run_command(block_id: usize, cmd: String, shell: State<'_, ShellHandle>) {
    shell.run_command(block_id, cmd);
}

/// Plan 041 M5: produce completions via the shared backend engine.
#[tauri::command]
pub async fn complete(
    line: String,
    cursor: usize,
    shell: State<'_, ShellHandle>,
) -> Result<Vec<ash_server::CompletionItem>, String> {
    shell.complete(line, cursor).await
}

/// Plan 041 M5: get the prompt context (git branch/status).
#[tauri::command]
pub async fn prompt_context(shell: State<'_, ShellHandle>) -> Result<ash_server::PromptContext, String> {
    shell.prompt_context().await
}

/// Plan 040 M3: run a SmartCommand by name.
#[tauri::command]
pub async fn run_smart_command(
    block_id: usize,
    name: String,
    args: Vec<String>,
    shell: State<'_, ShellHandle>,
) -> Result<String, String> {
    let result = shell.run_smart(block_id, name, args).await?;
    match result.error {
        Some(e) => Err(e),
        None => Ok(result.output),
    }
}

/// Plan 040 M5: cancel the running command.
#[tauri::command]
pub fn cancel_command(shell: State<'_, ShellHandle>) {
    shell.cancel();
}

/// Plan 040 M6: read the shared CLI history file.
#[tauri::command]
pub fn history() -> Vec<String> {
    ash_server::worker::read_history()
}

/// Boot data: cwd + command list + SmartCommands.
#[tauri::command]
pub async fn command_list(shell: State<'_, ShellHandle>) -> Result<ash_server::BootSnapshot, String> {
    shell.command_list().await
}

/// Open `path` with the OS default application (best-effort, detached).
#[tauri::command]
pub fn open_path(path: String) {
    let _ = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&path).spawn()
    };
}
