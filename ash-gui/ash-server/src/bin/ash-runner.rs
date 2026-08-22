// ash-runner —— Plan 060 M3:merged 模式宿主 runner(对齐 015-notes 双模式)。
//
// 架构(与 docs/plans/060-api-contract-unification.md §M3 一致):
//
//   .at GUI(auto-man run_vm_ui 编排,进程内)
//     └─ api.at 桩(shell.at 空桩)→ auto.host.call_value(name, args_json)
//         └─ auto-lang 宿主桥注册表(纯机制)
//             └─ 本 runner 注册的桥函数(直调,无 socket)
//                 └─ ash_server::worker(auto_shell::Shell → ash-core)
//                     └─ ShellEvent broadcast → 本文件事件泵
//                         └─ auto_lang::inject_shell_event(SSE 同格式)
//                             └─ 前端事件泵 → blocks 回写
//
// 后端逻辑 100% 在 ash-server/ash-core;HTTP 模式(ash-server bin)与本
// runner 共享同一 worker 实现,语义零分叉。
//
// 启动:在 ash-gui-auto 项目目录执行 `ash-runner`(等价旧 `auto run -r vm`,
// 但后端为真 Shell 而非 .at mock)。

use std::sync::Arc;

use ash_server::worker::{self, ShellHandle};
use ash_server::SmartResult;

fn main() {
    // ── 1. 进程内起 Shell worker(不起 axum;与 ash-server bin 共享实现)──
    let shell = worker::spawn();

    // 桥函数用的阻塞式 runtime(host.call 在 VM/UI 线程同步进入,毫秒级
    // 往返;worker 不回调 UI 线程,无死环)。
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("ash-runner: failed to build bridge runtime");

    // 等待 worker 初始化完成(boot 快照就绪;有界轮询)。
    if let Err(e) = rt.block_on(shell.command_list()) {
        eprintln!("ash-runner: shell worker boot failed: {}", e);
        std::process::exit(1);
    }

    // ── 2. 事件泵:ShellEvent broadcast → SSE 同格式 JSON 注入前端 ──
    {
        let mut events = shell.subscribe();
        std::thread::Builder::new()
            .name("ash-runner-event-pump".into())
            .spawn(move || loop {
                match events.blocking_recv() {
                    Ok(ev) => {
                        if let Ok(json) = serde_json::to_string(&ev) {
                            let tag = json_get_event_tag(&json);
                            let _ok = auto_lang::ui::iced::renderer::inject_shell_event(&tag, &json);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[ash-runner] event pump lagged, dropped {}", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            })
            .expect("ash-runner: failed to spawn event pump");
    }

    // ── 3. 注册 api.at 契约端点的宿主桥(参数/返回 JSON,与 HTTP 同构)──
    register_bridges(&shell, std::sync::Arc::new(rt));

    // ── 4. merged 环境 + 起 .at GUI(auto-man 编排,等价 auto run -r vm)──
    std::env::remove_var("AUTO_BACKEND"); // 确保 merged(非 HTTP)分派
    let project_dir = std::env::current_dir().expect("ash-runner: no cwd");
    eprintln!("ash-runner: merged mode (in-process shell backend), project = {}", project_dir.display());
    if let Err(e) = auto_man::rust_ui::run_vm_ui(&project_dir, std::env::args().skip(1).collect()) {
        eprintln!("ash-runner: GUI exited with error: {:?}", e);
        std::process::exit(1);
    }
}

/// 从 ShellEvent JSON 里取 event 判别键(payload["event"])。
fn json_get_event_tag(json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("event").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_default()
}

fn register_bridges(shell: &ShellHandle, rt: std::sync::Arc<tokio::runtime::Runtime>) {
    use auto_lang::vm::host_bridge::register_host_call;

    // GET /api/command_list → BootSnapshot
    let s = shell.clone();
    let rt1 = rt.clone();
    register_host_call("command_list", Arc::new(move |_args: &str| {
        let snap = rt1.block_on(s.command_list()).map_err(|e| e)?;
        serde_json::to_string(&snap).map_err(|e| e.to_string())
    }));

    // GET /api/history → Vec<String>(worker 自由函数直读)
    register_host_call("history", Arc::new(|_args: &str| {
        serde_json::to_string(&worker::read_history()).map_err(|e| e.to_string())
    }));

    // POST /api/complete {line, cursor} → Vec<CompletionItem>
    let s = shell.clone();
    let rt2 = rt.clone();
    register_host_call("complete", Arc::new(move |args: &str| {
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
    register_host_call("prompt_context", Arc::new(move |_args: &str| {
        let ctx = rt3.block_on(s.prompt_context()).map_err(|e| e)?;
        serde_json::to_string(&ctx).map_err(|e| e.to_string())
    }));

    // POST /api/run_command {block_id, cmd} → {}(非阻塞;结果走事件流)
    let s = shell.clone();
    let rt4 = rt.clone();
    register_host_call("run_command", Arc::new(move |args: &str| {
        let v: serde_json::Value = serde_json::from_str(args)
            .map_err(|e| format!("run_command: bad args: {}", e))?;
        let block_id = v.get("block_id").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
        let cmd = v.get("cmd").and_then(|x| x.as_str()).unwrap_or("").to_string();
        s.run_command(block_id, cmd);
        Ok("{}".to_string())
    }));

    // POST /api/run_smart {block_id, name, args} → SmartResult(无 serde
    // derive,手拼 JSON;对齐 types.rs 字段 output/error)
    let s = shell.clone();
    let rt5 = rt.clone();
    register_host_call("run_smart", Arc::new(move |args: &str| {
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
    let rt6 = rt.clone();
    register_host_call("cancel", Arc::new(move |_args: &str| {
        s.cancel();
        Ok("{}".to_string())
    }));

    // POST /api/open_path {path} → {}(对齐 http.rs open_path 的跨平台打开)
    register_host_call("open_path", Arc::new(|args: &str| {
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
    register_host_call("jobs", Arc::new(move |_args: &str| {
        let jobs = rt7.block_on(s.jobs()).map_err(|e| e)?;
        serde_json::to_string(&jobs).map_err(|e| e.to_string())
    }));

    // POST /api/kill_job {job_id} → {}
    let s = shell.clone();
    let rt8 = rt.clone();
    register_host_call("kill_job", Arc::new(move |args: &str| {
        let v: serde_json::Value = serde_json::from_str(args)
            .map_err(|e| format!("kill_job: bad args: {}", e))?;
        let job_id = v.get("job_id").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        s.kill_job(job_id);
        Ok("{}".to_string())
    }));

    // GET /api/stream —— host 模式事件已由事件泵注入,HTTP SSE 语义不适用。
    register_host_call("stream", Arc::new(|_args: &str| Ok("{}".to_string())));
}
