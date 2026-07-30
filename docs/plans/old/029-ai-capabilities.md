# Plan 029: ASH AI 能力增强 — TDD 计划

> **日期**: 2026-07-30
> **分支**: `main`（代码已合并，本计划为完成状态记录）
> **状态**: ✅ 全部完成（commit 58884af "029 fully complete"）
> **来源设计**: [`designs/029-ai-capabilities.md`](../../designs/029-ai-capabilities.md)（§0 重新评估版为准）
> **回归基线（2026-07-30）**: ash-core 384 + auto-shell 774 = **1158 全绿**

## Status: COMPLETE

---

## 0. 交付物总览

Plan 029 的 auto-shell 侧代码（~2700 行 + 43 测试）和 auto-ai 侧前置依赖（preferred_provider 链路 + OllamaProvider）均已合并 main。

### auto-ai 侧（前置依赖，已合并 main）

| 交付物 | 文件 | commit |
|---|---|---|
| preferred_provider 链路补全 | `wire.rs` / `agent.rs` / `server.rs` / `tier_router.rs` | d2facd6 |
| OllamaProvider | `auto-ai-daemon/src/provider/ollama.rs` | d2facd6 |

### auto-shell 侧（核心功能，全部已合并 main）

| 交付物 | 文件 | 行数 | 测试 | commit |
|---|---|---|---|---|
| AshCommandTool 桥 | `ash_command_tool.rs` | 505 | 7 | 197957d |
| SmartCommand 配置 | `smart_command/config.rs` | 341 | 13 | 21304c3 |
| SmartCommand 加载器 | `smart_command/loader.rs` | 199 | 6 | 21304c3 |
| SmartCommand 执行器 | `smart_command/executor.rs` | 132 | 5 | 21304c3 |
| SmartCommandRole | `smart_command/role.rs` | 162 | 7 | 21304c3 |
| SmartCommand NLU 路由 | `smart_command/nlu.rs` | 249 | 12 | 3f853b0 |
| SmartCommand CLI | `smart_command/cli.rs` | 189 | 3 | 21304c3 |
| F4 ChatSession → Agent | `frontend/ai.rs` | 709 | — | dd0b966 |
| F3 验证 + 多步预览 | `frontend/ai.rs` | — | — | dd0b966 |
| F3 NL→AutoLang | `frontend/ask.rs` | 141 | — | 51dbcf4 |
| Shell::eval_auto | `shell.rs` | — | — | 51dbcf4 |
| register_all（80 命令） | `ash_command_tool.rs` | — | — | 179eb31 |
| 上下文 builder | `frontend/ai_context.rs` | 80 | — | 3f197e5 |
| Shell 公开访问器 | `shell.rs` | — | — | 3f197e5 |
| Warp 式建议 | `frontend/suggest.rs` | — | — | 085edd3 |

### 已知小缺口（不阻塞归档）

| 缺口 | 说明 |
|---|---|
| `.at` 配置层 `preferred_provider` 字段 | config.rs 未加字段——代码内 Role 可设 pref，但用户 `.at` 文件无法配。低优先级（SmartCommand 主要是内置命令，用户自建场景少） |

---

## 1. 五个 AI 子能力 — 全部完成

| 子能力 | 状态 | 实现 |
|---|---|---|
| **SmartCommand** | ✅ | 配置/加载/执行/CLI + NLU 路由（`ash smart "<nl>"`） |
| **F4 tool-calling** | ✅ | Agent 后端 + AshCommandTool 桥 + StreamEvent 渲染 |
| **F3 NL→pipeline** | ✅ | 验证 + 多步预览 + Agent 后端 |
| **NL→AutoLang** | ✅ | `ash ask` + `eval_auto` + EvalAutoTool |
| **上下文感知** | ✅ | context builder + Shell 访问器 + Warp 建议 |

---

## 2. 架构决策记录

以下决策在设计文档 §0 中讨论，实施时确定：

| 决策 | 结论 |
|---|---|
| SmartCommandRole 落点 | 放 auto-shell（Role trait pub 导出，ash 可自定义领域 Role，不污染 auto-ai 共享库） |
| AshCommandTool 线程模型 | 专用 OS 线程 + channel（优于设计的 `Arc<Mutex>`，正确绕开 Shell 非 Send） |
| executor 不嵌入 AI 判断 | NLU 路由在 cli.rs 层完成（`nlu::route()` → SmartCommandRole → Agent），executor 保持纯确定性 body.ash 执行。职责清晰 |
| F3 用 Agent 而非手搓 CompletionRequest | 统一走 Agent 后端（`ask.rs` 用 `Agent::new(AutoLangCoder, client)`） |

---

## 3. 回归基线

- ash-core: **384** passed, 0 failed
- auto-shell: **774** passed, 0 failed
- **总计 1158 全绿**
- 029 相关内联测试: **43** passed（smart_command/* + ash_command_tool.rs）
