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
    /// Produce completions for `line` at `cursor`.
    Complete {
        line: String,
        cursor: usize,
        reply: tokio::sync::oneshot::Sender<Vec<CompletionItem>>,
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

/// Handle into the Shell worker. `Clone + Send` — stash in axum state or
/// `tauri::State`.
#[derive(Clone)]
pub struct ShellHandle {
    tx: mpsc::UnboundedSender<CommandReq>,
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

    /// Produce completions via the shared engine.
    pub async fn complete(
        &self,
        line: String,
        cursor: usize,
    ) -> Result<Vec<CompletionItem>, String> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(CommandReq::Complete {
                line,
                cursor,
                reply: reply_tx,
            })
            .map_err(|_| "worker channel closed".to_string())?;
        reply_rx.await.map_err(|_| "worker dropped reply".to_string())
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
    let cancel = Arc::new(AtomicBool::new(false));
    let (event_tx, _) = broadcast::channel::<ShellEvent>(256);
    let boot = Arc::new(tokio::sync::Mutex::new(None::<BootSnapshot>));

    let cancel_for_thread = cancel.clone();
    let event_tx_for_thread = event_tx.clone();
    let boot_for_thread = boot.clone();
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

                // M7: completion engine inputs.
                let completion_sigs: Vec<auto_shell::completions::CompletionSignature> =
                    shell.registry().params().into_iter().map(Into::into).collect();
                let mut completion_provider = auto_shell::completions::CompletionProvider::new();
                auto_shell::completions::definitions::register_all(&mut completion_provider);
                load_tier_specs(&mut completion_provider);

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
                            } else {
                                cancel_for_thread.store(false, Ordering::SeqCst);
                                let cwd = shell.pwd().to_string_lossy().to_string();
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
                                        Err(msg) => {
                                            (CommandStatus::Failed(msg), RenderedOutput::Empty, -1)
                                        }
                                    };
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
                        CommandReq::Complete { line, cursor, reply } => {
                            let ctx = completion_ctx(&shell);
                            let completions = auto_shell::completions::engine::complete(
                                &line,
                                cursor,
                                &completion_sigs,
                                &mut completion_provider,
                                &ctx,
                            );
                            let items: Vec<CompletionItem> =
                                completions.into_iter().map(completion_to_item).collect();
                            let _ = reply.send(items);
                        }
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
        cancel,
        event_rx: event_tx,
        boot,
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

fn completion_ctx(shell: &auto_shell::Shell) -> auto_shell::completions::engine::CompletionCtx {
    let history = history_file()
        .map(|p| read_recent_history(&p, 50))
        .unwrap_or_default();
    auto_shell::completions::engine::CompletionCtx {
        current_dir: shell.pwd().to_path_buf(),
        last_command: shell.last_command_line().map(String::from),
        last_exit_code: Some(shell.last_exit_code()),
        history,
        aliases: shell.aliases().clone(),
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
