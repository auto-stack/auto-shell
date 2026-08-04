//! ash-gui (Vue/Tauri) — the web-frontend version of ash.
//!
//! The Shell is `!Send` so it lives on a dedicated worker thread
//! (`shell_worker`); Tauri commands hand requests to it over a channel and
//! results come back as `command-result` events. See `designs/030-ash-gui.md`.

mod commands;
mod shell_worker;

use shell_worker::spawn;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Spawn the Shell worker; it stashes its boot snapshot + handle
            // into managed state that the commands read.
            let handle = spawn(app.handle().clone());
            app.manage(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::run_command,
            commands::run_smart_command,
            commands::cancel_command,
            commands::history,
            commands::command_list,
            commands::open_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ash-gui");
}
