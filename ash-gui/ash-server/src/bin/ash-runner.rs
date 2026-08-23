// ash-runner —— Plan 060 M3 merged 模式宿主 runner(过渡形态)。
//
// Plan 061 后:装配逻辑已下沉 ash_server::backend(cdylib 与本 bin 共享);
// 待 auto run -r vm 外部后端装载(pac.at back.project)全量验证后,本 bin
// 退役,run_vm.ps1/sh 回归 `auto run -r vm` 薄封装。
//
// 架构(与 docs/plans/060-api-contract-unification.md §M3 一致):
//
//   .at GUI(auto-man run_vm_ui 编排,进程内)
//     └─ api.at 桩(shell.at 空桩)→ auto.host.call_value(name, args_json)
//         └─ auto-lang 宿主桥注册表(纯机制)
//             └─ ash_server::backend::assemble_host_bridge(本文件调用)
//                 └─ ash_server::worker(auto_shell::Shell → ash-core)
//                     └─ ShellEvent broadcast → 事件泵 → inject_shell_event
//
// 启动:在 ash-gui-auto 项目目录执行 `ash-runner`。

use ash_server::worker::ShellHandle;

fn main() {
    // ── 1. 进程内装配后端(worker + 事件泵 + 宿主桥注册,backend.rs)──
    let shell: ShellHandle = ash_server::backend::assemble_host_bridge();

    // 阻塞式 runtime:等 worker 初始化完成(boot 快照就绪;有界轮询)。
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("ash-runner: failed to build boot-check runtime");
    if let Err(e) = rt.block_on(shell.command_list()) {
        eprintln!("ash-runner: shell worker boot failed: {}", e);
        std::process::exit(1);
    }

    // ── 2. merged 环境 + 起 .at GUI(auto-man 编排)──
    std::env::remove_var("AUTO_BACKEND"); // 确保 merged(非 HTTP)分派
    let project_dir = std::env::current_dir().expect("ash-runner: no cwd");
    eprintln!("ash-runner: merged mode (in-process shell backend), project = {}", project_dir.display());
    if let Err(e) = auto_man::rust_ui::run_vm_ui(&project_dir, std::env::args().skip(1).collect()) {
        eprintln!("ash-runner: GUI exited with error: {:?}", e);
        std::process::exit(1);
    }
}
