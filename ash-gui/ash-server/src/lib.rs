//! ash-server — the unified Shell backend for ash-gui.
//!
//! Serves both the **browser** (HTTP via axum, M2) and the **Tauri desktop app**
//! (IPC via `#[tauri::command]`, M3) from a single [`ShellApi`] implementation.
//! The Shell engine itself is `auto_shell::Shell` (same as CLI/TUI); this crate
//! wraps it in a frontend-agnostic API so both transports share one backend.
//!
//! ## Architecture (Plan 042)
//!
//! ```text
//!                         ┌──────────────────────────┐
//!                         │     ash-server (这里)      │
//!                         │  ┌──────────────────────┐ │
//!                         │  │ ShellApi (统一接口)   │ │
//!                         │  │  run / complete /    │ │
//!                         │  │  cancel / history    │ │
//!                         │  └────┬──────────┬──────┘ │
//!                         │  axum routes  tauri cmds  │
//!                         │  (HTTP+SSE)   (IPC+event) │
//!                         └───┬──────────────┬───────┘
//!                    HTTP/SSE │              │ Tauri IPC
//!                  ┌──────────┘              └──────────┐
//!                  ▼                                    ▼
//!          浏览器版(npm run dev)               Tauri 版(tauri dev)
//!          useShellHttp()                     useShellTauri()
//!          → fetch + EventSource              → invoke + listen
//! ```
//!
//! The Shell is `!Send` (auto-lang VM uses `Rc`), so it lives on a dedicated
//! worker thread (`ShellWorker`) — same architecture as the original
//! `shell_worker.rs`, now frontend-independent.

pub mod backend;
pub mod http;
pub mod types;
pub mod worker;

pub use types::*;
pub use worker::{spawn, ShellHandle};
