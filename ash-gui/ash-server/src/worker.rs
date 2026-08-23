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
    RunSmart {
        block_id: usize,
        name: String,
        args: Vec<String>,
        reply: tokio::sync::oneshot::Sender<SmartResult>,
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
                reply: reply_tx,
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
    let (tx, mut rx) = mpsc::unbounded_channel::<CommandReq>();
    // Plan 062 T10: dedicated completion channel/thread.
    let (complete_tx, complete_rx) = mpsc::unbounded_channel::<CompleteReq>();
    let cancel = Arc::new(AtomicBool::new(false));
    let (event_tx, _) = broadcast::channel::<ShellEvent>(256);
    let boot = Arc::new(tokio::sync::Mutex::new(None::<BootSnapshot>));
    // Plan 062 T10: session snapshot shared with the completion thread.
    let session = SharedSession::default();

    spawn_completion_worker(complete_rx, session.clone());

    let cancel_for_thread = cancel.clone();
    let event_tx_for_thread = event_tx.clone();
    let boot_for_thread = boot.clone();
    // Plan 062 T10: session snapshot updated by the main loop after each
    // command; read by the completion thread.
    let session_for_thread = session.clone();
    // Plan 057: worker-side sender clone for the job-reaper ticker.
    let tick_tx = tx.clone();

    std::thread::Builder::new()
        .name("ash-server-shell".into())
        .spawn(move || {
            // Construct + initialize the Shell BEFORE entering the runtime
            // (Shell::new → AutovmReplSession → blocking_lock panics in runtime).
            let mut shell = auto_shell::Shell::new();
            init_shell(&mut shell);

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build worker runtime");

            let captured: Arc<Mutex<Option<RenderedOutput>>> = Arc::new(Mutex::new(None));
            let smart_block: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
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
                            let specs = auto_shell::smart_command::loader::load_all();
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
                            let _ = reply.send(result);
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
    let specs = auto_shell::smart_command::loader::load_all();
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
struct StreamingOutputHook {
    block: Arc<Mutex<Option<usize>>>,
    event_tx: broadcast::Sender<ShellEvent>,
}

impl auto_shell::shell::OutputHook for StreamingOutputHook {
    fn emit(&self, output: &str) {
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
