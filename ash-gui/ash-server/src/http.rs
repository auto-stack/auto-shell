//! HTTP transport — axum routes + SSE streaming (Plan 042 M2).
//!
//! Exposes the Shell backend as a REST API + Server-Sent Events stream:
//!
//! | Method | Path | Body / Query | Returns |
//! |--------|------|-------------|---------|
//! | GET | `/api/command_list` | — | `BootSnapshot` JSON |
//! | GET | `/api/history` | — | `Vec<String>` JSON |
//! | POST | `/api/complete` | `{line, cursor}` | `Vec<CompletionItem>` |
//! | GET | `/api/prompt_context` | — | `PromptContext` |
//! | POST | `/api/run_command` | `{block_id, cmd}` | `{}` (result via SSE) |
//! | POST | `/api/cancel` | — | `{}` |
//! | POST | `/api/open_path` | `{path}` | `{}` |
//! | GET | `/api/stream` | — | SSE stream of `ShellEvent` |
//!
//! The browser frontend uses `fetch` for request-response endpoints and
//! `EventSource` for the `/api/stream` SSE channel.

use std::convert::Infallible;

use axum::{
    extract::State,
    http::StatusCode,
    response::{sse::{Event as SseEvent, KeepAlive, Sse}, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::worker::ShellHandle;

/// Shared state for all handlers — the Shell handle (Clone + Send).
#[derive(Clone)]
pub struct AppState {
    pub shell: ShellHandle,
}

/// Build the axum router with all API routes + CORS.
pub fn create_router(shell: ShellHandle) -> Router {
    let state = AppState { shell };

    Router::new()
        .route("/api/command_list", get(command_list))
        .route("/api/history", get(history))
        .route("/api/complete", post(complete))
        .route("/api/prompt_context", get(prompt_context))
        .route("/api/run_command", post(run_command))
        .route("/api/cancel", post(cancel))
        .route("/api/open_path", post(open_path))
        // Plan 055 Phase A: 作业控制。
        .route("/api/jobs", get(jobs))
        .route("/api/kill_job", post(kill_job))
        // Plan 062 T11: NL→命令翻译(同步契约)+ 待回填建议拉取。
        .route("/api/nl2cmd", post(nl2cmd))
        .route("/api/ai_pending", get(ai_pending))
        // Plan 063 T1: suggest-next 建议列表(JSON 数组串,取后即清)。
        .route("/api/ai_next", get(ai_next))
        // Plan 063 T2: 最近一次翻译的拆步结果(\n 连接 str,取后即清)。
        .route("/api/ai_steps", get(ai_steps))
        // Plan 064 T2: 开机脚本命令(静态 env 读,空串 = 无)。
        .route("/api/boot_script", get(boot_script))
        .route("/api/stream", get(stream_sse))
        .with_state(state)
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn command_list(State(state): State<AppState>) -> impl IntoResponse {
    match state.shell.command_list().await {
        Ok(snap) => Json(snap).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn history(State(_state): State<AppState>) -> impl IntoResponse {
    Json(crate::worker::read_history())
}

#[derive(Deserialize)]
struct CompleteBody {
    line: String,
    cursor: usize,
}

async fn complete(
    State(state): State<AppState>,
    Json(body): Json<CompleteBody>,
) -> impl IntoResponse {
    match state.shell.complete(body.line, body.cursor).await {
        Ok(items) => Json(items).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn prompt_context(State(state): State<AppState>) -> impl IntoResponse {
    match state.shell.prompt_context().await {
        Ok(ctx) => Json(ctx).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
struct RunCommandBody {
    block_id: usize,
    cmd: String,
}

async fn run_command(
    State(state): State<AppState>,
    Json(body): Json<RunCommandBody>,
) -> impl IntoResponse {
    // Non-blocking — result arrives via the SSE stream.
    state.shell.run_command(body.block_id, body.cmd);
    StatusCode::OK
}

#[derive(Deserialize)]
struct RunSmartBody {
    block_id: usize,
    name: String,
    args: Vec<String>,
}

async fn cancel(State(state): State<AppState>) -> impl IntoResponse {
    state.shell.cancel();
    StatusCode::OK
}

/// Plan 055 Phase A: 列出后台作业(`cmd &`)。
async fn jobs(State(state): State<AppState>) -> impl IntoResponse {
    match state.shell.jobs().await {
        Ok(j) => Json(j).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
struct KillJobBody {
    job_id: u32,
}

/// Plan 055 Phase A: kill 后台作业。
async fn kill_job(
    State(state): State<AppState>,
    Json(body): Json<KillJobBody>,
) -> impl IntoResponse {
    state.shell.kill_job(body.job_id);
    StatusCode::OK
}

// ── Plan 062 T11: NL→command ────────────────────────────────────────────────

#[derive(Deserialize)]
struct Nl2CmdBody {
    nl: String,
}

/// 同步翻译(契约/测试用;返回裸 JSON 字符串)。
async fn nl2cmd(
    State(state): State<AppState>,
    Json(body): Json<Nl2CmdBody>,
) -> impl IntoResponse {
    match state.shell.nl2cmd(body.nl).await {
        Ok(payload) => Json(payload).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// 待回填的 AI 建议命令(取后即清;空串 = 无)。
async fn ai_pending(State(_state): State<AppState>) -> impl IntoResponse {
    Json(crate::worker::read_ai_pending()).into_response()
}

/// Plan 063 T1: suggest-next 建议列表(JSON 数组串,"[]" = 无,取后即清)。
async fn ai_next(State(_state): State<AppState>) -> impl IntoResponse {
    Json(crate::worker::read_ai_next()).into_response()
}

/// Plan 063 T2: 最近一次翻译的拆步结果(\n 连接 str,空 = 无,取后即清)。
async fn ai_steps(State(_state): State<AppState>) -> impl IntoResponse {
    Json(crate::worker::read_ai_steps()).into_response()
}

/// Plan 064 T2: 开机脚本命令(空串 = 无;前端 Init 拉取后整体提交)。
async fn boot_script(State(_state): State<AppState>) -> impl IntoResponse {
    Json(crate::worker::boot_script_cmd()).into_response()
}

#[derive(Deserialize)]
struct OpenPathBody {
    path: String,
}

async fn open_path(Json(body): Json<OpenPathBody>) -> impl IntoResponse {
    let _ = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &body.path])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&body.path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&body.path).spawn()
    };
    StatusCode::OK
}

// ── SSE streaming ───────────────────────────────────────────────────────────

/// SSE endpoint: subscribes to the Shell worker's event broadcast and forwards
/// each `ShellEvent` as an SSE frame. The browser uses `EventSource('/api/stream')`.
///
/// Each frame is `data: <json of ShellEvent>\n\n`. The `ShellEvent` enum is
/// tagged with `#[serde(tag = "event")]` so the frontend can discriminate by
/// event type.
async fn stream_sse(State(state): State<AppState>) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = state.shell.subscribe();
    // BroadcastStream wraps the broadcast receiver into a Stream, handling
    // Lagged errors gracefully (skips them).
    let s = BroadcastStream::new(rx).filter_map(|result| {
        result.ok().map(|event| {
            let json = serde_json::to_string(&event).unwrap_or_default();
            Ok(SseEvent::default().data(json))
        })
    });

    Sse::new(s).keep_alive(KeepAlive::default())
}
