---
plan_id: PLAN-079
status: execution_done
feature_name: AI 模式 UX——恢复对话回放 + 工具结果摘要去表头
author: [zhaop]
created_at: 2026-09-07T00:00:00+08:00
updated_at: 2026-09-07T00:00:00+08:00

supersedes_spec_components: []
new_spec_components: []
touched_goals: []

current_step: 0
total_steps: 5
---

# [PLAN-079] AI 模式 UX——恢复对话回放与工具结果摘要修正

## 变更摘要

用户实测(F3 AI 模式,Windows Terminal)反馈两处:

1. **恢复横幅空承诺**:进入 F3 打印"已恢复 N 轮对话"但不回放任何内容,
   用户无法看到恢复了什么(且历史文件 `~/.auto-shell-ai-chat.json` 现存
   13 轮,其中前数轮为 ash-server 时代 fake client 写入的测试数据
   `"propose release"`/`call-fake-propose`,进一步造成"恢复的不是我的
   对话"观感)。
2. **工具结果摘要取首行**:AI 调用 `ls` 后,`← ls: Name Type Size Modified`
   只打印了表格表头——`brief_result` 的"第一个非空行"策略对表格型输出
   失效(037 期实现,纯文本假设)。

修复:① 恢复时回放最近 5 轮文本对话(用户/助手文本块,工具块跳过),
更早轮次折叠一行提示;② 工具结果摘要多行时改为 `N 行` 计数,不再展示
首行;单行结果维持原文截断 80 字符。Thinking 事件维持不打印(用户给出
的三选一容忍集内的最小改动)。

## 目标

1. F3 恢复时可见最近对话内容(带折叠),横幅不再空承诺
2. AI 调表格类命令后不再打印表头行,多行结果显示 `N 行`
3. 既有测试基线不劣化,新逻辑带单测

## 架构方案

不新增模块。`brief.rs` 增 `brief_tool_result`(纯函数);`ai/mod.rs` 增
`extract_transcript`(从 `&[Message]` 提取文本对的纯函数,跳过
ToolUse/ToolResult 块)+ `ChatSession::transcript()` 薄封装;
`repl.rs` 恢复横幅后接回放打印(最近 5 轮,助手消息超 8 行截断)。
不改 auto-ai / auto-lang(历史文件与 `agent.history()` 已够用)。

## 技术栈

Rust;auto-ai-agent 的 `Message { role, content: Vec<ContentBlock> }`
(ai_config wire 类型,auto-shell 直依赖 ai_config)。

## 需求分析与背景调查

- 用户截图(2026-09-07 17:57-17:58,Windows Terminal):`* 已恢复 5 轮
  对话 *` 后无任何对话内容;AI 调 `ls` 仅见表头行。
- 代码事实:`repl.rs:419` 横幅后无回放;`brief.rs:38` 取第一非空行;
  历史文件 26 消息(13 轮),首轮为 fake 测试数据;`agent.history()`
  在 auto-shell 可访问(turn_count 已用),`ContentBlock::Text { text }`
  为文本块。
- 用户设计容忍集:全打印 / 全不打印 / 只留 `⚙️ ls` 去表头。本计划取
  "工具启动行保留 + 结果行按行数摘要"——信息量优于全删,视觉噪声小于
  全打印。

## 详细设计

1. **`auto-shell/src/ai/brief.rs`**:`brief_tool_result(result: &str) ->
   String`——非空行数 ≤1 时 `brief_truncate(首行, 80)`;>1 时
   `format!("{n} 行")`。原 `brief_result` 保留(其他调用点不动)。
2. **`ash/ash/src/frontend/repl.rs:553` 与 `ask.rs:129`**:Tool 事件臂
   改用 `brief_tool_result`。
3. **`auto-shell/src/ai/mod.rs`**:
   - `pub fn extract_transcript(messages: &[Message]) -> Vec<(bool,
     String)>`——(is_user, text);拼接每条消息的 Text 块,跳过
     ToolUse/ToolResult/空文本;纯函数可测。
   - `ChatSession::transcript()` 薄封装返回上述结果。
4. **`repl.rs:417-422`**:横幅后回放——取 transcript,按 user 消息切轮;
   回放最近 `RESTORE_REPLAY_ROUNDS = 5` 轮:用户行
   `  \x1b[2m? {截断120}\x1b[0m`,助手内容截断 8 行(超出加 `…`);更早
   轮次打印 `  \x1b[2m(更早 {n} 轮已折叠)\x1b[0m`。
5. **告知用户**:当前历史含 fake 测试轮次,回放会使其现形,可用 `/clear`
   清空重开。

## 测试设计

- `brief_tool_result`:单行/多行/空/全空行四例单测(brief.rs 测试模块)。
- `extract_transcript`:混合块(text/tool_use/tool_result)、纯工具块、
  空历史三例单测(ai/mod.rs 测试模块,JSON 构造 Message)。
- 回放折叠:轮数边界(≤5 轮全回放、>5 轮折叠)由提取函数的轮切分测试
  覆盖(若切分逻辑独立成纯函数则直接测)。
- 编译门:`cargo check --workspace --all-targets` 0 error;回归:
  套件红名单不变(spill flaky、`<obj#…>` 引擎侧)。
- 交互验收由用户执行:F3 恢复可见回放、`ls` 后只见 `⚙️ ls` + `← ls · N 行`。

## 验收标准

- C1:`brief_tool_result`/`extract_transcript` 单测全绿
- C2:ash `cargo check --workspace --all-targets` 0 error,套件红名单不变
- C3:用户交互确认两处 UX 均符合预期
- C4:DEBTS/NEXT 记录同步

## 执行步骤

- [x] T1 `brief.rs` 增 `brief_tool_result` + 4 例单测。验证:
      `cargo test -p auto-shell --lib brief` 全绿 ✅ 已完成(4/4)
- [x] T2 `repl.rs:553`/`ask.rs:129` Tool 臂换 `brief_tool_result`(空摘要
      跳过 ← 行)。验证:`cargo check -p ash -p auto-shell` 0 error
      ✅ 已完成
- [x] T3 `ai/mod.rs` 增 `extract_transcript` + `ChatSession::transcript()`
      + 3 例单测。验证:`cargo test -p auto-shell --lib transcript`
      ✅ 已完成(3/3)
- [x] T4 `repl.rs` 恢复回放(最近 5 轮 + 折叠)。验证:T3 测试 + 编译门
      ✅ 已完成
- [x] T5 全量:`cargo check --workspace --all-targets` 0 error +
      `cargo test --workspace` 红名单不变。验证:C1/C2 ✅ 已完成
      (--all-targets Finished 0 error;套件红仅 spill flaky +
      `<obj#…>` 引擎侧,与折回前基线一致)

## 复审记录

(待复审填写)

## 待澄清事项

(无——设计在用户给出的容忍集内)
