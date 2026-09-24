---
plan_id: PLAN-083
status: executing
feature_name: AI 回合提速 + 工具 schema 兜底（thinking 控制 / 命令工具真 schema / 思考流可见 / 循环纠偏 / 错误分类）
author: [agent]
created_at: 2026-09-24T00:00:00+08:00
updated_at: 2026-09-24T16:00:00+08:00
plan_revision: 1
current_step: 7
total_steps: 7
supersedes_spec_components: []
new_spec_components:
  - designs/040-ai-turn-latency-tool-schema.md
  - docs/for-agents.md
touched_goals: []
---

# PLAN-083: AI 回合提速 + 工具 schema 兜底

## 0. 变更摘要

修复 2026-09-24 实测复现的 ash AI 模式两大问题：回合延迟 30s+（根因：
GLM-5.3-flash 经 zhipu anthropic 端点默认开启深度思考 + REPL 丢弃思考流事
件）与 du 工具调用失败（根因：82 个命令工具暴露给模型的参数 schema 为空，
模型无法带参调用，三次无参重复后被循环检测终止并收到误导性 API-key 报
错）。五项修复：thinking 显式控制、命令工具真参数 schema（由 Signature 生
成）、思考流可见、循环检测纠偏、错误提示分类；外加 daemon 路由健壮性（未
知 model 拒绝于路由层）。目标：复现场景首个工具调用 ≤10s 且带参。

## 1. 目标

1. **回合提速**：复现场景（"统计当前目录各子目录磁盘占用"，含 37 条历史 +
   82 工具的真实形态请求）从用户提交到首个工具调用 ≤10s（实测现状 42.6s）。
2. **工具一次成功率**：模型从工具 schema 即可得知命令参数（如 `du` 的
   path、flags），首次调用带参——不再出现连发无参 `du`/`cd` 的试错循环。
3. **过程可见**：模型思考期间 REPL/ask 显示思考流（灰显、可折叠计数），
   消除"静默 30s"体感；思考关闭时无此输出。
4. **循环纠偏**：达循环阈值时先向模型注入一次纠偏 tool_result（列出已重复
   的 `(tool,args)`、提示换参或直接作答），仅再犯才终止——替代当前的立
   即终止。
5. **错误分类**：回合错误不再一律附 API-key/daemon footer；仅初始化/连接
   类错误附该提示。
6. **daemon 健壮性**：非 `tier:` 且不在任何 provider 模型表的 model 字段在
   路由层返回 400 "unknown model"，不再透传上游报"模型不存在"。

### 非目标

- du 输出自身缺陷（total 重复计数等）——已并入 **PLAN-082 T-05**（随本
  计划同日修订，revision 2）。
- daemon `idle_timeout_min: 10` 空闲自杀 + 冷启动拉起体验、持久化历史无限
  增长（当前 15KB/6k token，压缩阈值 ~112k token）——影响小或另案处理，
  需要时再立项。
- GUI(ash-gui) 聊天面板的同步适配——CLI REPL 先行，GUI 事件流兼容（新增
  Thinking 渲染属增量）。
- auto-ai 的 thinking 档位语义（off/low/high/max 的 GLM 官方映射）调优——
  沿用 PLAN-064 既有契约，仅选择默认档。

## 2. 架构方案

问题出在三层，修复各自独立、可分别合入：

```
ash (auto-shell 仓)                      auto-ai (其仓 master, .at 源+retranspile)
┌─────────────────────────────┐          ┌──────────────────────────────────────┐
│ ChatSession::build / ask.rs │          │ auto-ai-agent/src/agent.at            │
│  agent.set_thinking_level_  │          │  循环检测: 阈值时注入纠偏 tool_result │
│    override("<档位>")       │          │  阈值+1 才 AgentError::LoopDetected   │
│ AshCommandTool::parameters()│          │ auto-ai-daemon/src/server.at          │
│  ← Signature.arguments 生成 │          │  未知 model → 路由层 400              │
│ repl.rs/ask.rs 事件回调     │          └──────────────────────────────────────┘
│  Thinking 事件渲染 + footer │           wire 契约(designs/040): Completion-
└─────────────────────────────┘           Request.thinking_level ∈ off|low|high|max
```

- **thinking 控制在 ash 侧落地**：`Agent::set_thinking_level_override` 为既有
  公开 API（auto-ai-agent，agent.at 转译产物 `rust/src/agent.rs:729`），
  ash 无需改 auto-ai 即可生效；daemon 侧 `thinking` → anthropic 体
  `{"type":"disabled"}` 映射已存在且实测有效（anthropic.rs:147-172）。
- **schema 在 ash 侧落地**：`register_ash_tools`（ai/mod.rs:281）已持有
  `Vec<Signature>`，只取了 name+description；`Signature.arguments`
  （cmd.rs:70，含 name/description/required/is_flag/is_option/short/default）
  足以生成 JSON-Schema properties + required。
- **auto-ai 仓改动走 .at 源 + retranspile.sh**（PLAN-080 单源化架构，
  `rust/` 目录是 a2r 生成物，禁手改）。
- 跨仓合入节奏沿用 PLAN-081/082 模式：auto-ai 先落其仓 master，ash junction
  跟进并在计划登记其 commit id。

## 3. 技术栈

- ash 侧：Rust（auto-shell 的 `ai/mod.rs`、`ash_command_tool.rs`、
  `ash/src/frontend/repl.rs`、`ai/ask.rs`）。
- auto-ai 侧：AutoLang `.at` 源（`auto-ai-agent/src/agent.at`、
  `auto-ai-daemon/src/server.at`）+ a2r retranspile + 其仓 `cargo test`。
- 验证：daemon 直连 curl 计时（复用 2026-09-24 的探针方法）、REPL/ask 手测、
  agent 单测（mock client 断言纠偏语义）。

## 4. 需求分析与背景调查

**授权**：用户 2026-09-24 会话——(a) 对 30s 延迟与 du 调用失败两个问题"请
分析为什么"→ 分析已交付；(b) 对修复组合（本计划 §0 五项 + daemon 健壮性）
回复"OK，起草"，并认可 du 修复并入 PLAN-082。期望值明确：**复现用例 10s 内
给出答案**。范围：auto-shell + auto-ai 两仓；无预算/延续限制约定。

**实测锚点（2026-09-24，本机，aaid 17654 / glm-5.3-flash / 29KB
真实形态 payload）**：

| 观察 | 数值 | 证据 |
|---|---|---|
| 原问题默认（思考开）端到端 | **42.6s**（1670 个 reasoning 事件后首见输出） | D:/tmp/du_stream.txt 复现记录 |
| 同请求 `thinking:"off"` | **4.3s**（直接产出 tool_call） | D:/tmp/du_off.txt |
| 极简请求（daemon+网络基线） | 1.6s | curl 探针 |
| `ash ask` trivial 全链路 | 5.0s | 实跑 |
| Assistant 角色 thinking_level | None（不传参→zhipu 端点默认开思考） | builtin_role_assistant.rs 无覆写；role_def.rs trait 默认 None |
| REPL/ask 对 Thinking 事件 | 直接丢弃 `Thinking {..} => {}` | repl.rs 事件回调、ask.rs:138 |
| 命令工具 schema | 82 个全为 trait 默认空 schema `{"type":"object","properties":{}}` | ash_command_tool.rs 未覆写 parameters()；仅 EvalAutoTool 有（:435） |
| du 复现回合模型行为 | 三次 `input:{}` 无参调用（含"指定目录"意图后仍无参） | 用户日志 + D:/tmp/du_off.txt 首响应同样 `input:{}` |
| 循环检测 | 阈值 3，按 `tool::args` 计数，达阈值执行前终止 | agent.at（rust 产物 agent.rs:76、607-625） |
| 错误 footer | 任何回合错误一律附 API-key 提示 | repl.rs:694-699（另有初始化处 :406） |
| daemon 未知 model | 非 tier: 且查无此模型 → 仍以该串为 model 发上游 | server.at（rust 产物 server.rs:285-300），探针 `model:"mid"` 实测触发"模型不存在" |
| aaid 生命周期 | `idle_timeout_min:10`；本次实例为 11:37:27 用户进 AI 模式时懒拉起 | ~/.config/autoos/ai-daemon.at + Get-Process |

**规范载体**：本仓无 `docs/specs/`（PLAN-081 判例）。跨仓 wire 契约
（thinking_level 值域、循环纠偏语义、工具 schema 生成规则）落
designs/040；ash 对 agent 的既有表述在 docs/for-agents.md。

## 5. 详细设计

### 5.1 命令工具真 schema（ash）

`AshCommandTool` 增加 `signature: Signature` 字段与构造参数；
`register_ash_tools` 传入完整 `Signature`。`parameters()` 由
`Signature.arguments` 生成：

- 位置参数（`is_flag=false && is_option=false`）→ properties `<name>`
  `{type:"string", description}`；`required=true` 者进 required 数组，并按
  声明顺序在 description 中标注"第 N 个位置参数"。
- flag（`is_flag=true`）→ property `<name>` `{type:"boolean"}`；description
  注明 `--name`（有 short 则 `(-s, --name)`）。
- option（`is_option=true`）→ property `<name>` `{type:"string"}`；注明
  `--name VALUE`；有 default 注明默认值。
- `description()` 拼接 Signature.description + `extra_help`（若有），模型
  可见用法不劣于 `--help` 人类输出。

`json_args_to_cli` 的既有编组规则不变：`{"args":[...]}` 顺序数组、对象按
值展开——schema 的 description 里给出推荐形态（位置参数用 `args` 数组按
序传）。评估_auto（EvalAutoTool）不动。

### 5.2 thinking 显式控制（ash）

- `ChatSession::build`（ai/mod.rs:440）与 `ask.rs` 两处，在 Agent 构造后调
  `agent.set_thinking_level_override(Some(<default>))`。
- `<default>` 由 T-02 实测定档（off vs low，≥3 个代表性 prompt 计时），初
  始倾向：**REPL 聊天/ask 默认 off**（工具调用回合不需要长思考，实测
  4.3s）；`ASH_AI_THINKING` 环境变量可覆盖为 low/high/max/off/inherit。
- 不改 auto-ai 角色 trait；daemon 映射已就绪。

### 5.3 思考流可见（ash）

- REPL：事件回调新增 `Thinking { text }` 分支——写入 TurnTailState 新
  LineKind::Thinking（灰显前缀如 `·· `），回合冻结时思考行折叠为一行计数
  （如 `· 思考 6.1k 字`），完整思考不回放（079 精神：工具噪音不回放）。
- ask.rs：直接 println 灰显单行前缀，回合结束折叠计数。
- 思考关闭时无任何新输出（零回归）。

### 5.4 循环纠偏（auto-ai-agent，.at 源）

agent.at 循环检测点改为两段：

- `count == threshold`：不执行工具，向 memory 注入
  `Message::tool_result(tc.id, hint)`，hint 文本含已重复的调用名、原始
  args、重复次数与指令（"换参数，或基于已有结果直接作答；再次完全相同调
  用将被终止"），本回合继续（模型获得一次纠偏机会）。
- `count == threshold + 1`（仍完全相同）：现行 `AgentError::LoopDetected`
  终止。
- 有界性：纠偏仅一次（按 key 记录已纠偏标记），总调用预算（max_turns）
  不变，不产生无限循环面。

### 5.5 错误分类（ash）

repl.rs:694 的回合错误 footer 改为条件式：错误串含连接/初始化特征
（`DaemonUnavailable`、`AI client init`、connection refused 等）才附
API-key/daemon 提示；`LoopDetected`、`upstream error`、`quota` 等仅打印本
体。初始化处（:406）不动。

### 5.6 daemon 路由健壮性（auto-ai-daemon，.at 源）

server.at 的非 tier: 分支：model 查无任何 provider 命中时，返回 400
`unknown model '<id>' (known: tier:max|pro|mid|lite|min, or a provider
model id)`，不再落到 default_provider 透传。

### 规范增量

| delta_id | add/modify/retire | docs/specs/... target | before/after rule | rationale | acceptance IDs |
|---|---|---|---|---|---|
| SD-01 | add | designs/040-ai-turn-latency-tool-schema.md | 无 → thinking_level 覆盖策略（默认档位/环境变量）、命令工具 schema 生成规则、循环纠偏两段语义、未知 model 路由 400 契约 | 跨 auto-shell/auto-ai 的 wire 与行为契约需要单一载体（PLAN-081 判例：无 docs/specs 体系，designs 承载） | AC-01..06 |
| SD-02 | modify | docs/for-agents.md | 未提及工具 schema/思考档位 → 增补 AI 工具参数形态与 ASH_AI_THINKING 说明 | agent 侧使用者（auto-ai-cli 等）需要知道 ash 工具的可传参形态 | AC-02、AC-07 |

## 6. 测试设计

- **auto-shell 单测**：`AshCommandTool::parameters()` 对含位置参数/flag/
  option/short/default 的样例 Signature 的生成快照；json_args_to_cli 既有
  用例全绿（编组规则未动）。
- **auto-ai-agent 单测**（.at 同步）：mock client 连发 3 次同参调用 →
  第 3 次收到纠偏 tool_result、第 4 次才 LoopDetected；不同 args 不触发。
- **auto-ai-daemon 单测**：`model:"mid"`（无 tier: 前缀且非模型 id）→
  400 unknown model；`model:"tier:mid"` 与具体模型 id 不受影响。
- **端到端计时**（复现脚本，验收载体）：真实形态请求（历史+82 工具）经
  daemon 计时，断言首 tool_call ≤10s 且 du 调用 input 含 path；REPL 手测
  思考流显示与错误分类。
- **回归**：`ash ask "只需回复ok"` ~5s 基线不劣化；GUI 事件流兼容冒烟。

## 7. 验收标准

- **AC-01** 复现场景提速：du 问题（含预载历史+82 工具形态）从提交到首个工
  具调用 ≤10s。验证：计时脚本 3 次取中位。
- **AC-02** 工具带参：模型对 `du` 类命令的首次调用 input 非空（含 path 或
  flags），且 schema 含该参数描述。验证：daemon/agent 事件流断言 + 单测。
- **AC-03** 思考可见：思考开启时 REPL/ask 显示思考流且回合结束折叠计数；
  思考关闭无新增输出。验证：手测 + LineKind 断言。
- **AC-04** 错误分类：LoopDetected/upstream/quota 错误无 API-key footer；
  连接/初始化错误保留。验证：注入式手测。
- **AC-05** 循环纠偏：达阈值注入一次含重复清单的纠偏 tool_result，再犯
  终止。验证：auto-ai-agent 单测。
- **AC-06** 路由拒绝：未知 model 得 400 "unknown model"，不再透传上游。
  验证：daemon 单测 + curl 探针。
- **AC-07** 回归与文档：ask/REPL 冒烟全绿；designs/040 与 docs/for-agents.md
  合入。

## 8. 执行步骤

- **T-01**（ash）[x] AshCommandTool 真 schema：Signature 字段 + parameters()/
  description() 生成 + register_ash_tools 传参 + 生成快照单测。
  验证：`cargo test -p auto-shell ash_command_tool` → 29/29（含
  json_args_to_cli 既有用例）。✅ 已完成 commit `907dc0b`。→ AC-02
- **T-02**（ash）[x] thinking 控制：build/clear/ask 三入口 override +
  ASH_AI_THINKING 环境变量；实测 off 5.88/2.20/6.22s vs low
  10.02/10.79/11.66s vs 默认 30-42s → **默认档 off**（结论记 designs/040
  §2/§3.1）。✅ commit `f9f4cdb`。→ AC-01
- **T-03**（ash）[x] 思考流可见：LineKind::Thinking + 折叠计数（REPL 尾部
  视口流式 `·· ` 行 + 冻结折叠 `· 思考 N 字`；ask 内联灰显 + 回合尾折叠）。
  验证：tail_chat 11 测试（+4）；ask 手测开/关两态（low 可见+折叠 341 字；
  off 零输出）。✅ commit `f9f4cdb`。→ AC-03
- **T-04**（ash）[x] 错误 footer 分类 wants_api_key_footer。
  验证：2 测试（连接/初始化 6 例附 footer；loop/quota/tool/config 5 例裸
  打印）。✅ commit `f9f4cdb`。→ AC-04
- **T-05**（auto-ai .at）[x] 循环纠偏两段语义 + 单测 + retranspile。
  验证：rust-ref 118 过/1 挂（roles 预存）、mvp_harness 25/25（+3）、
  a2r transpiled_harness 30/30（+2）；顺带修复 a2r 树计数不回传潜伏缺陷
  （bump_seen 以值传 Map）。✅ auto-ai commit `c789841`。→ AC-05
- **T-06**（auto-ai .at）[x] daemon 未知 model 400 + 单测 + retranspile。
  验证：`cargo test -p auto-ai-daemon` 73/73（+2）+ curl 探针（mid→400
  unknown model；tier:mid→200）。✅ auto-ai commit `79ff93a`。→ AC-06
- **T-07** [x] 端到端验收 + 回归 + 文档（designs/040 成稿、for-agents.md）。
  验证：落地后 auto-ai main 复测 7.33/4.95/5.13s 中位 **5.13s ≤10s**、du
  首调 input 均含 path；ask 回归 5.9s；dump_agent_payload.rs 为验收载体。
  ✅ commit `03550b3`。→ AC-01/02/07

依赖：T-01/T-02 独立先行（同仓）；T-03/T-04 随后；T-05/T-06 独立
（auto-ai 仓，可与 T-01..04 并行）；T-07 收口。跨仓节奏见 §10-Q3。
**执行顺序实录**：T-01→T-02→T-03/T-04（同仓同分支）→ T-05→T-06（auto-ai
worktree）→ 落 auto-ai main → T-07（junction 跟进后复测）。

## 9. 复审记录

- 2026-09-24 stage: new, plan_revision: 1 — 起草 handoff：背景为同日实测
  复现（42.6s/4.3s/空 schema/阈值 3/误导 footer，证据见 §4）；du 输出修
  复已随本起草并入 PLAN-082（其 revision 2）。outcome: **pass**（授权明
  确：五项修复 + daemon 健壮性 + 10s 目标）；next: **work**（T-01/T-05 可
  立即并行起步）。待定项仅 §10-Q1 默认档位（T-02 内闭环，不阻塞开工）。
- 2026-09-24 stage: work | plan_id: PLAN-083 | plan_revision: 1 |
  outcome: **pass** | code_commit: auto-shell worktree `plan-083-dev` @
  `03550b3`（base 28e8796，4 commits：28e8796 前置 WIP 路由 / 907dc0b
  T-01 / f9f4cdb T-02..04 / 03550b3 T-07）；auto-ai main @ `79ff93a`
  （c789841 T-05 / 79ff93a T-06，已落其仓 main，junction 跟进复测通过）|
  task_ids: T-01..T-07 全部 [x] | evidence:
  AC-01 e2e 中位 5.13s≤10s（3 次取中位，落地后复测）；AC-02 du 首调
  input 含 path+flags（dump_agent_payload + 探针）；AC-03 tail_chat 11 测
  + ask 手测开/关两态；AC-04 wants_api_key_footer 2 测（注入式 REPL 手测
  未做，以单测覆盖两态——记 §10-Q4）；AC-05 rust-ref/mvp_harness/a2r 三
  树单测；AC-06 daemon 73/73 + curl 400 探针；AC-07 ask 回归 5.9s、
  designs/040 与 for-agents.md 随 worktree 待 merge 合入。| blockers: 无
  （3 处预存测试失败与本计划无关，证据在 designs/040 §5；主检出遗留 WIP
  已路由 fix-lang-feature-defaults 并落地 main 28e8796）| next:
  **review**（auto-plan-review；worktree plan-083-dev 保留供复审与 merge）。
  附:执行中发现并修复 auto-ai a2r 树循环计数不回传潜伏缺陷（designs/040
  §3.3）；wire 字段名为 thinking_level（探针传 thinking 被静默忽略，已记
  契约）。

## 10. 待澄清事项

- **Q1 thinking 默认档**：已按倾向执行 **off**（实测：off 中位 5.88s /
  low 10.79s / 默认 30-42s；low 不达 10s 目标），`ASH_AI_THINKING` 可调、
  `inherit` 可回 provider 默认——结论记 designs/040 §2。owner: 用户；
  next: review 时认可（如需"简单问题保留轻思考"可改 low，属一行改动）。
- **Q2 循环纠偏语义**：已按默认纠偏方案执行（阈值 3 → 注入一次纠偏
  tool_result，4 → 终止），rust-ref/a2r/mvp_harness 三树测试锁定。若复审
  希望回到"达阈值立即终止"，T-05 回退面为 agent.at 两段块 + 3 个测试。
  owner: 用户；next: review。
- **Q3 跨仓合入**：已完成——auto-ai `c789841`/`79ff93a` 落其仓 main
  （retranspile 全量重转），ash junction 跟进后复测通过，commit id 已
  登记 designs/040 §7 与本计划 §8。owner: work 执行者；next: 已闭环。
- **Q4（新增）AC-03/AC-04 的 REPL 交互路径手测**：非交互环境无法驱动
  F3 进入 REPL AI 模式，两处以单测覆盖（LineKind 折叠/计数 4 测、footer
  分类 2 测）+ ask 路径真跑验证；REPL 尾部视口的思考流实机观感建议复审
  时人工过一遍。owner: 复审者；next: review。
