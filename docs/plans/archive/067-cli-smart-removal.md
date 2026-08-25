# 067 — CLI/TUI 模式减法:撤除 `ash smart` 子命令

- 日期:2026-08-25
- 状态:**已实施,验收通过**(plan-067 worktree 分支)
- 背景:Plan 066(用户裁定)—— 模式层面只保留"普通命令"与"AI 模式"两种,
  smart command 以「AI 模式内轻量 skill」形态择期回归。066 撤了 GUI 表面层,
  本计划把 CLI 侧同步撤净;ash-tui 本就没有 smart 入口(核查零引用),无需动。
- 实施:
  - 删 `ash/ash/src/main.rs` 的 `"smart"` 分派臂(唯一 CLI 入口)。
  - 删 `smart_command/cli.rs`(子命令处理器,纯表面层)及 mod.rs 注册。
  - mod.rs 文档改写:注明 066/067 模式减法与「skill 底件保留」定位。
  - **保留**:`smart_command` 的 config/loader/executor/nlu/role(底件,
    暂无调用方)+ plugin 的 smart 贡献目录集成(loader 侧,plugin_e2e 锁定)。
- 验收:`cargo build -p ash` 绿;smart_command 底件单测 45 passed;
  冒烟 —— `ash -c "echo …"` 正常,`ash smart list` → "ash: smart: No such
  file"(按普通命令/脚本路径处理,与 GUI 撤除语义一致)。
