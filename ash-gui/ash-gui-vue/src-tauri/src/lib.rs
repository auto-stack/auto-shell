//! ash-gui (Vue/Tauri) — the web-frontend version of ash.
//!
//! Plan 042 M3: the backend logic lives in `ash-server`. This crate is now a
//! thin Tauri shell: it spawns the `ash_server` worker, registers Tauri
//! commands that forward to `ShellHandle`, and bridges the worker's broadcast
//! events (`ShellEvent`) to the Tauri event bus (`command-result` /
//! `command-output`) so the frontend's `listen()` calls work unchanged.

mod commands;

use ash_server::ShellEvent;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Spawn the unified Shell worker (owns the !Send Shell on a thread).
            let handle = ash_server::spawn();

            // Bridge: subscribe to the worker's broadcast channel and re-emit
            // each ShellEvent as a Tauri event. The frontend listens on
            // `command-result` / `command-output` (unchanged from Plan 040/041).
            let app_handle = app.handle().clone();
            let rx = handle.subscribe();
            tauri::async_runtime::spawn(async move {
                use tokio_stream::wrappers::BroadcastStream;
                use tokio_stream::StreamExt;
                let mut stream = BroadcastStream::new(rx);
                while let Some(result) = stream.next().await {
                    if let Ok(event) = result {
                        bridge_event(&app_handle, event);
                    }
                }
            });

            app.manage(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::run_command,
            commands::run_smart_command,
            commands::cancel_command,
            commands::complete,
            commands::prompt_context,
            commands::history,
            commands::command_list,
            commands::open_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ash-gui");
}

/// Translate a `ShellEvent` into the Tauri events the frontend expects.
///
/// `ShellEvent::CommandOutput` → `command-output` event (streaming chunks)
/// `ShellEvent::CommandResult` → `command-result` event (final result)
fn bridge_event(app: &tauri::AppHandle, event: ShellEvent) {
    use ash_server::CommandOutput;
    match event {
        ShellEvent::CommandOutput { block_id, chunk } => {
            let _ = app.emit("command-output", CommandOutput { block_id, chunk });
        }
        ShellEvent::CommandResult(result) => {
            let _ = app.emit("command-result", result);
        }
    }
}
