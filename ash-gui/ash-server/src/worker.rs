//! Shell worker — owns the `!Send` Shell on a dedicated OS thread.
//!
//! This is the frontend-agnostic core extracted from
//! `ash-gui-vue/src-tauri/src/shell_worker.rs` (Plan 042 M1). The only change
//! is that output events go through a [`tokio::sync::broadcast`] channel
//! ([`ShellEvent`]) instead of `tauri::AppHandle::emit`. Both the HTTP (axum)
//! and Tauri transports subscribe to this channel and forward events in their
//! own format (SSE frames / Tauri events).
//!
//! See the crate-level docs (`lib.rs`) for the architecture overview.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ash_core::renderer::RenderedOutput;
use tokio::sync::{broadcast, mpsc};

use crate::types::*;

// ── Public handle (Clone + Send — safe for axum handlers / Tauri State) ──────

/// A request to the Shell worker thread.
enum CommandReq {
    /// Run `cmd`, attribute the result to `block_id`.
    Run { block_id: usize, cmd: String },
    /// Plan 057: periodic no-op that drives the job reaper — previously the
    /// reaper only ran when a new request arrived, so `job_done` events (and
    /// the removal from the jobs list) were delayed until the user's next
    /// interaction. Sent every 2s by a ticker task holding a Clone-able
    /// sender clone.
    Tick,
    /// Run a SmartCommand body against the worker's live Shell.
    /// Plan 063 T3: `reply` is `None` for the command-line `smart …` flow —
    /// the outcome then arrives as a `CommandResult` event (block model),
    /// exactly like the `?` NL flow. The HTTP `/api/run_smart` endpoint and
    /// the sidebar keep the synchronous oneshot reply.
    RunSmart {
        block_id: usize,
        name: String,
        args: Vec<String>,
        reply: Option<tokio::sync::oneshot::Sender<SmartResult>>,
    },
    /// Get the prompt context (git branch/status) for the current cwd.
    PromptContext {
        reply: tokio::sync::oneshot::Sender<PromptContext>,
    },
    /// Plan 055 Phase A: list background jobs.
    Jobs {
        reply: tokio::sync::oneshot::Sender<Vec<JobInfo>>,
    },
    /// Plan 055 Phase A: kill a background job by id.
    KillJob { job_id: u32 },
}

/// Plan 062 T10: completion requests travel on a DEDICATED channel/thread —
/// the main worker is serialized behind the running command, and the UI
/// thread's complete() host-call used to block for the whole command
/// duration (measured 27.6s behind `ping -n 30`).
struct CompleteReq {
    line: String,
    cursor: usize,
    reply: tokio::sync::oneshot::Sender<Vec<CompletionItem>>,
}

/// Plan 062 T11: NL→命令翻译请求(`?` 前缀提交)。`?` 流程带 `reply: None`
/// —— 结果以既有 `CommandResult` 事件回到块上(零新 SSE 事件族,引擎不动);
/// 同步 `/api/nl2cmd` 端点带 oneshot reply,直接取回 JSON 结果。
struct NlReq {
    block_id: usize,
    nl: String,
    reply: Option<tokio::sync::oneshot::Sender<String>>,
}

/// Plan 062 T12: 块内 AI chat 请求(`??` 前缀提交)。流式增量经既有
/// `CommandOutput` 事件写进块的 streamed_text(Running 态实时渲染),
/// 回合结束发 `CommandResult` 收尾 —— 复用 T11 的零引擎事件方案,
/// 对齐 CLI block_tui 的块内聊天形态(右侧抽屉面板留待引擎新事件族)。
struct ChatReq {
    block_id: usize,
    msg: String,
}

/// Plan 063 T3: load SmartCommands for the SHELL SESSION cwd. The VM host
/// chdirs to `src/front` at boot, so `loader::load_all()`'s process-cwd scan
/// finds nothing; the session cwd (`shell.pwd()`) is also the semantically
/// right "project-local" root (follows `cd`, like the CLI).
fn load_smart_specs(
    cwd: &std::path::Path,
) -> Vec<auto_shell::smart_command::config::SmartCommandSpec> {
    let home = home_dir().unwrap_or_default();
    let extra = auto_shell::plugin::loader::enabled_plugin_smart_dirs();
    auto_shell::smart_command::loader::load_all_with_extra(cwd, &home, &extra)
}

/// Plan 063 T3: smart NL 路由请求(命令行 `smart …` 词法:`smart run <名>`
/// 名字未命中时的回退,或裸自然语言)。路由在专用线程跑(Agent 整轮秒级,
/// 主 worker 零等待);命中后发回 `CommandReq::RunSmart { reply: None }` 回
/// 主循环按名执行(executor 需要 !Send 的 Shell,只有主线程有);未命中发
/// Failed 事件带可用命令建议。
struct SmartNlReq {
    block_id: usize,
    text: String,
}

/// Plan 062 T11: 最近一次翻译成功的命令。前端 RefreshContext(引擎在每个
/// command_result 后触发)经 `/api/ai_pending` 取后即清,用于把建议命令
/// 回填输入框(编辑入口,Pick 同语义)。静态槽镜像 CLI suggest-next 缓存
/// (ai/suggest.rs 的 PENDING)。
static AI_PENDING: Mutex<Option<String>> = Mutex::new(None);

/// Plan 063 T2: 最近一次翻译的拆步结果(`\n` 连接的 str,空 = 无/单步)。
/// 与 AI_PENDING 同款「槽位先落再发事件」:前端 RefreshContext 拉取后按
/// `?` 前缀定位翻译块写入本地 steps 字段 —— 不放进 AiSuggestion 事件
/// payload 再由 handler 深路径读(三层深读在 VM handler 静默中止),
/// str 槽 + api 拉取是 ai_pending 已验证的通道。
static AI_STEPS: Mutex<Option<String>> = Mutex::new(None);

/// 取走待回填的 AI 建议命令(空串 = 无)。api `ai_pending` 的 worker 侧实现。
pub fn read_ai_pending() -> String {
    match AI_PENDING.lock() {
        Ok(mut slot) => slot.take().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Plan 064 T2: 开机脚本命令(静态读 env,无 worker 队列开销)。
/// `ASH_BOOT_SCRIPT` 非空 → 返回完整 `script <路径>[ args…]` 命令串
/// (`ASH_BOOT_ARGS` 空格分词透传,脚本 `$1/$@` 可见);空 = 无开机脚本。
/// 前端 Init 拉取后整体提交(.RunCommand),不经用户输入。
pub fn boot_script_cmd() -> String {
    let path = match std::env::var("ASH_BOOT_SCRIPT") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return String::new(),
    };
    let mut cmd = format!("script {path}");
    if let Ok(a) = std::env::var("ASH_BOOT_ARGS") {
        let a = a.trim();
        if !a.is_empty() {
            cmd.push(' ');
            cmd.push_str(a);
        }
    }
    cmd
}

/// Plan 063 T2: 取走最近一次翻译的拆步结果(空串 = 无)。api `ai_steps` 的
/// worker 侧实现(取后即清)。
pub fn read_ai_steps() -> String {
    match AI_STEPS.lock() {
        Ok(mut slot) => slot.take().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Plan 063 T1: 取走 suggest-next 的建议命令列表(JSON 数组串,"[]" = 无)。
/// api `ai_next` 的 worker 侧实现 —— 直接 drain auto-shell crate 的 PENDING
/// 槽(`suggest_next_async` 后台线程写,取后即清),与 CLI repl 的「下个
/// 提示符前 take_pending」同语义;GUI 的等价拉取时机是 RefreshContext
/// (引擎在每个 command_result 后触发)。多值不走 AI_PENDING 单值槽。
pub fn read_ai_next() -> String {
    if std::env::var("ASH_DEBUG_SMART").is_ok() {
        eprintln!("[dbg063] read_ai_next called");
    }
    let out = auto_shell::ai::suggest::take_pending()
        .map(|v| serde_json::to_string(&v).unwrap_or_else(|_| "[]".to_string()))
        .unwrap_or_else(|| "[]".to_string());
    if std::env::var("ASH_DEBUG_SMART").is_ok() {
        eprintln!("[dbg063] read_ai_next -> {out}");
    }
    out
}

/// Plan 064: worker 启动时刻(boot 窗口判定 —— 开机脚本的延迟执行)。
static WORKER_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Session snapshot shared with the completion thread (its own Shell can't
/// see the main session's cd / last command). Completions never mutate the
/// session, so a snapshot is equivalent.
#[derive(Clone, Default)]
struct SharedSession {
    cwd: Arc<Mutex<String>>,
    last_command: Arc<Mutex<String>>,
    last_exit: Arc<std::sync::atomic::AtomicI32>,
}

/// Handle into the Shell worker. `Clone + Send` — stash in axum state or
/// `tauri::State`.
#[derive(Clone)]
pub struct ShellHandle {
    tx: mpsc::UnboundedSender<CommandReq>,
    /// Plan 062 T10: dedicated completion channel (own thread — never waits
    /// behind a running command).
    complete_tx: mpsc::UnboundedSender<CompleteReq>,
    /// Plan 062 T11: dedicated NL→command channel (own thread — a multi-second
    /// AI round trip must never block the serialized main worker).
    nl_tx: mpsc::UnboundedSender<NlReq>,
    /// Plan 062 T12: dedicated AI-chat channel (own thread — the ChatSession
    /// and its agent turns live there for the process lifetime).
    chat_tx: mpsc::UnboundedSender<ChatReq>,
    /// Cancel flag — set directly (concurrent) so it lands even while the
    /// worker is blocked in `spawn_blocking`. See Plan 040 M5.
    cancel: Arc<AtomicBool>,
    /// Subscribe to streaming events (command-output / command-result).
    event_rx: broadcast::Sender<ShellEvent>,
    /// Boot snapshot (filled by the worker thread at startup).
    boot: Arc<tokio::sync::Mutex<Option<BootSnapshot>>>,
}

impl ShellHandle {
    /// Boot data: cwd + command list + SmartCommands. Polls until the worker
    /// has initialized (bounded).
    pub async fn command_list(&self) -> Result<BootSnapshot, String> {
        for _ in 0..200 {
            if let Some(snap) = self.boot.lock().await.clone() {
                return Ok(snap);
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        Err("Shell worker failed to initialize in time".into())
    }

    /// Submit a command (non-blocking). The result arrives as a
    /// `ShellEvent::CommandResult` on the event stream.
    pub fn run_command(&self, block_id: usize, cmd: String) {
        let _ = self.tx.send(CommandReq::Run { block_id, cmd });
    }

    /// Run a SmartCommand by name on the worker's Shell.
    pub async fn run_smart(
        &self,
        block_id: usize,
        name: String,
        args: Vec<String>,
    ) -> Result<SmartResult, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(CommandReq::RunSmart {
                block_id,
                name,
                args,
                reply: Some(reply_tx),
            })
            .map_err(|_| "worker channel closed".to_string())?;
        reply_rx.await.map_err(|_| "worker dropped reply".to_string())
    }

    /// Produce completions for `line` at `cursor`.
    pub async fn complete(
        &self,
        line: String,
        cursor: usize,
    ) -> Result<Vec<CompletionItem>, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.complete_tx
            .send(CompleteReq {
                line,
                cursor,
                reply: reply_tx,
            })
            .map_err(|_| "completion channel closed".to_string())?;
        reply_rx.await.map_err(|_| "completion worker dropped reply".to_string())
    }

    /// Plan 062 T11: synchronous NL→command translation (contract/tests —
    /// the interactive `?` flow goes through `run_command` interception and
    /// receives its result as a CommandResult event instead). Returns a JSON
    /// string `{ok, cmd, notice, multi}` / `{ok:false, error}`.
    pub async fn nl2cmd(&self, nl: String) -> Result<String, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.nl_tx
            .send(NlReq {
                block_id: usize::MAX,
                nl,
                reply: Some(reply_tx),
            })
            .map_err(|_| "nl2cmd channel closed".to_string())?;
        reply_rx.await.map_err(|_| "nl2cmd worker dropped reply".to_string())
    }

    /// Get the prompt context (git branch/status).
    pub async fn prompt_context(&self) -> Result<PromptContext, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(CommandReq::PromptContext { reply: reply_tx })
            .map_err(|_| "worker channel closed".to_string())?;
        reply_rx.await.map_err(|_| "worker dropped reply".to_string())
    }

    /// Cancel the running command (concurrent — sets the flag directly).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Plan 055 Phase A: list background jobs (`cmd &`)。
    pub async fn jobs(&self) -> Result<Vec<JobInfo>, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(CommandReq::Jobs { reply: reply_tx })
            .map_err(|_| "worker channel closed".to_string())?;
        reply_rx.await.map_err(|_| "worker dropped reply".to_string())
    }

    /// Plan 055 Phase A: kill a background job by id.
    pub fn kill_job(&self, job_id: u32) {
        let _ = self.tx.send(CommandReq::KillJob { job_id });
    }

    /// Subscribe to streaming events (command-output / command-result).
    pub fn subscribe(&self) -> broadcast::Receiver<ShellEvent> {
        self.event_rx.subscribe()
    }
}

// ── Worker spawn ─────────────────────────────────────────────────────────────

/// Spawn the Shell worker thread. Returns a handle for commands + events.
///
/// The Shell is `!Send` (auto-lang VM uses `Rc`), so it lives on one dedicated
/// thread. This function constructs + initializes the Shell **before** entering
/// the Tokio runtime (Shell::new calls `blocking_lock` internally, which panics
/// inside a runtime context — see Plan 041 bugfix).
pub fn spawn() -> ShellHandle {
    let _ = WORKER_START.set(Instant::now());
    let (tx, mut rx) = mpsc::unbounded_channel::<CommandReq>();
    // Plan 062 T10: dedicated completion channel/thread.
    let (complete_tx, complete_rx) = mpsc::unbounded_channel::<CompleteReq>();
    // Plan 062 T11: dedicated NL→command channel/thread.
    let (nl_tx, nl_rx) = mpsc::unbounded_channel::<NlReq>();
    // Plan 062 T12: dedicated AI-chat channel/thread.
    let (chat_tx, chat_rx) = mpsc::unbounded_channel::<ChatReq>();
    // Plan 063 T3: dedicated smart NL-routing channel/thread (the multi-second
    // routing Agent round must never block the main worker).
    let (smart_nl_tx, smart_nl_rx) = mpsc::unbounded_channel::<SmartNlReq>();
    let cancel = Arc::new(AtomicBool::new(false));
    let (event_tx, _) = broadcast::channel::<ShellEvent>(256);
    let boot = Arc::new(tokio::sync::Mutex::new(None::<BootSnapshot>));
    // Plan 062 T10: session snapshot shared with the completion thread.
    let session = SharedSession::default();

    spawn_completion_worker(complete_rx, session.clone());
    // Plan 062 T11: nl worker shares the session snapshot (context) and the
    // event broadcast (CommandResult delivery).
    spawn_nl_worker(nl_rx, session.clone(), event_tx.clone());
    // Plan 062 T12: chat worker shares the session snapshot (context) and the
    // event broadcast (streaming + result delivery).
    spawn_chat_worker(chat_rx, session.clone(), event_tx.clone());
    // Plan 063 T3: smart routing worker — hits are sent back into the main
    // CommandReq queue (the executor needs the main thread's !Send Shell).
    spawn_smart_route_worker(smart_nl_rx, session.clone(), event_tx.clone(), tx.clone());

    let cancel_for_thread = cancel.clone();
    let event_tx_for_thread = event_tx.clone();
    let boot_for_thread = boot.clone();
    // Plan 062 T10: session snapshot updated by the main loop after each
    // command; read by the completion thread.
    let session_for_thread = session.clone();
    // Plan 057: worker-side sender clone for the job-reaper ticker.
    let tick_tx = tx.clone();
    // Plan 063 T3: main-loop self-addressed sender(`smart run <名>` 命中 →
    // RunSmart{reply: None} 转发回本队列,tick_tx 同款 clone)。
    let tx_for_loop = tx.clone();
    // Plan 062 T11: main-loop side of the nl channel (`?` interception).
    let nl_tx_for_loop = nl_tx.clone();
    // Plan 062 T12: main-loop side of the chat channel (`??` interception).
    let chat_tx_for_loop = chat_tx.clone();
    // Plan 063 T3: main-loop side of the smart-nl channel(`smart …` 拦截)。
    let smart_nl_tx_for_loop = smart_nl_tx.clone();

    std::thread::Builder::new()
        .name("ash-server-shell".into())
        .spawn(move || {
            // Construct + initialize the Shell BEFORE entering the runtime
            // (Shell::new → AutovmReplSession → blocking_lock panics in runtime).
            let mut shell = auto_shell::Shell::new();
            init_shell(&mut shell);
            // Plan 062 T11: 预填会话 cwd 快照 —— 首条命令(可能是 `?` 翻译)
            // 之前快照为空,翻译上下文会缺「当前目录」。
            if let Ok(mut c) = session_for_thread.cwd.lock() {
                *c = shell.pwd().to_string_lossy().to_string();
            }

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build worker runtime");

            let captured: Arc<Mutex<Option<RenderedOutput>>> = Arc::new(Mutex::new(None));
            let smart_block: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
            // Plan 063 T3: smart body 输出全量累计(事件收尾自带 Text)。
            let smart_acc: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
            // Plan 055 Phase A: 后台作业管理(复用 CLI JobManager,worker 线程局部)。
            let mut job_mgr = auto_shell::job::JobManager::new();

            runtime.block_on(async move {
                // M1: capture structured output via a RenderHook.
                shell.set_render_hook(Box::new(CaptureHook {
                    slot: captured.clone(),
                }));
                // M3: stream SmartCommand body output via OutputHook.
                shell.set_output_hook(Box::new(StreamingOutputHook {
                    block: smart_block.clone(),
                    event_tx: event_tx_for_thread.clone(),
                    acc: smart_acc.clone(),
                }));

                // Plan 057: drive the job reaper on a 2s ticker so job_done
                // events (and jobs-list removal) arrive in real time instead
                // of waiting for the next user request.
                {
                    let tick_tx = tick_tx.clone();
                    tokio::spawn(async move {
                        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                        loop {
                            interval.tick().await;
                            let _ = tick_tx.send(CommandReq::Tick);
                        }
                    });
                }

                // Boot snapshot.
                let snapshot = harvest_boot(&shell);
                *boot_for_thread.lock().await = Some(snapshot);

                // Main loop.
                while let Some(req) = rx.recv().await {
                    // Plan 055 Phase A: reap finished background jobs → JobDone。
                    for (jid, jcmd, jcode) in job_mgr.reap_finished() {
                        let _ = event_tx_for_thread.send(ShellEvent::JobDone {
                            job_id: jid,
                            exit_code: jcode,
                            cmd: jcmd,
                        });
                    }
                    match req {
                        // Plan 057: Tick only exists to run the reaper at the
                        // top of this loop (job_done events in real time).
                        CommandReq::Tick => {}
                        CommandReq::Run { block_id, cmd } => {
                            // Plan 062 T11: `?` 前缀 → NL→命令翻译(CLI F3 的
                            // GUI 等价)。拦截在历史展开之前 —— 问题文本不是
                            // 命令。块保持 Running 直到 nl 线程发 CommandResult
                            // (复用既有事件交付结果,零新 SSE 事件族,引擎不动)。
                            let trimmed_in = cmd.trim();
                            // Plan 062 T12: `??` 前缀 → 块内 AI chat(先于单 `?`
                            // 检查 —— "??" 同样以 '?' 开头)。多轮会话跨块持续
                            // (ChatSession 持久化 ~/.auto-shell-ai-chat.json),
                            // `?? /clear` 清空。
                            if let Some(chat_msg) = trimmed_in.strip_prefix("??") {
                                let chat_msg = chat_msg.trim().to_string();
                                if chat_msg.is_empty() {
                                    let cwd_err = shell.pwd().to_string_lossy().to_string();
                                    let _ = event_tx_for_thread.send(
                                        ShellEvent::CommandResult(CommandResult {
                                            block_id,
                                            cwd: cwd_err,
                                            status: CommandStatus::Failed(
                                                "AI chat 用法:?? <消息>(?? /clear 清空会话)".into(),
                                            ),
                                            output: RenderedOutput::Empty,
                                            duration_ms: 0,
                                            exit_code: -1,
                                        }),
                                    );
                                    continue;
                                }
                                let _ = event_tx_for_thread.send(ShellEvent::CommandOutput {
                                    block_id,
                                    chunk: "💬 AI 对话中…
".to_string(),
                                });
                                let _ = chat_tx_for_loop.send(ChatReq { block_id, msg: chat_msg });
                                let _ = append_history(trimmed_in);
                                continue;
                            }
                            if let Some(nl) = trimmed_in.strip_prefix('?') {
                                let nl = nl.trim().to_string();
                                if nl.is_empty() {
                                    let cwd_err = shell.pwd().to_string_lossy().to_string();
                                    let _ = event_tx_for_thread.send(
                                        ShellEvent::CommandResult(CommandResult {
                                            block_id,
                                            cwd: cwd_err,
                                            status: CommandStatus::Failed(
                                                "AI 用法:? <自然语言描述>(如 ? 列出当前目录的文件)"
                                                    .into(),
                                            ),
                                            output: RenderedOutput::Empty,
                                            duration_ms: 0,
                                            exit_code: -1,
                                        }),
                                    );
                                    continue;
                                }
                                // 即时提示行(Running 块内可见;command_result
                                // 到达时 update_block_in_state 清空 streamed_text)。
                                let _ = event_tx_for_thread.send(ShellEvent::CommandOutput {
                                    block_id,
                                    chunk: "⤾ AI 翻译中…\n".to_string(),
                                });
                                let _ = nl_tx_for_loop.send(NlReq { block_id, nl, reply: None });
                                let _ = append_history(trimmed_in);
                                continue;
                            }
                            // Plan 063 T3: `smart …` 命令行(与 CLI `ash smart`
                            // 同词法,worker 解析):`smart list` 列出;`smart run
                            // <名> [args]` 按名执行、名字未命中转 NL 路由(计划
                            // T3 的「run_smart 提交侧」语义);其余整体视为自然
                            // 语言走 nlu::route 专用线程。块保持 Running 直到
                            // 事件/转发收尾。
                            if trimmed_in == "smart" || trimmed_in.starts_with("smart ") {
                                if std::env::var("ASH_DEBUG_SMART").is_ok() {
                                    let cd = std::env::current_dir();
                                    eprintln!(
                                        "[dbg063] smart hit; proc cwd={cd:?}; smart/ exists={}",
                                        cd.as_ref().map(|c| c.join("smart").exists()).unwrap_or(false)
                                    );
                                }
                                let rest = trimmed_in["smart".len()..].trim().to_string();
                                let usage_err = |shell: &auto_shell::Shell, msg: &str| {
                                    let _ = event_tx_for_thread.send(ShellEvent::CommandResult(
                                        CommandResult {
                                            block_id,
                                            cwd: shell.pwd().to_string_lossy().to_string(),
                                            status: CommandStatus::Failed(msg.to_string()),
                                            output: RenderedOutput::Empty,
                                            duration_ms: 0,
                                            exit_code: -1,
                                        },
                                    ));
                                };
                                if rest.is_empty() {
                                    usage_err(
                                        &shell,
                                        "smart 用法:smart list | smart run <名> [args] | smart <自然语言>",
                                    );
                                    continue;
                                }
                                if rest == "list" {
                                    let specs = load_smart_specs(&shell.pwd());
                                    let mut body = String::from("SmartCommands:\n");
                                    for s in &specs {
                                        body.push_str(&format!("  {}\n    {}\n", s.name, s.description));
                                    }
                                    if specs.is_empty() {
                                        body.push_str(
                                            "没有 SmartCommand(在 ./smart/ 或 ~/.config/ash/smart/ 添加)",
                                        );
                                    }
                                    let cwd_l = shell.pwd().to_string_lossy().to_string();
                                    let _ = event_tx_for_thread.send(ShellEvent::CommandResult(
                                        CommandResult {
                                            block_id,
                                            cwd: cwd_l,
                                            status: CommandStatus::Success,
                                            output: RenderedOutput::Text(body),
                                            duration_ms: 0,
                                            exit_code: 0,
                                        },
                                    ));
                                    let _ = append_history(trimmed_in);
                                    continue;
                                }
                                if rest == "run" {
                                    usage_err(&shell, "smart run 用法:smart run <名> [args]");
                                    continue;
                                }
                                if let Some(run_rest) = rest.strip_prefix("run ") {
                                    let run_rest = run_rest.trim();
                                    if !run_rest.is_empty() {
                                        // parse_command 处理引号词法(与流式外部
                                        // 命令路径同源),CLI 的 argv 已分词等价。
                                        let parts =
                                            ash_core::cmd::external::parse_command(run_rest);
                                        if let Some(name) = parts.first() {
                                            let args = parts[1..].to_vec();
                                            let specs = load_smart_specs(&shell.pwd());
                                            if specs.iter().any(|s| s.name == *name) {
                                                // 命中 → 按名执行(自回环队列,
                                                // 事件流收尾)。
                                                let _ = tx_for_loop.send(
                                                    CommandReq::RunSmart {
                                                        block_id,
                                                        name: name.clone(),
                                                        args,
                                                        reply: None,
                                                    },
                                                );
                                            } else {
                                                // 未命中 → 整个 run_rest 作为
                                                // 自然语言走路由线程。
                                                let _ = smart_nl_tx_for_loop.send(SmartNlReq {
                                                    block_id,
                                                    text: run_rest.to_string(),
                                                });
                                            }
                                            let _ = append_history(trimmed_in);
                                            continue;
                                        }
                                    }
                                    usage_err(&shell, "smart run 用法:smart run <名> [args]");
                                    continue;
                                }
                                // 裸自然语言 → NL 路由。
                                let _ = smart_nl_tx_for_loop.send(SmartNlReq {
                                    block_id,
                                    text: rest,
                                });
                                let _ = append_history(trimmed_in);
                                continue;
                            }
                            // Plan 064 T1: `script <路径> [args…]` —— 运行
                            // .ash/.at 脚本文件,过程与结果落块(与 smart 执行
                            // 同一输出通道:smart_block 槽 + smart_acc 全量)。
                            // GUI 里手输跑脚本的正式入口(输入框直跑路径会被
                            // 流式外部命令的 not-found 预检错杀 —— 进程 cwd
                            // 是 src/front 而非会话 cwd;source 则输出哑)。
                            // 路径相对 shell 会话 cwd 解析;引号词法走
                            // parse_command;args 注入 $1/$@/#。
                            if trimmed_in == "script" || trimmed_in.starts_with("script ") {
                                let rest_s = trimmed_in["script".len()..].trim().to_string();
                                let usage_err_s = |shell: &auto_shell::Shell, msg: &str| {
                                    let _ = event_tx_for_thread.send(ShellEvent::CommandResult(
                                        CommandResult {
                                            block_id,
                                            cwd: shell.pwd().to_string_lossy().to_string(),
                                            status: CommandStatus::Failed(msg.to_string()),
                                            output: RenderedOutput::Empty,
                                            duration_ms: 0,
                                            exit_code: -1,
                                        },
                                    ));
                                };
                                if rest_s.is_empty() {
                                    usage_err_s(
                                        &shell,
                                        "script 用法:script <脚本路径> [args…](支持 .ash/.at)",
                                    );
                                    continue;
                                }
                                let parts_s = ash_core::cmd::external::parse_command(&rest_s);
                                let path_s = match parts_s.first() {
                                    Some(p) => p.clone(),
                                    None => {
                                        usage_err_s(&shell, "script 用法:script <脚本路径> [args…]");
                                        continue;
                                    }
                                };
                                // 相对路径相对会话 cwd(063 load_smart_specs 同因)。
                                let path_pb = std::path::Path::new(&path_s);
                                let resolved_s = if path_pb.is_absolute() {
                                    path_pb.to_path_buf()
                                } else {
                                    shell.pwd().join(path_pb)
                                };
                                if std::env::var("ASH_DEBUG_SMART").is_ok() {
                                    eprintln!(
                                        "[dbg064] script hit; path={path_s} resolved={resolved_s:?}"
                                    );
                                }
                                // Plan 064: 开机脚本延迟 —— Init 的 boot 提交
                                // 发生在 UI mount 前后(initial Task),worker
                                // 毫秒级完成的事件会在块链稳态前被消费丢弃
                                // (块卡 Running,实测)。开机窗口(15s)内识别
                                // 为 boot 命令(trimmed == boot_script_cmd 同源
                                // 构造)时延迟 1.5s 执行,等渲染器执行器/事件
                                // 泵进入稳态(ST-02 的稳态链已验证)。
                                if let Some(t0) = WORKER_START.get() {
                                    if t0.elapsed().as_secs() < 15
                                        && trimmed_in == boot_script_cmd()
                                    {
                                        std::thread::sleep(
                                            std::time::Duration::from_millis(1500),
                                        );
                                    }
                                }
                                let content_s = match std::fs::read_to_string(&resolved_s) {
                                    Ok(c) => c,
                                    Err(e) => {
                                        usage_err_s(
                                            &shell,
                                            &format!(
                                                "script: 无法读取 {}: {e}",
                                                resolved_s.display()
                                            ),
                                        );
                                        continue;
                                    }
                                };
                                let args_s: Vec<String> = parts_s[1..].to_vec();
                                shell.set_script_args(args_s.clone());
                                // 复用 SmartCommand 的输出通道:槽 + 全量累计
                                // (事件收尾带 Text,Empty 会清空 streamed_text)。
                                if let Ok(mut slot) = smart_block.lock() {
                                    *slot = Some(block_id);
                                }
                                if let Ok(mut a) = smart_acc.lock() {
                                    a.clear();
                                }
                                let started_s = Instant::now();
                                if std::env::var("ASH_DEBUG_SMART").is_ok() {
                                    eprintln!("[dbg064] script executing ({} bytes)", content_s.len());
                                }
                                let exec_result =
                                    shell.execute_script_content(&content_s);
                                if std::env::var("ASH_DEBUG_SMART").is_ok() {
                                    eprintln!("[dbg064] script done: {:?}", exec_result.is_ok());
                                }
                                if let Ok(mut slot) = smart_block.lock() {
                                    *slot = None;
                                }
                                let acc_text_s = smart_acc
                                    .lock()
                                    .map(|a| a.clone())
                                    .unwrap_or_default();
                                let (status_s, exit_s) = match exec_result {
                                    Ok(()) => (CommandStatus::Success, 0),
                                    Err(e) => {
                                        (CommandStatus::Failed(format!("{e}")), -1)
                                    }
                                };
                                let cwd_s = shell.pwd().to_string_lossy().to_string();
                                let _ = event_tx_for_thread.send(ShellEvent::CommandResult(
                                    CommandResult {
                                        block_id,
                                        cwd: cwd_s,
                                        status: status_s,
                                        output: RenderedOutput::Text(acc_text_s),
                                        duration_ms: started_s.elapsed().as_millis() as u64,
                                        exit_code: exit_s,
                                    },
                                ));
                                let _ = append_history(trimmed_in);
                                continue;
                            }
                            // Plan 062 T6: 提交侧历史展开(!!/!n/!string)——与
                            // CLI repl 同源同表(共享 ~/.auto-shell-history)。
                            // 展开失败(如 !999 越界)→ Failed 块,不执行。
                            // 注:块标题仍显示原始输入(展开结果对块不可见)。
                            let cmd = match expand_history_refs(&cmd) {
                                Ok(s) => s,
                                Err(e) => {
                                    let cwd_err = shell.pwd().to_string_lossy().to_string();
                                    let _ = event_tx_for_thread.send(
                                        ShellEvent::CommandResult(CommandResult {
                                            block_id,
                                            cwd: cwd_err,
                                            status: CommandStatus::Failed(format!(
                                                "history expansion: {e}"
                                            )),
                                            output: RenderedOutput::Empty,
                                            duration_ms: 0,
                                            exit_code: -1,
                                        }),
                                    );
                                    continue;
                                }
                            };
                            // Plan 055 Phase A: 后台 `cmd &` — spawn_external_background
                            // + 注册 job + 发 JobStarted(不阻塞主循环,reaper 下轮收)。
                            let trimmed = cmd.trim();
                            if trimmed.ends_with('&') && !trimmed.ends_with("&&") {
                                let cmd_part = trimmed.trim_end_matches('&').trim();
                                let cwd_bg = shell.pwd().to_path_buf();
                                match ash_core::cmd::external::spawn_external_background(
                                    cmd_part,
                                    &cwd_bg,
                                ) {
                                    Ok(child) => {
                                        let job_id = job_mgr.add(cmd_part.to_string(), child);
                                        let _ = event_tx_for_thread.send(ShellEvent::JobStarted {
                                            job_id,
                                            block_id,
                                            cmd: cmd_part.to_string(),
                                        });
                                        if std::env::var("ASH_DEBUG_JOBS").is_ok() {
                                            eprintln!("[dbg062] worker sent JobStarted id={job_id} cmd={cmd_part}");
                                        }
                                    }
                                    Err(e) => {
                                        let _ = event_tx_for_thread.send(ShellEvent::CommandResult(
                                            CommandResult {
                                                block_id,
                                                cwd: cwd_bg.to_string_lossy().to_string(),
                                                status: CommandStatus::Failed(format!("{e}")),
                                                output: RenderedOutput::Empty,
                                                duration_ms: 0,
                                                exit_code: -1,
                                            },
                                        ));
                                    }
                                }
                            } else if let Some(note) = console_handover_reason(trimmed) {
                                // Plan 062 T1: 交互式命令(vim/ssh/REPL…)需要真
                                // 终端,GUI 的管道 stdio 会挂死 —— 移交独立系统
                                // 终端窗口,等待线程在进程退出后回填块结果。
                                cancel_for_thread.store(false, Ordering::SeqCst);
                                let cwd_iv = shell.pwd().to_path_buf();
                                match spawn_console_command(trimmed, &cwd_iv) {
                                    Ok(child) => {
                                        let cmd_iv = trimmed.to_string();
                                        let cancel_iv = cancel_for_thread.clone();
                                        let event_iv = event_tx_for_thread.clone();
                                        let wait = std::thread::Builder::new()
                                            .name(format!("console-handover-{block_id}"))
                                            .spawn(move || {
                                                wait_console_child(
                                                    child,
                                                    block_id,
                                                    cmd_iv,
                                                    cwd_iv,
                                                    cancel_iv,
                                                    event_iv,
                                                );
                                            });
                                        if wait.is_ok() {
                                            let _ = append_history(trimmed);
                                        }
                                    }
                                    Err(e) => {
                                        let _ = event_tx_for_thread.send(
                                            ShellEvent::CommandResult(CommandResult {
                                                block_id,
                                                cwd: cwd_iv.to_string_lossy().to_string(),
                                                status: CommandStatus::Failed(format!(
                                                    "无法启动系统终端:{e}"
                                                )),
                                                output: RenderedOutput::Text(note),
                                                duration_ms: 0,
                                                exit_code: -1,
                                            }),
                                        );
                                    }
                                }
                            } else {
                                cancel_for_thread.store(false, Ordering::SeqCst);
                                let started = Instant::now();
                                // Plan 057: run_command 现在带上流式路径的子进程
                                // 真实退出码(None = execute() 路径,沿用
                                // shell.last_exit_code())。非零码按 shell 语义
                                // 报 Failed("exit code N")—— 修复流式外部命令
                                // 失败(如未知命令经 cmd /C 退出 1)被误报
                                // Success/exit 0 的问题。
                                let (status, output, exit_code) =
                                    match run_command(
                                        &mut shell,
                                        &cmd,
                                        &captured,
                                        &cancel_for_thread,
                                        block_id,
                                        &event_tx_for_thread,
                                    )
                                    .await
                                    {
                                        Ok((out, streamed_code)) => {
                                            let code = streamed_code
                                                .unwrap_or_else(|| shell.last_exit_code());
                                            if code != 0 {
                                                (
                                                    CommandStatus::Failed(format!("exit code {code}")),
                                                    out,
                                                    code,
                                                )
                                            } else {
                                                (CommandStatus::Success, out, code)
                                            }
                                        }
                                        // Plan 060 R16:取消(run_command 的
                                        // Err("cancelled"),drain 检测到 cancel
                                        // flag 后 kill)映射为 Cancelled 终态,
                                        // 不再混入 Failed。
                                        Err(ref msg) if msg == "cancelled" => {
                                            (CommandStatus::Cancelled, RenderedOutput::Empty, -1)
                                        }
                                        Err(msg) => {
                                            (CommandStatus::Failed(msg), RenderedOutput::Empty, -1)
                                        }
                                    };
                                // Plan 057: 命令执行后重新读取 cwd。此前在 run_command
                                // 前捕获 shell.pwd() 是旧值——cd 等内置命令改变工作
                                // 目录后,标题栏 cwd 仍停在旧路径(VM 端 store cwd 从
                                // command_result.cwd 回写)。execute() 路径同步改
                                // shell 内部 pwd,故执行完重读即为新值;外部流式命令
                                // 不改 cwd,重读无副作用。
                let cwd = shell.pwd().to_string_lossy().to_string();
                // Plan 062 T10: refresh the shared session snapshot (completion
                // thread's live view of cwd / last command / exit code).
                if let Ok(mut c) = session_for_thread.cwd.lock() {
                    *c = cwd.clone();
                }
                if let Ok(mut lc) = session_for_thread.last_command.lock() {
                    *lc = cmd.clone();
                }
                session_for_thread
                    .last_exit
                    .store(exit_code, Ordering::SeqCst);
                // Plan 063 T1: suggest-next(CLI repl.rs 同款 best-effort)。
                // 钩在事件发送前:fake 后端同步落槽,由本事件触发的
                // RefreshContext 拉取时可见;真后端异步线程慢一步无妨 ——
                // PENDING 取后即清,下一个 command_result 的拉取补上。
                if auto_shell::ai::suggest::is_enabled() {
                    if std::env::var("ASH_DEBUG_SMART").is_ok() {
                        eprintln!("[dbg063] suggest hook fired for cmd={cmd:?}");
                    }
                    let snippet = match &output {
                        RenderedOutput::Text(s) => s.chars().take(200).collect(),
                        _ => String::new(),
                    };
                    auto_shell::ai::suggest::suggest_next_async(
                        cwd.clone(),
                        cmd.clone(),
                        snippet,
                    );
                }
                let _ = event_tx_for_thread.send(ShellEvent::CommandResult(
                    CommandResult {
                        block_id,
                        cwd,
                                        status,
                                        output,
                                        duration_ms: started.elapsed().as_millis() as u64,
                                        // Plan 054 M2: child exit code (0 = success).
                                        exit_code,
                                    },
                                ));
                                let _ = append_history(&cmd);
                            }
                        }
                        CommandReq::Jobs { reply } => {
                            let jobs: Vec<JobInfo> = job_mgr
                                .jobs_raw()
                                .iter()
                                .map(|(id, j)| JobInfo {
                                    id: *id,
                                    command: j.command.clone(),
                                    state: format!("{:?}", j.state),
                                    exit_code: None,
                                })
                                .collect();
                            let _ = reply.send(jobs);
                        }
                        CommandReq::KillJob { job_id } => {
                            if let Some(mut job) = job_mgr.remove(job_id) {
                                let _ = job.child.kill();
                            }
                        }
                        CommandReq::RunSmart { block_id, name, args, reply } => {
                            if let Ok(mut slot) = smart_block.lock() {
                                *slot = Some(block_id);
                            }
                            // Plan 063 T3: 本轮 smart 输出从零累计(事件收尾
                            // 带全量 Text —— Empty 会清空块 streamed_text)。
                            if let Ok(mut a) = smart_acc.lock() {
                                a.clear();
                            }
                            let started = Instant::now();
                            let specs = load_smart_specs(&shell.pwd());
                            let result = match specs.iter().find(|s| s.name == name) {
                                Some(spec) => match auto_shell::smart_command::executor::execute(
                                    spec, &args, &mut shell,
                                ) {
                                    Ok(()) => SmartResult { output: String::new(), error: None },
                                    Err(e) => SmartResult { output: String::new(), error: Some(format!("{e}")) },
                                },
                                None => SmartResult {
                                    output: String::new(),
                                    error: Some(format!("SmartCommand '{name}' not found")),
                                },
                            };
                            if let Ok(mut slot) = smart_block.lock() {
                                *slot = None;
                            }
                            match reply {
                                Some(reply) => {
                                    let _ = reply.send(result);
                                }
                                None => {
                                    // Plan 063 T3: 命令行 smart 路径(reply
                                    // None)以事件流收尾块。output 必须自带
                                    // Text(全量累计)—— Empty 是裸字符串变体,
                                    // 引擎 update_block_in_state 会清空
                                    // streamed_text 且不回退(DEBTS 已知限制,
                                    // chat 线程同款处理);失败也带已流出增量。
                                    let cwd_now =
                                        shell.pwd().to_string_lossy().to_string();
                                    let acc_text = smart_acc
                                        .lock()
                                        .map(|a| a.clone())
                                        .unwrap_or_default();
                                    let (status, exit_code) = match &result.error {
                                        Some(e) => {
                                            (CommandStatus::Failed(e.clone()), -1)
                                        }
                                        None => (CommandStatus::Success, 0),
                                    };
                                    let _ = event_tx_for_thread
                                        .send(ShellEvent::CommandResult(
                                            CommandResult {
                                                block_id,
                                                cwd: cwd_now,
                                                status,
                                                output: RenderedOutput::Text(acc_text),
                                                duration_ms: started
                                                    .elapsed()
                                                    .as_millis() as u64,
                                                exit_code,
                                            },
                                        ));
                                }
                            }
                        }
                        // Plan 062 T10:Complete 已移驻独立补全线程(见 spawn_completion_worker)。
                        CommandReq::PromptContext { reply } => {
                            let cwd = shell.pwd().to_path_buf();
                            auto_shell::prompt::context::refresh_git_info_async(cwd);
                            let ctx = auto_shell::prompt::context::AshContext::new(
                                shell.pwd().to_path_buf(),
                                home_dir().unwrap_or_default(),
                                None,
                                Some(shell.last_exit_code()),
                                auto_shell::prompt::AshConfig::default(),
                            );
                            let pc = match ctx.git_info() {
                                Some(gi) => PromptContext {
                                    git_branch: Some(gi.branch),
                                    git_status: Some(GitStatusInfo {
                                        staged: gi.status.staged,
                                        unstaged: gi.status.unstaged,
                                        untracked: gi.status.untracked,
                                        conflicted: gi.status.conflicted,
                                        ahead: gi.status.ahead,
                                        behind: gi.status.behind,
                                    }),
                                },
                                None => PromptContext::default(),
                            };
                            let _ = reply.send(pc);
                        }
                    }
                }
            });
        })
        .expect("failed to spawn ash-server Shell worker thread");

    ShellHandle {
        tx,
        complete_tx,
        nl_tx,
        chat_tx,
        cancel,
        event_rx: event_tx,
        boot,
    }
}

/// Plan 062 T10: dedicated completion worker — own OS thread + own Shell +
/// own runtime. Completions never mutate session state, so a second Shell
/// (same init: registry/aliases/.ashrc) is equivalent; the live cwd / last
/// command come from the shared snapshot. This keeps typing responsive while
/// the main worker is serialized behind a running command.
fn spawn_completion_worker(
    rx: mpsc::UnboundedReceiver<CompleteReq>,
    session: SharedSession,
) {
    std::thread::Builder::new()
        .name("ash-server-complete".into())
        .spawn(move || {
            let mut shell = auto_shell::Shell::new();
            init_shell(&mut shell);
            // M7 (completion engine inputs) — same setup the main loop used.
            let completion_sigs: Vec<auto_shell::completions::CompletionSignature> =
                shell.registry().params().into_iter().map(Into::into).collect();
            let mut completion_provider = auto_shell::completions::CompletionProvider::new();
            auto_shell::completions::definitions::register_all(&mut completion_provider);
            load_tier_specs(&mut completion_provider);

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build completion worker runtime");

            runtime.block_on(async move {
                let mut rx = rx;
                while let Some(req) = rx.recv().await {
                    let ctx = completion_ctx_shared(
                        &shell,
                        &session,
                    );
                    let completions = auto_shell::completions::engine::complete(
                        &req.line,
                        req.cursor,
                        &completion_sigs,
                        &mut completion_provider,
                        &ctx,
                    );
                    let items: Vec<CompletionItem> =
                        completions.into_iter().map(completion_to_item).collect();
                    let _ = req.reply.send(items);
                }
            });
        })
        .expect("failed to spawn ash-server completion worker thread");
}

/// completion_ctx for the completion thread: live bits (cwd / last command /
/// exit code) from the shared snapshot; aliases from its own Shell.
fn completion_ctx_shared(
    shell: &auto_shell::Shell,
    session: &SharedSession,
) -> auto_shell::completions::engine::CompletionCtx {
    let history = history_file()
        .map(|p| read_recent_history(&p, 50))
        .unwrap_or_default();
    auto_shell::completions::engine::CompletionCtx {
        current_dir: std::path::PathBuf::from(
            session.cwd.lock().map(|c| c.clone()).unwrap_or_default(),
        ),
        last_command: session
            .last_command
            .lock()
            .ok()
            .and_then(|c| if c.is_empty() { None } else { Some(c.clone()) }),
        last_exit_code: Some(session.last_exit.load(Ordering::SeqCst)),
        history,
        aliases: shell.aliases().clone(),
    }
}

// ── Plan 062 T11: NL→command worker ─────────────────────────────────────────

/// Dedicated NL→command worker (T10 completion-thread pattern): own OS thread
/// so a multi-second AI round trip never blocks the serialized main worker
/// nor the UI. Context (cwd / last command / exit code) comes from the shared
/// session snapshot; the alias layer (L2) is skipped — the snapshot carries
/// no alias table and a second Shell just for aliases is not worth it for a
/// translation prompt.
fn spawn_nl_worker(
    rx: mpsc::UnboundedReceiver<NlReq>,
    session: SharedSession,
    event_tx: broadcast::Sender<ShellEvent>,
) {
    std::thread::Builder::new()
        .name("ash-server-nl2cmd".into())
        .spawn(move || {
            // Multi-thread runtime held for the thread's life — mirrors the
            // CLI ask_ai requirement (repl.rs): current-thread runtimes can
            // panic on drop under tokio >=1.52 in some contexts.
            let runtime = tokio::runtime::Runtime::new()
                .expect("failed to build nl2cmd worker runtime");
            let mut rx = rx;
            // AiClient::new() probes (and lazily starts) the aaid daemon — a
            // blocking call that must NOT run inside a runtime context, so
            // the client is built in the sync part of each iteration and
            // cached across requests (rebuilt after an error so a daemon
            // restart heals on the next request).
            let mut client: Option<auto_ai_client::AiClient> = None;
            while let Some(req) = rx.blocking_recv() {
                let started = Instant::now();
                let translation =
                    translate_nl(&mut client, &session, &req.nl, &runtime);
                let duration_ms = started.elapsed().as_millis() as u64;
                let cwd = session
                    .cwd
                    .lock()
                    .map(|c| c.clone())
                    .unwrap_or_default();
                match translation {
                    Ok((cmd, notice, multi, steps)) => {
                        let payload = serde_json::json!({
                            "ok": true,
                            "cmd": cmd,
                            "notice": notice,
                            "multi": multi,
                            "steps": steps,
                        })
                        .to_string();
                        if let Some(reply) = req.reply {
                            let _ = reply.send(payload);
                        } else {
                            // 槽位先于事件写入:RefreshContext 由本事件触发,
                            // 先落槽保证 ai_pending 拉取时可见。
                            if let Ok(mut slot) = AI_PENDING.lock() {
                                *slot = Some(cmd.clone());
                            }
                            // Plan 063 T2: 拆步结果同款落槽(多步才有内容;
                            // 单步落空串,前端按块清拆)。
                            if let Ok(mut slot) = AI_STEPS.lock() {
                                let joined = if multi {
                                    steps.join("\n")
                                } else {
                                    String::new()
                                };
                                *slot = Some(joined);
                            }
                            let _ = event_tx.send(ShellEvent::CommandResult(
                                CommandResult {
                                    block_id: req.block_id,
                                    cwd,
                                    status: CommandStatus::Success,
                                    output: RenderedOutput::AiSuggestion {
                                        question: req.nl,
                                        cmd,
                                        notice,
                                        multi,
                                        steps,
                                    },
                                    duration_ms,
                                    exit_code: 0,
                                },
                            ));
                        }
                    }
                    Err(msg) => {
                        let payload =
                            serde_json::json!({ "ok": false, "error": msg }).to_string();
                        if let Some(reply) = req.reply {
                            let _ = reply.send(payload);
                        } else {
                            let _ = event_tx.send(ShellEvent::CommandResult(
                                CommandResult {
                                    block_id: req.block_id,
                                    cwd,
                                    status: CommandStatus::Failed(msg),
                                    output: RenderedOutput::Empty,
                                    duration_ms,
                                    exit_code: -1,
                                },
                            ));
                        }
                    }
                }
            }
        })
        .expect("failed to spawn ash-server nl2cmd worker thread");
}

/// One NL→command translation — mirrors the CLI `ask_ai` (repl.rs:388-444):
/// fixed system prompt + snapshot context, `tier:mid` single-shot, code-fence
/// stripping, then the same pure validators (danger patterns / multi-step).
/// Plan 063 T2: also returns the `split_steps` breakdown so the GUI card can
/// render one row per step. Returns `(cmd, notice, multi, steps)`.
fn translate_nl(
    client: &mut Option<auto_ai_client::AiClient>,
    session: &SharedSession,
    nl: &str,
    runtime: &tokio::runtime::Runtime,
) -> Result<(String, String, bool, Vec<String>), String> {
    let cmd = if fake_ai_enabled() {
        fake_translate(nl)
    } else {
        if client.is_none() {
            *client = Some(
                auto_ai_client::AiClient::new()
                    .map_err(|e| format!("AI client init: {e}"))?,
            );
        }
        let system = format!(
            "You are an AI assistant for Ash (AutoShell), a shell similar to bash/fish.\n\
             {}\n\
             The user will describe what they want to do in natural language.\n\
             Translate it into a SINGLE ash shell command (or pipeline).\n\
             Rules:\n\
             - Respond with ONLY the command, no explanation, no markdown.\n\
             - Use standard Unix commands (ls, grep, find, etc.).\n\
             - For Ash-specific features, use: ls | .size > 10.mb | sort .name\n\
             - If multiple steps are needed, use && to chain them.\n\
             - If you're unsure, give your best single-command guess.",
            nl_context(session)
        );
        let req = auto_ai_client::CompletionRequest::single("tier:mid", nl)
            .with_system(&system)
            .with_max_tokens(256)
            .with_temperature(0.3);
        match runtime.block_on(client.as_ref().unwrap().complete(&req)) {
            Ok(resp) if resp.is_ok() => strip_code_fence(resp.content.trim()),
            Ok(resp) => {
                *client = None;
                return Err(format!("AI returned error: {:?}", resp.error));
            }
            Err(e) => {
                *client = None;
                return Err(format!(
                    "{e}(start the aaid daemon or set AAID_URL)"
                ));
            }
        }
    };
    let findings = auto_shell::ai::validate_suggestion(&cmd);
    let notice = findings
        .iter()
        .map(|f| match f {
            auto_shell::ai::ValidationFinding::Danger(m) => format!("⚠ 危险:{m}"),
            auto_shell::ai::ValidationFinding::Warning(m) => format!("⚠ {m}"),
        })
        .collect::<Vec<_>>()
        .join("; ");
    let steps = auto_shell::ai::split_steps(&cmd);
    let multi = steps.len() > 1;
    Ok((cmd, notice, multi, steps))
}

/// Snapshot context for the translation prompt — mirrors
/// `auto_shell::ai::context::build_context_block` (L0 OS/cwd + L1 last
/// command/exit; the alias layer needs a live Shell and is skipped here).
fn nl_context(session: &SharedSession) -> String {
    let mut lines = Vec::new();
    lines.push(format!("操作系统: {}", std::env::consts::OS));
    let cwd = session
        .cwd
        .lock()
        .map(|c| c.clone())
        .unwrap_or_default();
    if !cwd.is_empty() {
        lines.push(format!("当前目录: {cwd}"));
    }
    let last = session
        .last_command
        .lock()
        .ok()
        .filter(|c| !c.is_empty());
    if let Some(last) = last {
        lines.push(format!(
            "上一条命令: {last} (exit {})",
            session.last_exit.load(Ordering::SeqCst)
        ));
    }
    lines.join("\n")
}

/// Strip markdown code fences (same chain as the CLI ask_ai).
fn strip_code_fence(cmd: &str) -> String {
    cmd.trim_start_matches("```bash\n")
        .trim_start_matches("```sh\n")
        .trim_start_matches("```\n")
        .trim_end_matches("\n```")
        .trim()
        .to_string()
}

/// ASH_FAKE_AI (non-empty) swaps the model for a deterministic fake so tests
/// never touch the real daemon (plan 062 §5 fake-backend contract).
fn fake_ai_enabled() -> bool {
    std::env::var("ASH_FAKE_AI")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Deterministic fake translation: questions containing 危险/danger exercise
/// the danger validator (`rm -rf /` chain → Danger notice + multi-step card);
/// questions containing 多步/multi produce a harmless 3-step `&&` chain for
/// the step-execution acceptance (ST-01..03); everything else becomes an
/// executable echo carrying the question (assertable end-to-end).
fn fake_translate(nl: &str) -> String {
    let n = nl.trim();
    if n.contains("多步") || n.contains("multi") {
        "echo multi-a && echo multi-b && echo multi-c".to_string()
    } else if n.contains("danger") || n.contains("危险") {
        "rm -rf / && echo cleaned".to_string()
    } else {
        format!("echo fake-ai:{n}")
    }
}

// ── Plan 062 T12: block AI chat worker ──────────────────────────────────────

/// Dedicated AI-chat worker thread: owns the persistent [`ChatSession`]
/// (agent + ReAct + shell-backed tools) for the process lifetime. Streaming
/// deltas/tool events become `CommandOutput` chunks (block streamed_text,
/// rendered live while Running); the turn ends with a `CommandResult`
/// (`output: Empty` → the frontend falls back to the streamed text, same as
/// the execute path). `?? /clear` rebuilds the conversation.
fn spawn_chat_worker(
    rx: mpsc::UnboundedReceiver<ChatReq>,
    session: SharedSession,
    event_tx: broadcast::Sender<ShellEvent>,
) {
    std::thread::Builder::new()
        .name("ash-server-chat".into())
        .spawn(move || {
            // Multi-thread runtime held for the thread's life (agent turns are
            // driven per-request with block_on; client construction happens in
            // the sync ChatSession factories below, never inside the runtime).
            let runtime = tokio::runtime::Runtime::new()
                .expect("failed to build chat worker runtime");
            let mut rx = rx;
            // Lazy: built on the first turn. Rebuilt (None) after a load error
            // so a daemon restart heals on the next message.
            let mut chat: Option<auto_shell::ai::ChatSession> = None;
            while let Some(req) = rx.blocking_recv() {
                let started = Instant::now();
                let cwd = session
                    .cwd
                    .lock()
                    .map(|c| c.clone())
                    .unwrap_or_default();
                // 会话命令:/clear(与 CLI F4 同名指令)。
                if req.msg == "/clear" {
                    if let Some(c) = chat.as_mut() {
                        c.clear();
                        let _ = c.save();
                    }
                    let _ = event_tx.send(ShellEvent::CommandResult(CommandResult {
                        block_id: req.block_id,
                        cwd,
                        status: CommandStatus::Success,
                        output: RenderedOutput::Text("AI 会话已清空。".into()),
                        duration_ms: started.elapsed().as_millis() as u64,
                        exit_code: 0,
                    }));
                    continue;
                }
                if chat.is_none() {
                    match build_chat_session() {
                        Ok(c) => chat = Some(c),
                        Err(e) => {
                            let _ = event_tx.send(ShellEvent::CommandResult(
                                CommandResult {
                                    block_id: req.block_id,
                                    cwd,
                                    status: CommandStatus::Failed(format!(
                                        "AI chat init: {e}(start the aaid daemon or set AAID_URL)"
                                    )),
                                    output: RenderedOutput::Empty,
                                    duration_ms: 0,
                                    exit_code: -1,
                                },
                            ));
                            continue;
                        }
                    }
                }
                let c = chat.as_mut().unwrap();
                // 上下文走快照(与 nl2cmd 同款);ChatSession 不持 Shell。
                c.set_context_str(nl_context(&session));
                let user = req.msg.clone();
                let event_tx_cb = event_tx.clone();
                let bid = req.block_id;
                // 累计全量流文本:command_result 的 output 若为 Empty(裸字符串
                // "Empty",非 null)不会触发前端的 streamed_text 回退且会清空
                // streamed_text —— 收尾必须自带 Text(全文)。
                let acc = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                let acc_cb = acc.clone();
                let on_event: std::sync::Arc<dyn Fn(auto_ai_agent::StreamEvent) + Send + Sync> =
                    std::sync::Arc::new(move |ev| {
                        // 事件 → 增量文本行(对齐 CLI block_tui 的 ChatEv 渲染)。
                        let chunk = match ev {
                            auto_ai_agent::StreamEvent::Delta { text } => text,
                            auto_ai_agent::StreamEvent::ToolStart { tool, args } => {
                                format!("\n  ⚙ {tool} {}", auto_shell::ai::brief::brief_args(&args))
                            }
                            auto_ai_agent::StreamEvent::Tool { tool, result, .. } => {
                                format!("\n  ← {tool}: {}", auto_shell::ai::brief::brief_result(&result))
                            }
                            auto_ai_agent::StreamEvent::Warning { text } => {
                                format!("\n  ⚠ {text}")
                            }
                            auto_ai_agent::StreamEvent::Thinking { text } => {
                                format!("\n  💭 {text}")
                            }
                            auto_ai_agent::StreamEvent::Error { message } => {
                                format!("\n  [error] {message}")
                            }
                            _ => return,
                        };
                        if let Ok(mut a) = acc_cb.lock() {
                            a.push_str(&chunk);
                        }
                        let _ = event_tx_cb.send(ShellEvent::CommandOutput {
                            block_id: bid,
                            chunk,
                        });
                    });
                let turn = runtime.block_on(c.send_turn_streaming(&user, on_event));
                let duration_ms = started.elapsed().as_millis() as u64;
                let streamed = acc
                    .lock()
                    .map(|a| a.clone())
                    .unwrap_or_default();
                match turn {
                    Ok(_final_text) => {
                        // 持久化文本回合(工具消息已滤)。收尾带 Text(全量流
                        // 文本)—— command_result 会清空 streamed_text,若不带
                        // output,内容即丢失(Empty 是裸字符串变体,不触发回退)。
                        let _ = c.save();
                        let _ = event_tx.send(ShellEvent::CommandResult(CommandResult {
                            block_id: req.block_id,
                            cwd,
                            status: CommandStatus::Success,
                            output: RenderedOutput::Text(streamed),
                            duration_ms,
                            exit_code: 0,
                        }));
                    }
                    Err(msg) => {
                        // 失败但保留已流出的增量(错误信息进 status,文本进 output)。
                        if msg.contains("daemon unavailable") {
                            chat = None; // daemon 重启后下一条消息重建会话
                        }
                        let _ = event_tx.send(ShellEvent::CommandResult(CommandResult {
                            block_id: req.block_id,
                            cwd,
                            status: CommandStatus::Failed(msg),
                            output: RenderedOutput::Text(streamed),
                            duration_ms,
                            exit_code: -1,
                        }));
                    }
                }
            }
        })
        .expect("failed to spawn ash-server chat worker thread");
}

/// Build the ChatSession — real daemon client, or the deterministic fake
/// under `ASH_FAKE_AI` (same gate as the nl2cmd worker / ai_layer).
fn build_chat_session() -> Result<auto_shell::ai::ChatSession, String> {
    if fake_ai_enabled() {
        let client: std::sync::Arc<dyn auto_ai_agent::Client> =
            std::sync::Arc::new(FakeChatClient);
        return Ok(auto_shell::ai::ChatSession::with_client(client));
    }
    auto_shell::ai::ChatSession::load()
}

/// Plan 062 T12: deterministic chat client for tests — echoes the last user
/// message. Plain text (no tool calls) terminates the ReAct loop in one step.
struct FakeChatClient;

#[async_trait::async_trait]
impl auto_ai_agent::Client for FakeChatClient {
    async fn complete(
        &self,
        req: &auto_ai_client::CompletionRequest,
    ) -> Result<auto_ai_client::CompletionResponse, auto_ai_client::ClientError> {
        let last_user = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .and_then(|m| {
                Some(m
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        auto_ai_client::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(""))
            })
            .unwrap_or_default();
        Ok(auto_ai_client::CompletionResponse {
            content: format!("fake-chat:{last_user}"),
            tool_calls: vec![],
            stop_reason: Some("end_turn".into()),
            usage: None,
            model: "fake".into(),
            error: None,
            model_meta: None,
        })
    }
}

// ── Plan 063 T3: smart NL routing worker ────────────────────────────────────

/// Dedicated NL→SmartCommand routing worker (the T11 nl-thread pattern,
/// minus the runtime): own OS thread so the multi-second routing Agent round
/// never blocks the serialized main worker. `nlu::route` is fully synchronous
/// (it drives its one-shot runtime internally via `ai::block_on_async`), so
/// this thread needs no long-lived runtime. On a hit it sends
/// `CommandReq::RunSmart { reply: None }` back into the main loop (the
/// executor needs the main thread's !Send Shell); on a miss it emits the
/// block's Failed CommandResult with a hint listing the available commands.
fn spawn_smart_route_worker(
    rx: mpsc::UnboundedReceiver<SmartNlReq>,
    session: SharedSession,
    event_tx: broadcast::Sender<ShellEvent>,
    main_tx: mpsc::UnboundedSender<CommandReq>,
) {
    std::thread::Builder::new()
        .name("ash-server-smart-nlu".into())
        .spawn(move || {
            let mut rx = rx;
            while let Some(req) = rx.blocking_recv() {
                let started = Instant::now();
                let cwd = session
                    .cwd
                    .lock()
                    .map(|c| c.clone())
                    .unwrap_or_default();
                let specs = load_smart_specs(std::path::Path::new(&cwd));
                if specs.is_empty() {
                    let _ = event_tx.send(ShellEvent::CommandResult(CommandResult {
                        block_id: req.block_id,
                        cwd,
                        status: CommandStatus::Failed(
                            "没有可用的 SmartCommand(在 ./smart/ 或 ~/.config/ash/smart/ 添加)".into(),
                        ),
                        output: RenderedOutput::Empty,
                        duration_ms: started.elapsed().as_millis() as u64,
                        exit_code: -1,
                    }));
                    continue;
                }
                let routed = if fake_ai_enabled() {
                    // 测试假后端:走真 route 链(prompt 构建 + 输出解析 +
                    // spec 校验),client 换确定性 fake(见 FakeNluClient)。
                    let client: std::sync::Arc<dyn auto_ai_agent::Client> =
                        std::sync::Arc::new(FakeNluClient);
                    auto_shell::smart_command::nlu::route(&req.text, &specs, client)
                } else {
                    // Client per request(同步段构造,daemon 探测阻塞无妨 ——
                    // 路由低频,不值得缓存失效逻辑;nl worker 的缓存是为高频
                    // 翻译准备的)。
                    match auto_ai_client::AiClient::new() {
                        Ok(c) => {
                            let client: std::sync::Arc<dyn auto_ai_agent::Client> =
                                std::sync::Arc::new(c);
                            auto_shell::smart_command::nlu::route(&req.text, &specs, client)
                        }
                        Err(e) => Err(format!(
                            "AI client init: {e}(start the aaid daemon or set AAID_URL)"
                        )),
                    }
                };
                let duration_ms = started.elapsed().as_millis() as u64;
                match routed {
                    Ok(result) => {
                        // 命中 → 回主循环按名执行(route 已校验名字在 specs
                        // 里,必然命中),事件流收尾块。
                        let _ = main_tx.send(CommandReq::RunSmart {
                            block_id: req.block_id,
                            name: result.command,
                            args: result.args,
                            reply: None,
                        });
                    }
                    Err(msg) => {
                        // 未命中/路由失败 → Failed 带可用命令建议。
                        let names: Vec<&str> =
                            specs.iter().map(|s| s.name.as_str()).take(5).collect();
                        let hint = if specs.len() > 5 {
                            format!(
                                "可用命令(前 5/{}):{} … `smart list` 查看全部",
                                specs.len(),
                                names.join(", ")
                            )
                        } else {
                            format!("可用命令:{}", names.join(", "))
                        };
                        let _ = event_tx.send(ShellEvent::CommandResult(CommandResult {
                            block_id: req.block_id,
                            cwd,
                            status: CommandStatus::Failed(format!(
                                "SmartCommand 路由失败:{msg}\n{hint}"
                            )),
                            output: RenderedOutput::Text(format!("🤔 {}\n{hint}", req.text)),
                            duration_ms,
                            exit_code: -1,
                        }));
                    }
                }
            }
        })
        .expect("failed to spawn ash-server smart-nlu worker thread");
}

/// Plan 063 T3: deterministic routing client for tests (ASH_FAKE_AI gate).
/// Answers in the prescribed `COMMAND:`/`ARGS:` two-line format. It picks the
/// menu entry whose name starts with "zz" from the system prompt (the
/// test-injected SmartCommand `zz.smoke` in `$CWD/smart/`, falling back to the
/// first entry); a user message containing "nomatch" picks a non-existent
/// name to exercise the miss path.
struct FakeNluClient;

#[async_trait::async_trait]
impl auto_ai_agent::Client for FakeNluClient {
    async fn complete(
        &self,
        req: &auto_ai_client::CompletionRequest,
    ) -> Result<auto_ai_client::CompletionResponse, auto_ai_client::ClientError> {
        let user = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .and_then(|m| {
                Some(
                    m.content
                        .iter()
                        .filter_map(|b| match b {
                            auto_ai_client::ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(""),
                )
            })
            .unwrap_or_default();
        // Agent 的 role prompt 走 CompletionRequest.system_prompt 字段
        // (不在 messages 里),菜单行在其中。
        let system = req.system_prompt.clone().unwrap_or_default();
        if user.contains("nomatch") {
            return Ok(auto_ai_client::CompletionResponse {
                content: "COMMAND: no.such.command\nARGS: x".into(),
                tool_calls: vec![],
                stop_reason: Some("end_turn".into()),
                usage: None,
                model: "fake".into(),
                error: None,
                model_meta: None,
            });
        }
        // 菜单行形如 "- <name> <args>: <desc>"(build_nlu_prompt;无 args
        // 时名字直接带冒号,如 "- zz.smoke: desc",取词后剥尾冒号)。
        let mut picked = String::new();
        let mut first = String::new();
        for line in system.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("- ") {
                let name = rest.split_whitespace().next().unwrap_or("");
                let name = name.trim_end_matches(':');
                if name.is_empty() {
                    continue;
                }
                if first.is_empty() {
                    first = name.to_string();
                }
                if name.starts_with("zz") {
                    picked = name.to_string();
                    break;
                }
            }
        }
        if picked.is_empty() {
            picked = first;
        }
        Ok(auto_ai_client::CompletionResponse {
            content: format!("COMMAND: {picked}\nARGS: {user}"),
            tool_calls: vec![],
            stop_reason: Some("end_turn".into()),
            usage: None,
            model: "fake".into(),
            error: None,
            model_meta: None,
        })
    }
}

// ── Shell initialization (mirrors repl.rs:36-79) ────────────────────────────

pub fn init_shell(shell: &mut auto_shell::Shell) {
    shell.load_env_persistence();
    let shell_config = auto_shell::config::AshShellConfig::load();
    for (name, value) in &shell_config.aliases {
        shell.set_alias(name, value);
    }
    if let Some(home) = home_dir() {
        let rc_path = home.join(".ashrc");
        if rc_path.exists() {
            let _ = shell.source_file(&rc_path);
        } else if let Ok(content) = std::str::from_utf8(auto_shell::DEFAULT_ASHRC.as_bytes()) {
            let _ = std::fs::write(&rc_path, content);
            let _ = shell.source_file(&rc_path);
        }
    }
    if let Ok(report) = auto_shell::plugin::load_all_plugins(shell) {
        report.print_to_stderr();
    }
}

// ── Boot snapshot harvest ───────────────────────────────────────────────────

fn harvest_boot(shell: &auto_shell::Shell) -> BootSnapshot {
    let reg = shell.registry();
    let mut names: Vec<String> = reg.names().cloned().collect();
    names.sort();
    let commands: Vec<ToolEntry> = names
        .iter()
        .filter_map(|n| {
            reg.get(n).map(|cmd| {
                let sig = cmd.signature();
                ToolEntry { name: sig.name, description: sig.description }
            })
        })
        .collect();
    let specs = load_smart_specs(&shell.pwd());
    let smart_commands: Vec<SmartCommandEntry> = specs
        .iter()
        .map(|s| SmartCommandEntry { name: s.name.clone(), description: s.description.clone() })
        .collect();
    let home = home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();
    BootSnapshot {
        cwd: shell.pwd().to_string_lossy().to_string(),
        home,
        commands,
        smart_commands,
    }
}

// ── Completion helpers ──────────────────────────────────────────────────────

fn load_tier_specs(provider: &mut auto_shell::completions::CompletionProvider) {
    if let Some(dir) = auto_shell::completions::spec_tiers::generated_dir() {
        for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
            provider.register(spec);
        }
    }
    if let Some(dir) = auto_shell::completions::spec_tiers::user_dir() {
        for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
            provider.register(spec);
        }
    }
    for dir in auto_shell::plugin::loader::enabled_plugin_completion_dirs() {
        for spec in auto_shell::completions::spec_tiers::load_dir(&dir) {
            provider.register(spec);
        }
    }
}

fn completion_to_item(c: auto_shell::completions::Completion) -> CompletionItem {
    use auto_shell::completions::CompletionKind;
    let kind = match c.kind {
        CompletionKind::Command => "command",
        CompletionKind::External => "external",
        CompletionKind::File => "file",
        CompletionKind::Directory => "directory",
        CompletionKind::Variable => "variable",
        CompletionKind::Flag => "flag",
        CompletionKind::Subcommand => "subcommand",
        CompletionKind::AiSuggested => "ai",
    };
    CompletionItem {
        replacement: c.replacement,
        display: c.display,
        description: c.description,
        kind: kind.to_string(),
    }
}

// ── Command execution (Plan 040 M1 + M4 streaming) ──────────────────────────

/// Run a command. Tries the streaming path for simple external commands (M4),
/// otherwise falls back to `shell.execute()` (full preprocessing, M1).
/// Plan 057: returns `(rendered, exit_code)` — `exit_code` is `Some` only for
/// the streaming path (the child's real exit code); the `execute()` path
/// returns `None` and the caller uses `shell.last_exit_code()`.
async fn run_command(
    shell: &mut auto_shell::Shell,
    cmd: &str,
    captured: &Arc<Mutex<Option<RenderedOutput>>>,
    cancel: &Arc<AtomicBool>,
    block_id: usize,
    event_tx: &broadcast::Sender<ShellEvent>,
) -> Result<(RenderedOutput, Option<i32>), String> {
    if let Ok(mut slot) = captured.lock() {
        *slot = None;
    }

    // M4: streaming path for simple external commands.
    if let Some((rendered, exit_code)) =
        run_streaming_external(shell, cmd, cancel, block_id, event_tx).await?
    {
        return Ok((rendered, Some(exit_code)));
    }

    // Default: full preprocessing via shell.execute().
    let result = shell.execute(cmd);
    if let Ok(slot) = captured.lock() {
        if let Some(rendered) = slot.as_ref() {
            if !matches!(rendered, RenderedOutput::Empty) {
                return Ok((rendered.clone(), None));
            }
        }
    }
    match result {
        Ok(Some(s)) => Ok((RenderedOutput::Text(s), None)),
        Ok(None) => Ok((RenderedOutput::Empty, None)),
        Err(e) => Err(format!("{e}")),
    }
}

// ── Streaming external command path ─────────────────────────────────────────

async fn run_streaming_external(
    shell: &auto_shell::Shell,
    cmd: &str,
    cancel: &Arc<AtomicBool>,
    block_id: usize,
    event_tx: &broadcast::Sender<ShellEvent>,
) -> Result<Option<(RenderedOutput, i32)>, String> {
    use ash_core::cmd::external as ext;
    use ash_core::parser::{pipeline as chain_parser, pipe_stages, redirect};

    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.ends_with('&') {
        return Ok(None);
    }
    let (_clean, redirect) = redirect::parse_redirect(trimmed);
    if let Some(redir) = &redirect {
        if redir.stdout.is_some() || redir.stdin.is_some() {
            return Ok(None);
        }
    }
    let segments = chain_parser::parse_chain(trimmed);
    let has_logic = segments
        .iter()
        .any(|s| matches!(s.op, Some(chain_parser::ChainOp::And | chain_parser::ChainOp::Or)));
    if has_logic {
        return Ok(None);
    }
    let pipe_cmds: Vec<String> = segments.into_iter().map(|s| s.command).collect();

    // Plan 055 Phase B: 多段管道流式。每段必须是外部命令(非 builtin/auto/
    // alias/DSL stage),否则整条落 shell.execute 一次性。逐段过滤。
    for seg in &pipe_cmds {
        if pipe_stages::parse_pipe_stage(seg).is_some() {
            return Ok(None);
        }
        let parts = ext::parse_command(seg);
        if parts.is_empty() {
            return Ok(None);
        }
        let name = &parts[0];
        if shell.is_auto_expression_pub(seg)
            || shell.registry().get(name).is_some()
            || shell.has_auto_function(name)
            || shell.aliases().contains_key(name.as_str())
            || auto_shell::cmd::builtin::is_legacy_builtin(name)
            || is_shell_builtin(name)
        {
            return Ok(None);
        }
    }
    // terminal-only(color):单段返回提示(原行为);多段管道里无意义落 shell.execute。
    if pipe_cmds.len() == 1 {
        let name = ext::parse_command(&pipe_cmds[0]).remove(0);
        if is_terminal_only_command(&name) {
            return Ok(Some((RenderedOutput::Text(terminal_only_message(&name)), 0)));
        }
    } else if pipe_cmds.iter().any(|seg| {
        ext::parse_command(seg)
            .first()
            .map_or(false, |n| is_terminal_only_command(n))
    }) {
        return Ok(None);
    }

    // Plan 062 T3: command-not-found pre-check. The spawn fallback chain
    // (direct → powershell/sh) swallows unknown commands as a silent exit 1
    // with the error text stranded on inherited stderr — annotate them up
    // front instead. cmd.exe builtins that only resolve through the fallback
    // are whitelisted so `dir`/`type`/… keep working.
    for seg in &pipe_cmds {
        if let Some(name) = ext::parse_command(seg).first() {
            if !command_resolvable(name) {
                let mut msg = format!("command not found: {name}");
                if let Some(suggestion) = shell.suggest_command_for(name) {
                    msg.push_str(&format!("\n  did you mean: {suggestion}?"));
                }
                return Err(msg);
            }
        }
    }

    let cwd = shell.pwd();
    // OS pipe 链:首段 spawn_external_stream,后续 spawn_external_chained
    // (prev.stdout → next.stdin,kernel 级,同 CLI execute_pipeline_with_auto)。
    // cancel 只 kill 末段 handle,上游靠 SIGPIPE 自然退出(写已关闭的 pipe)。
    let mut stream = match ext::spawn_external_stream(&pipe_cmds[0], &cwd) {
        Ok(s) => s,
        Err(e) => return Err(format!("{e}")),
    };
    for seg in &pipe_cmds[1..] {
        let raw = stream.into_raw_stdout();
        stream = match ext::spawn_external_chained(seg, &cwd, raw) {
            Ok(s) => s,
            Err(e) => return Err(format!("{e}")),
        };
    }

    // Plan 057: drain 会消费 stream(lines() 取走 stdout),先克隆末段的退出
    // 状态句柄 —— 排空 stdout 后轮询取子进程真实退出码(此前流式路径从不
    // 上报退出码,未知命令经 cmd /C 退出 1 也被报成 Success/exit 0)。
    let status_handle = stream.exit_status_handle();

    let cancel_clone = cancel.clone();
    let event_tx_clone = event_tx.clone();
    let kill = stream.kill_handle();
    let rendered = tokio::task::spawn_blocking(move || {
        drain_stream(stream, block_id, &cancel_clone, &kill, &event_tx_clone)
    })
    .await
    .map_err(|e| format!("streaming task failed: {e}"))?;

    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".to_string());
    }
    // stdout EOF 通常与进程退出同时;短暂轮询等 status 线程落值,超时按 0
    // (避免竞态误报失败)。
    let mut exit_code = 0i32;
    for _ in 0..40 {
        let status = status_handle.lock().unwrap().clone();
        if let Some(st) = status {
            exit_code = st.code().unwrap_or(-1);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(Some((rendered, exit_code)))
}

fn drain_stream(
    stream: ash_core::pipeline::ExternalStream,
    block_id: usize,
    cancel: &Arc<AtomicBool>,
    kill: &Arc<Mutex<Option<u32>>>,
    event_tx: &broadcast::Sender<ShellEvent>,
) -> RenderedOutput {
    let mut buf = String::new();
    let mut cancelled = false;
    for line in stream.lines() {
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        match line {
            Ok(l) => {
                buf.push_str(&l);
                buf.push('\n');
                let _ = event_tx.send(ShellEvent::CommandOutput {
                    block_id,
                    chunk: format!("{l}\n"),
                });
            }
            Err(_) => break,
        }
    }
    if cancelled {
        ash_core::pipeline::ExternalStream::kill_from_handle(kill);
    }
    if buf.is_empty() {
        RenderedOutput::Empty
    } else {
        RenderedOutput::Text(buf)
    }
}

// ── Hooks (CaptureHook + StreamingOutputHook) ───────────────────────────────

/// Captures structured output (Table/Record) from `format_output`.
struct CaptureHook {
    slot: Arc<Mutex<Option<RenderedOutput>>>,
}

impl auto_shell::shell::RenderHook for CaptureHook {
    fn render_structured(
        &self,
        rendered: &RenderedOutput,
        _term_width: u16,
        _icons: auto_shell::config::IconStyle,
    ) -> Option<String> {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(rendered.clone());
        }
        None
    }
}

/// Forwards SmartCommand body output as `ShellEvent::CommandOutput`.
/// Plan 063 T3: also accumulates the full text — the RunSmart event
/// finalization must carry `Text(full)` (an `Empty` output makes the engine's
/// update_block_in_state CLEAR the block's streamed_text without fallback;
/// DEBTS known-limitation, chat worker already follows it).
struct StreamingOutputHook {
    block: Arc<Mutex<Option<usize>>>,
    event_tx: broadcast::Sender<ShellEvent>,
    acc: Arc<Mutex<String>>,
}

impl auto_shell::shell::OutputHook for StreamingOutputHook {
    fn emit(&self, output: &str) {
        if let Ok(mut a) = self.acc.lock() {
            a.push_str(output);
        }
        if let Ok(slot) = self.block.lock() {
            if let Some(block_id) = *slot {
                let _ = self.event_tx.send(ShellEvent::CommandOutput {
                    block_id,
                    chunk: output.to_string(),
                });
            }
        }
    }
}

// ── History persistence (Plan 040 M6) ───────────────────────────────────────

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
}

fn history_file() -> Option<std::path::PathBuf> {
    home_dir().map(|h| h.join(".auto-shell-history"))
}

fn append_history(cmd: &str) -> std::io::Result<()> {
    use std::io::Write;
    let line = cmd.trim();
    if line.is_empty() {
        return Ok(());
    }
    let path = match history_file() {
        Some(p) => p,
        None => return Ok(()),
    };
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let safe = line.replace('\n', " ");
    writeln!(f, "{safe}")?;
    Ok(())
}

/// Read the full history file (one command per line, oldest first).
pub fn read_history() -> Vec<String> {
    let path = match history_file() {
        Some(p) => p,
        None => return Vec::new(),
    };
    read_history_file(&path)
}

/// Plan 062 T6: history expansion (`!!` / `!n` / `!-n` / `!string` /
/// `!?string`) on the submit side — same source as the CLI REPL
/// (ash_core::parser::history::expand_history over the shared history file),
/// so both transports expand identically.
fn expand_history_refs(cmd: &str) -> Result<String, String> {
    struct FileHistory {
        strings: Vec<String>,
    }
    impl ash_core::parser::history::History for FileHistory {
        fn search(&self, _query: Option<&str>) -> Vec<String> {
            self.strings.clone()
        }
    }
    let fh = FileHistory {
        strings: read_history(),
    };
    ash_core::parser::history::expand_history(cmd, &fh).map_err(|e| format!("{e}"))
}

fn read_history_file(path: &std::path::Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(c) => c.lines().filter(|l| !l.trim().is_empty()).map(|l| l.to_string()).collect(),
        Err(_) => Vec::new(),
    }
}

fn read_recent_history(path: &std::path::Path, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let all = read_history_file(path);
    let len = all.len();
    if len <= n {
        all
    } else {
        all[len - n..].to_vec()
    }
}

// ── Terminal-only command degradation (Plan 041 M8) ─────────────────────────

fn is_shell_builtin(name: &str) -> bool {
    matches!(
        name,
        "cd" | "alias" | "unalias" | "source" | "." | "pushd" | "popd" | "dirs"
            | "jobs" | "fg" | "bg" | "suspend" | "def" | "hook" | "abbr" | "config"
            | "bind" | "up" | "u" | "b" | "set" | "export" | "unset" | "env"
            | "env.path" | "path" | "completions" | "use" | "exit" | "quit" | "q"
    )
}

/// Plan 062 T3: can `name` resolve to something runnable? PATH + PATHEXT on
/// Windows (plain lookup on Unix), explicit paths, and the cmd.exe builtins
/// that survive only via the powershell/sh spawn fallback.
fn command_resolvable(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.contains('/') || name.contains('\\') {
        return std::path::Path::new(name).exists();
    }
    const CMD_BUILTINS: &[&str] = &[
        "dir", "del", "copy", "ren", "rename", "move", "rd", "rmdir", "type",
        "cls", "ver", "vol", "title", "start", "mklink", "assoc", "ftype",
        "prompt", "setlocal", "endlocal", "call", "choice", "color", "where",
        "more", "sort", "tasklist", "taskkill", "timeout",
    ];
    if CMD_BUILTINS.contains(&name) {
        return true;
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![String::new()]
    };
    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path_var.split(sep).filter(|d| !d.is_empty()) {
        for ext in &exts {
            if std::path::Path::new(dir).join(format!("{name}{ext}")).exists() {
                return true;
            }
        }
    }
    false
}

fn is_terminal_only_command(name: &str) -> bool {
    // Plan 055 Phase C: less/more 放行 —— 无 tty 时多数实现直接 cat 内容到
    // stdout,GUI 走流式路径(spawn_external_stream)+ block 已可滚动浏览。
    // 只保留 color(终端真彩检测在 webview CSS 恒为真彩,无意义)。
    matches!(name, "color")
}

fn terminal_only_message(name: &str) -> String {
    match name {
        "color" => String::from(
            "color 检测终端色彩能力,依赖 crossterm 终端 API。\n\
             GUI 走 webview CSS 渲染,恒为 24-bit 真彩——无需检测。",
        ),
        _ => format!("{name} 是终端专属命令,GUI 不支持。"),
    }
}

// ── Plan 062 T1: interactive-command console handover ───────────────────────

/// REPL-style commands: bare invocation is an interactive REPL (needs a
/// terminal), but with arguments they run scripts — those stay on the
/// streaming path so output lands in the block. The CLI hands both to the
/// inherited terminal; a GUI block makes the distinction worth it.
const REPL_STYLE: &[&str] = &["python", "ipython", "node", "irb"];

/// Interactive-list members the GUI deliberately does NOT hand over:
/// pagers degrade gracefully without a tty (Plan 055 Phase C — they stream
/// their contents and the block scrolls).
const PAGER_COMMANDS: &[&str] = &["less", "more", "bat"];

/// If `cmd` should run in a fresh OS terminal window, return the note shown
/// in the block. Mirrors the CLI check (`repl.rs` → `interactive.rs`).
fn console_handover_reason(cmd: &str) -> Option<String> {
    let first = cmd.split_whitespace().next()?;
    if !ash_core::cmd::interactive::is_interactive_command(first) {
        return None;
    }
    let name = first
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(first)
        .trim_end_matches(".exe");
    if PAGER_COMMANDS.contains(&name) {
        return None;
    }
    if REPL_STYLE.contains(&name) && cmd.split_whitespace().count() > 1 {
        return None;
    }
    Some(format!(
        "`{name}` 是交互式命令,已移交系统终端窗口运行;关闭其窗口或点 Stop 结束。"
    ))
}

/// Spawn `cmd` in a brand-new console window (Windows) / terminal emulator
/// (Unix best-effort). The returned Child is the console wrapper process.
#[cfg(windows)]
fn spawn_console_command(
    cmd: &str,
    cwd: &std::path::Path,
) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0010_0000;
    // `cmd /C` 让命令在全新控制台里跑,控制台随命令退出关闭。
    std::process::Command::new("cmd")
        .raw_arg(format!("/C {cmd}"))
        .current_dir(cwd)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
}

#[cfg(unix)]
fn spawn_console_command(
    cmd: &str,
    cwd: &std::path::Path,
) -> std::io::Result<std::process::Child> {
    // $TERMINAL 优先,其后常见模拟器;sh -c 包装保持命令串完整。
    let mut terms: Vec<String> = Vec::new();
    if let Ok(t) = std::env::var("TERMINAL") {
        terms.push(t);
    }
    terms.extend(
        ["gnome-terminal", "konsole", "xfce4-terminal", "xterm"]
            .iter()
            .map(|s| s.to_string()),
    );
    for term in terms {
        let flag = if term.contains("gnome-terminal") { "--" } else { "-e" };
        let spawned = std::process::Command::new(&term)
            .arg(flag)
            .arg("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd)
            .spawn();
        if let Ok(child) = spawned {
            return Ok(child);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "未找到可用的终端模拟器(可设 $TERMINAL)",
    ))
}

/// Wait for a console-handover child, then emit the block's CommandResult.
/// Polls the worker cancel flag so the block's Stop button terminates the
/// handover. Plain `Child::kill` only hits the wrapper process — same
/// semantics as the existing background-job kill.
fn wait_console_child(
    mut child: std::process::Child,
    block_id: usize,
    cmd: String,
    cwd: std::path::PathBuf,
    cancel: Arc<AtomicBool>,
    event_tx: broadcast::Sender<ShellEvent>,
) {
    let started = Instant::now();
    let mut killed = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let (status, exit_code) = if killed {
                    (CommandStatus::Cancelled, -1)
                } else {
                    let code = status.code().unwrap_or(-1);
                    if code != 0 {
                        (CommandStatus::Failed(format!("exit code {code}")), code)
                    } else {
                        (CommandStatus::Success, code)
                    }
                };
                let _ = event_tx.send(ShellEvent::CommandResult(CommandResult {
                    block_id,
                    cwd: cwd.to_string_lossy().to_string(),
                    status,
                    output: RenderedOutput::Text(format!(
                        "交互式命令 `{cmd}` 已在系统终端窗口中结束。"
                    )),
                    duration_ms: started.elapsed().as_millis() as u64,
                    exit_code,
                }));
                return;
            }
            Ok(None) => {
                if !killed && cancel.load(Ordering::SeqCst) {
                    killed = true;
                    let _ = child.kill();
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            Err(_) => return,
        }
    }
}
