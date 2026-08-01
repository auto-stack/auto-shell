# Plan 037: ASH CLI 架构收尾 — Block UX + crate 拆分 + Command trait 解耦

> **日期**: 2026-08-01
> **分支**: 待建（`feat/037-cli-architecture-cleanup`）
> **状态**: 待实施
> **来源**: 从归档的 Plan 013（Phase 4）、014（A4/A5 + §II）、020（5.1/A.2）收拢的残留架构优化项
> **预估**: 3 个里程碑，~2000 行，2-3 周

---

## 0. 背景与动机

Plan 013/014/017/020 是 2026-06 的早期路线图。经 2026-08-01 对照代码逐项核实，**功能性工作 100% 完成**（I/O 重定向、管道、glob、别名、函数、补全、AI 集成、插件系统、Atom 管道等），这 4 个计划已归档。

本计划收拢它们残留的**架构优化项**——这些不是功能缺口，而是代码组织的债：TUI 代码未拆成独立 crate、Command trait 与 Shell 紧耦合、CLI 仍是线性 REPL（无 Block UX）。这些项在功能层面已被 `frontend-tui` feature flag 实质性绕过（ash-gui 已能消费纯逻辑），但完整的架构清理仍有长期价值。

### 与现有计划的关系

| 残留项 | 来源 | 现状 |
|---|---|---|
| CLI Block UX（sticky 头/alternate-screen） | 013 Phase 4 / 020 附录 A.2 | CLI 是 reedline 线性 REPL；GUI（030）已有 Block 模型 |
| ash-tui crate 拆分 | 014 A4 / 020 5.1 | TUI 在 `auto-shell/src/frontend/`；`frontend-tui` feature 已隔离依赖 |
| Command trait 解耦 | 014 §II 耦合点 #1 | `Command::run(&mut Shell)` 紧耦合，未抽 ShellContext trait |

---

## M1：Command trait 解耦（~500 行）

**目标**：`Command::run` 不再直接接 `&mut Shell`，改为接 `&mut dyn ShellContext` trait，降低命令与 Shell 实现的耦合。

**任务**：
- 在 ash-core 或 auto-shell 定义 `ShellContext` trait（暴露命令所需的最小接口：`pwd`/`vars`/`env`/`registry`/`execute`/`policy` 等）
- `Shell` 实现 `ShellContext`
- `Command::run(args, input, &mut dyn ShellContext)` 替换 `&mut Shell`
- 79 个命令迁移（机械替换，大部分只改签名）

**验收**：全量回归全绿（876+408 不变），命令行为零变化。

**风险**：改动面广（79 个命令签名），但机械性强。建议分批迁移（先核心命令，再批量）。

---

## M2：ash-tui crate 拆分（~800 行）

**目标**：把 `auto-shell/src/frontend/` 的终端依赖代码（reedline/crossterm/ratatui/nu-ansi-term）拆到独立的 `ash-tui` crate，`auto-shell` 变成纯 Shell 逻辑 + 命令的薄层。

**任务**：
- 新建 `ash-tui` crate（workspace 成员）
- 迁移 `frontend/{repl,term,completions_reedline,renderer,menu,prompt/engine+module+modules}` 到 ash-tui
- `auto-shell` 保留：shell 逻辑、命令、completions 定义、plugin、smart_command
- `ash` binary 依赖 `auto-shell` + `ash-tui`；`ash-gui` 只依赖 `auto-shell`（已是现状）
- 移除 `frontend-tui` feature flag（不再需要——依赖隔离由 crate 边界天然保证）

**验收**：
- `cargo build -p auto-shell` 无终端依赖（当前 `--no-default-features` 的效果由 crate 边界达成）
- CLI 行为零变化（回归全绿）
- ash-gui 仍正常构建

**风险**：模块迁移涉及大量 `use` 路径调整。`frontend-tui` feature 的 cfg-gate 逻辑要清理。M1 先行（Command 解耦后，ash-tui 对 Shell 的依赖更干净）。

---

## M3：CLI Block UX（可选，~700 行）

**目标**：CLI 的 reedline REPL 支持 Block 式输出（sticky 命令头、状态着色），对齐 GUI 的 Block 模型。

**任务**：
- reedline 的 `Prompt` 渲染增强：每条命令的输出带 sticky 头（命令 + 退出码 + 耗时）
- alternate-screen Block 滚动（可选，依赖 reedline 能力）
- 与 ash-gui 的 `Block` 模型（030 M3）共享数据结构

**验收**：命令输出带 sticky 头 + 状态色；视觉上更接近 Warp。

**风险**：reedline 的渲染钩子有限，alternate-screen 可能需要自定义。如果 reedline 限制太大，降级为"命令头着色 + 不滚动"。本里程碑**可选**——如果 M1/M2 后评估 reedline 限制过大，可降级或跳过。

---

## 依赖与顺序

```
M1（Command trait 解耦）→ M2（ash-tui crate 拆分）→ M3（CLI Block UX，可选）
```

M1 是 M2 的前置（解耦后 frontend 对 Shell 的依赖更干净）。M3 依赖 M2（ash-tui 独立后才好在 TUI 层做 Block 渲染）。

## 非目标（明确不做）
- ❌ 原生 HTTP 客户端（仍用 curl）—— 独立计划，非架构项
- ❌ LLMProvider trait 抽象 —— AI 已用 auto-ai-agent，设计过时
- ❌ man pages / 独立文档系统 —— 文档项，非架构项

## 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| M1 迁移 79 命令引入回归 | 中 | 高 | 分批迁移 + 每批回归；机械替换为主 |
| M2 crate 拆分路径调整量大 | 中 | 中 | 先建 crate 骨架验证编译，再逐步迁移 |
| M3 reedline 限制 Block UX | 中 | 低 | M3 标注可选，降级方案明确 |

## 成功指标
1. M1：Command::run 接 `&mut dyn ShellContext`，回归全绿
2. M2：ash-tui 独立 crate，auto-shell 无终端依赖，移除 frontend-tui feature
3. M3（若做）：CLI 命令输出带 sticky 头 + 状态色
