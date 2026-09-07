---
plan_id: PLAN-078
status: drafting
feature_name: 吸收 auto-ai API 漂移(ToolOutput 分离 + StreamEvent 回合臂)
author: [zhaop]
created_at: 2026-09-07T00:00:00+08:00
updated_at: 2026-09-07T00:00:00+08:00

supersedes_spec_components: []
new_spec_components: []
touched_goals: []

current_step: 0
total_steps: 6
---

# [PLAN-078] 吸收 auto-ai API 漂移——ToolOutput 分离与 StreamEvent 回合臂

## 变更摘要

auto-ai master 演进了两组契约:`Tool::execute` 返回值由 `String` 改为
`ToolOutput`(`content`/`details` 分离,PLAN-029);`StreamEvent` 新增
`TurnStart`/`TurnEnd` 回合边界事件。本仓 ash 工作区与 ash-server 未吸收,
实测编译失败(lib 4 错 / lib test 10 错;ash-server 依赖继承 + 自身
worker.rs 多处 StreamEvent 匹配预期浮出)。本计划做最小适配恢复全绿。
auto-lang(566 装箱)与此无关且已实测全绿,零触碰。

## 目标

1. `ash/` 工作区 `cargo check --workspace --all-targets` 0 error
2. `ash-gui/ash-server` `cargo check` 0 error
3. 测试基线不劣化(在册预存红不变),CLI 冒烟正常
4. DEBTS 偏斜条目结清注记

## 架构方案

不引入新模块、新依赖、新类型。三个 Tool 实现(`AshCommandTool`/
`ProposeTool`/`EvalAutoTool`)以 `ToolOutput::text(...)` 最小包装返回值;
`details` 本期一律不填(纯文本语义不变),UI/审批流需要结构化细节时另立
计划。`TurnStart`/`TurnEnd` 显式补臂、暂忽略(不渲染回合边界),为未来
每回合 usage 计量(`TurnEnd.usage`/`tool_count`)留扩展口。

## 技术栈

Rust;async-trait;`auto-ai-agent`(path dep `../../../auto-ai/crates/auto-ai-agent`,
契约权威定义在其 `rust-ref/src/tool.rs` 与 `rust-ref/src/agent.rs`)。

## 需求分析与背景调查

- DEBTS.md 2026-09-07"auto-lang 566 Value 装箱迁移与 auto-ai 偏斜"条在案:
  偏斜与装箱无关(控制实验实证),属 auto-shell 需吸收 auto-ai
  PLAN-029/064 变更的独立事项。
- 实测(2026-09-07,auto-lang master `d14d6400b`,auto-ai master 含
  PLAN-064 thinking 系列):ash-core ✅(18s);ash 工作区 lib 4 错 +
  `--all-targets` 10 错;ash-server 编译失败全部为继承(错误文件均在
  `ash/auto-shell/src`),但自身 `worker.rs:1129` 起多处 `StreamEvent`
  显式匹配在依赖修好后预期浮出新 E0004。
- 新契约(auto-ai 侧,以代码为准):
  - `tool.rs:21` `pub struct ToolOutput { pub content: String,
    pub details: Option<JsonValue> }`;构造器 `ToolOutput::text(impl
    Into<String>)`;`details` 永不进 LLM 上下文,仅随 `StreamEvent::Tool`
    附给 UI/审批流;形状约定 edit→`{diff,patch,first_changed_line}`、
    run_command→`{truncation,full_output_path}`、read→`{truncation}`。
  - `agent.rs:91` `TurnStart { turn: u32 }`(1-based,每回合首个
    Delta/Thinking/ToolStart 前发射);`TurnEnd { turn: u32,
    usage: Option<Usage>, tool_count: u32 }`(取消/错误中断时不发射)。

## 详细设计

1. **`ash/auto-shell/src/ai/ask.rs:119`**:`on_event` 闭包的 match 补
   `StreamEvent::TurnStart { .. } | StreamEvent::TurnEnd { .. } => {}`
   显式忽略臂(放最后;显式枚举而非 `_` 通配,保留未来新增臂的编译期提醒)。
2. **`ash/auto-shell/src/ash_command_tool.rs:169/233/419`**:三个
   `#[async_trait] execute` 的成功返回值 `String` → `ToolOutput::text(...)`
   包装;`ToolError` 错误臂不变。函数体内构造 `String` 的中间逻辑不动,
   只在返回点包装。
3. **同文件 `#[cfg(test)]` 测试(~604/632/666/677)**:原按 `String`
   消费 execute 结果的断言改为 `.content` 字段访问;`Display`/`contains`/
   `trim` 族调用改到 `content` 上;两处 E0308 按新类型修正。
4. **`ash-gui/ash-server/src/worker.rs`(1129 起各 StreamEvent match)**:
   依赖修好后复测,浮出的 E0004 逐一补 `TurnStart`/`TurnEnd` 忽略臂;
   若某 match 已有 `_` 通配则无需动。
5. **DEBTS.md**:偏斜条目追加结清注记(吸收完成,指向 PLAN-078)。

## 测试设计

- 编译门:`cd ash && cargo check --workspace --all-targets` 0 error;
  `cd ash-gui/ash-server && cargo check` 0 error。
- 回归:`cd ash && cargo test --workspace`——基线 auto-shell 702 过 2 挂
  (在册预存)、ash 130,红名单不变即过;ash-core 不在本计划触碰面。
- 冒烟:`ash -c "echo hi"` 正常;`??` 对话一轮正常(ask.rs 改动面)。
- 不新增单测:补臂为纯忽略语义、返回值为机械包装,无新逻辑可测;
  既有测试改断言已覆盖类型迁移。

## 验收标准

- C1:ash 工作区 `cargo check --workspace --all-targets` 0 error
- C2:ash-server `cargo check` 0 error
- C3:`cargo test` 基线不劣化(预存红名单不变)
- C4:`ash -c` 冒烟正常
- C5:DEBTS 偏斜条目结清;NEXT.md 已递增(078→079)同提交入库

## 执行步骤

- [ ] T1 `ash/auto-shell/src/ai/ask.rs:119` match 补
      `TurnStart { .. } | TurnEnd { .. } => {}` 忽略臂。验证:
      `cd ash && cargo check -p auto-shell 2>&1 | grep -c "E0004"` → 0
- [ ] T2 `ash/auto-shell/src/ash_command_tool.rs:169/233/419` 三个
      execute 成功返回 `ToolOutput::text(...)` 包装。验证:同上
      `grep -c "E0053"` → 0
- [ ] T3 同文件 `#[cfg(test)]` 段(~604/632/666/677)String 消费点改
      `.content`;Display/contains/trim 移到 content;E0308 两处修类型。
      验证:`cd ash && cargo check --workspace --all-targets` → 0 error
- [ ] T4 `cd ash-gui/ash-server && cargo check`,对 `src/worker.rs` 浮出的
      E0004 逐一补臂。验证:该命令 → 0 error
- [ ] T5 回归 + 冒烟:`cd ash && cargo test --workspace`(基线对照)、
      `ash -c "echo hi"`。验证:C3/C4 达成
- [ ] T6 DEBTS.md 偏斜条目结清注记。验证:git diff 仅注记行

## 复审记录

(待 /auto-plan:review 填写)

## 待澄清事项

(无——需求与契约均已实测确认)
