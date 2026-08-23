// backend.rs — Plan 061:ash-server 作为外部后端 cdylib + 共享后端装配。
//
// 三种宿主形态共享同一装配逻辑(真 ash-core worker + 事件泵 + 10 端点注册):
//   1. cdylib(本文件 #[no_mangle] 导出):`auto run` merged 模式经
//      auto_lang::vm::backend_abi 装载注册(BackendRegistry 回调表);
//   2. ash-runner bin(过渡形态):同进程直调 assemble_host_bridge;
//   3. ash-server bin(HTTP):axum 路由直调 worker,不经此桥。
//
// 事件泵:worker 的 ShellEvent broadcast → renderer::inject_shell_event
// (SSE 同格式 JSON)。这是当前唯一宿主(auto run merged / ash-runner)
// 的事件通道;BackendRegistry::inject_event 保留给未来不同通道的宿主。

use std::sync::Arc;

use crate::worker::{self, ShellHandle};
use crate::SmartResult;

/// ABI 版本(与 auto_lang::vm::backend_abi::BACKEND_ABI_VERSION 对齐)。
/// 通过导出符号 `auto_backend_abi_version` 供宿主装载期校验。
#[no_mangle]
pub extern "Rust" fn auto_backend_abi_version() -> u32 {
    auto_lang::vm::backend_abi::BACKEND_ABI_VERSION
}

/// cdylib 注册入口(宿主装载后调用):起 worker + 事件泵,注册全部端点。
/// registry 以 Arc 交割 —— 事件泵线程经**宿主的** inject_event 回流事件
/// (cdylib 内的 auto_lang 副本不接 UI,不可用其 inject_shell_event)。
#[no_mangle]
pub extern "Rust" fn auto_backend_register(
    reg: std::sync::Arc<dyn auto_lang::vm::backend_abi::BackendRegistry>,
) -> Result<(), String> {
    let shell = assemble(reg);
    // boot 探活:① fail-fast(worker/ash-core 起不来立即报错而非静默);
    // ② 强制 Shell **立即**初始化 —— Shell 的会话 cwd 惰性取自首次调用
    // 时进程 cwd,而宿主随后会 chdir 到 src/front;此处先发制人,把会话
    // cwd 锁定在项目根(与 ash-runner 行为一致,auto run / bin 两形态同)。
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("boot-check runtime: {e}"))?;
    rt.block_on(shell.command_list())
        .map_err(|e| format!("shell worker boot failed: {e}"))?;
    Ok(())
}

/// 过渡形态(ash-runner bin)用的注册表适配器:直连 vm::host_bridge 与
/// renderer 事件注入(bin 场景只有一份 auto_lang,本地直调即正确)。
pub struct HostBridgeRegistry;

impl auto_lang::vm::backend_abi::BackendRegistry for HostBridgeRegistry {
    fn host_call(&self, name: &str, f: auto_lang::vm::backend_abi::BackendHostCallFn) {
        auto_lang::vm::host_bridge::register_host_call(name, f);
    }
    fn inject_event(&self, tag: &str, json: &str) -> bool {
        auto_lang::ui::iced::renderer::inject_shell_event(tag, json)
    }
    fn log(&self, msg: &str) {
        eprintln!("[ash-backend] {msg}");
    }
}

/// 过渡入口(bin 直调,不经 cdylib):装配 + 注册进 vm::host_bridge。
pub fn assemble_host_bridge() -> ShellHandle {
    assemble(std::sync::Arc::new(HostBridgeRegistry))
}

/// 装配完整后端:进程内 Shell worker + 阻塞 runtime + 事件泵 + 10 端点注册。
/// 返回 ShellHandle(调用方持有;worker 线程随进程生命周期)。
pub fn assemble(
    reg: std::sync::Arc<dyn auto_lang::vm::backend_abi::BackendRegistry>,
) -> ShellHandle {
    let shell = worker::spawn();

    // 桥函数用的阻塞式 runtime(host.call 在 VM/UI 线程同步进入,毫秒级
    // 往返;worker 不回调 UI 线程,无死环)。
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("ash-backend: failed to build bridge runtime");

    // 事件泵:ShellEvent broadcast → **宿主 registry** 的事件注入(SSE 同
    // 格式)。必须走 registry —— cdylib 场景进程内有两份 auto_lang(宿主 +
    // 本库),本地 inject_shell_event 写的是休眠副本,宿主 UI 收不到。
    {
        let mut events = shell.subscribe();
        let reg_pump = reg.clone();
        std::thread::Builder::new()
            .name("ash-backend-event-pump".into())
            .spawn(move || loop {
                match events.blocking_recv() {
                    Ok(ev) => {
                        if let Ok(json) = serde_json::to_string(&ev) {
                            let tag = json_get_event_tag(&json);
                            if std::env::var("ASH_DEBUG_JOBS").is_ok() && tag.starts_with("job") {
                                eprintln!("[dbg062] pump recv {tag}: {json}");
                            }
                            if !reg_pump.inject_event(&tag, &json) {
                                reg_pump.log(&format!("event inject dropped: {tag}"));
                            } else if std::env::var("ASH_DEBUG_JOBS").is_ok() && tag.starts_with("job") {
                                eprintln!("[dbg062] pump inject ok: {tag}");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[ash-backend] event pump lagged, dropped {}", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            })
            .expect("ash-backend: failed to spawn event pump");
    }

    register_bridges(&shell, Arc::new(rt), reg);
    shell
}

/// 从 ShellEvent JSON 里取 event 判别键(payload["event"])。
fn json_get_event_tag(json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("event").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_default()
}

/// api.at 契约的 10 端点注册(参数/返回 JSON,与 HTTP 同构)。
fn register_bridges(
    shell: &ShellHandle,
    rt: Arc<tokio::runtime::Runtime>,
    reg: std::sync::Arc<dyn auto_lang::vm::backend_abi::BackendRegistry>,
) {
    macro_rules! host_call {
        ($name:expr, $f:expr) => {
            reg.host_call($name, $f)
        };
    }
    // GET /api/command_list → BootSnapshot
    let s = shell.clone();
    let rt1 = rt.clone();
    host_call!("command_list", Arc::new(move |_args: &str| {
        let snap = rt1.block_on(s.command_list()).map_err(|e| e)?;
        serde_json::to_string(&snap).map_err(|e| e.to_string())
    }));

    // GET /api/history → Vec<String>(worker 自由函数直读)
    host_call!("history", Arc::new(|_args: &str| {
        serde_json::to_string(&worker::read_history()).map_err(|e| e.to_string())
    }));

    // POST /api/complete {line, cursor} → Vec<CompletionItem>
    let s = shell.clone();
    let rt2 = rt.clone();
    host_call!("complete", Arc::new(move |args: &str| {
        let v: serde_json::Value = serde_json::from_str(args)
            .map_err(|e| format!("complete: bad args: {}", e))?;
        let line = v.get("line").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let cursor = v.get("cursor").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        let items = rt2.block_on(s.complete(line, cursor)).map_err(|e| e)?;
        serde_json::to_string(&items).map_err(|e| e.to_string())
    }));

    // GET /api/prompt_context → PromptContext
    let s = shell.clone();
    let rt3 = rt.clone();
    host_call!("prompt_context", Arc::new(move |_args: &str| {
        let ctx = rt3.block_on(s.prompt_context()).map_err(|e| e)?;
        serde_json::to_string(&ctx).map_err(|e| e.to_string())
    }));

    // POST /api/run_command {block_id, cmd, cwd} → {}(非阻塞;结果走事件流)
    let s = shell.clone();
    host_call!("run_command", Arc::new(move |args: &str| {
        let v: serde_json::Value = serde_json::from_str(args)
            .map_err(|e| format!("run_command: bad args: {}", e))?;
        let block_id = v.get("block_id").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        let cmd = v.get("cmd").and_then(|x| x.as_str()).unwrap_or("").to_string();
        s.run_command(block_id, cmd);
        Ok("{}".to_string())
    }));

    // POST /api/run_smart {block_id, name, args} → SmartResult
    let s = shell.clone();
    let rt5 = rt.clone();
    host_call!("run_smart", Arc::new(move |args: &str| {
        let v: serde_json::Value = serde_json::from_str(args)
            .map_err(|e| format!("run_smart: bad args: {}", e))?;
        let block_id = v.get("block_id").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let cmd_args: Vec<String> = v
            .get("args")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|i| i.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let r: SmartResult = rt5.block_on(s.run_smart(block_id, name, cmd_args)).map_err(|e| e)?;
        Ok(format!(
            "{{\"output\":{},\"error\":{}}}",
            serde_json::to_string(&r.output).unwrap_or_default(),
            serde_json::to_string(&r.error).unwrap_or_default()
        ))
    }));

    // POST /api/cancel → {}
    let s = shell.clone();
    host_call!("cancel", Arc::new(move |_args: &str| {
        s.cancel();
        Ok("{}".to_string())
    }));

    // POST /api/open_path {path} → {}
    host_call!("open_path", Arc::new(|args: &str| {
        let v: serde_json::Value = serde_json::from_str(args)
            .map_err(|e| format!("open_path: bad args: {}", e))?;
        let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if !path.is_empty() {
            let _ = if cfg!(target_os = "windows") {
                std::process::Command::new("cmd").args(["/C", "start", "", &path]).spawn()
            } else if cfg!(target_os = "macos") {
                std::process::Command::new("open").arg(&path).spawn()
            } else {
                std::process::Command::new("xdg-open").arg(&path).spawn()
            };
        }
        Ok("{}".to_string())
    }));

    // GET /api/jobs → Vec<JobInfo>
    let s = shell.clone();
    let rt7 = rt.clone();
    host_call!("jobs", Arc::new(move |_args: &str| {
        let jobs = rt7.block_on(s.jobs()).map_err(|e| e)?;
        serde_json::to_string(&jobs).map_err(|e| e.to_string())
    }));

    // POST /api/kill_job {job_id} → {}
    let s = shell.clone();
    host_call!("kill_job", Arc::new(move |args: &str| {
        let v: serde_json::Value = serde_json::from_str(args)
            .map_err(|e| format!("kill_job: bad args: {}", e))?;
        let job_id = v.get("job_id").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        s.kill_job(job_id);
        Ok("{}".to_string())
    }));

    // GET /api/stream —— 事件已由事件泵注入,HTTP SSE 语义不适用。
    host_call!("stream", Arc::new(|_args: &str| Ok("{}".to_string())));
}
