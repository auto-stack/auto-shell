# Plan 029: ASH AI 能力增强设计

> **日期**: 2026-07-20(初稿 SmartCommand)/ 2026-07-21(扩展为 AI 能力总设计)
> **状态**: ⚠️ **v1 设计已过时（见 §0 重新评估）** —— 2026-07-30 基于 auto-ai 最新 daemon+client+app 架构重新评估，发现多处设计在 auto-shell 自实现本属 harness 的逻辑。下方 §1-§8 为原始 v1 设计（保留作历史），**实施以 §0 为准**。
> **战略驱动**: 让 ash 成为 AI 时代的结构化 shell —— 内置完整的 AI 能力(SmartCommand + F4 tool-calling + F3 NL→pipeline + NL→AutoLang + 上下文感知),本地小模型 + 云端大模型分层协同
> **范围**: 跨 auto-shell + auto-ai 仓库,统一设计 ash 的所有 AI 子能力
> **预估**: 含 5 个子能力,共享基础设施后总工作量约 8-12 周(详见 §8)

---

## §0 基于最新 auto-ai 架构的重新评估（2026-07-30）

> **⚠️ 实施以本节为准。** 下方 §1-§8 是 2026-07-21 的 v1 设计，写于 auto-ai 架构定型之前，多处把本属 harness 的逻辑设计在 auto-shell 内自实现。本节基于 auto-ai 当前的 daemon+client+app 架构重新评估，修订实施计划。

### 0.1 触发原因

v1 设计（§1-§8）写于 2026-07-21。此后 auto-ai 确立了清晰的**三层架构**（daemon 唯一 LLM 网关 + client 薄客户端 + agent 是 app 借助的 harness 库），核心理念变为「**harness 功能尽量放 auto-ai，各 app 借助 auto-ai-agent 库调用**」。

对照该理念重新审查 v1，发现三处明确在 auto-shell 自实现了本属 harness 的逻辑：
- §A.6 的 `ai.generate` native 直接手搓 `CompletionRequest` + `block_on` 直连 provider
- §6.3 的 NL→AutoLang 手写 mini-ReAct（max_retries + 错误回喂循环），违背 v1 自己「不重写 ReAct」的决策
- §4 的 F4 ChatSession 手搓 Agent 构造 + 历史回放

### 0.2 auto-ai 现状（事实基础，均经一手核实）

#### 三层架构

| 层 | crate | 职责 | 对 029 的意义 |
|---|---|---|---|
| **daemon** (`aaid`) | `auto-ai-daemon` | 唯一 LLM 网关：所有 provider 知识、tier 路由、并发池、usage tracking | provider/路由必须在此 |
| **client** | `auto-ai-client` | 薄 HTTP 客户端：发 canonical 请求、收 canonical 响应，无 provider 知识 | auto-shell **目前只到这层** |
| **agent** | `auto-ai-agent` | **app 借助的 harness**：Role/Agent/Tool/ReAct/Workflow，用 client 驱动 | 029 的大多数 AI 功能该用这层 |

#### 关键事实（file:line 基于 `D:\autostack\auto-ai`）

1. **`Agent::run` + `run_stream` 已完整实现**（`auto-ai-agent/src/agent.rs:286/521`）—— ReAct 循环、tool 执行、循环检测（LOOP_DETECT_THRESHOLD=3）、软/硬 turn 上限、流式、取消，全部就绪。**029 无需自写任何工具循环。**
2. **`Tool` trait + `ToolRegistry` 已存在**（`auto-ai-agent/src/tool.rs:21/49`）—— ash 只需把命令实现成 `Tool` 注册即可。
3. **已有 14 个内置 Role**（`auto-ai-agent/src/builtin_roles/mod.rs:50`），含专为 Ash 设计的：
   - `Translator`（tier=Pro/云端，纯翻译不执行，max_turns=3）—— NL→命令
   - `Runner`（tier=Mid/云端，带 tool 执行，max_turns=15）—— 执行
4. **`Agent::with_context` / `with_context_file` 已存在**（`agent.rs:176`）—— 上下文注入基础设施就绪。
5. **流式输出全链路打通**：client `complete_stream` → daemon SSE（`server.rs:256`）→ provider → Agent `run_stream`。
6. **`preferred_provider()` 方法已在 Role trait 中存在**（`role_def.rs:116`），`TierRouter::resolve(tier, pref)` 也能接收（`tier_router.rs:113`）——**但链路有 3 处断点**（见 0.3）。
7. **auto-shell 现状**：只依赖 `auto-ai-client`（`ash/auto-shell/Cargo.toml:19`），**不依赖 `auto-ai-agent`**，手工构造 `CompletionRequest`（`frontend/ai.rs:157`、`frontend/repl.rs:376`）——停留在 Layer 2（client），未用 agent 的 ReAct/Tool/Role 体系。

### 0.3 功能归属重新划分（核心）

#### 🟢 auto-ai 已有，auto-shell 直接用（v1 当成「待实现」，其实已完成）

| v1 设计的功能 | auto-ai 现状 | auto-shell 要做的 |
|---|---|---|
| F4 tool-calling 的 ReAct 循环 | `Agent::run`/`run_stream` 已实现（`agent.rs:286/521`） | 仅构造 Agent + 注册 Tool |
| Tool 系统 | `Tool` trait + `ToolRegistry`（`tool.rs:21/49`） | 实现 ash 命令为 Tool |
| 流式输出 | 全链路打通 | 渲染 `StreamEvent` |
| 上下文注入基础设施 | `Agent::with_context`（`agent.rs:176`） | 喂入 shell 特定 context chunk |
| NL→命令 的 Role | `Translator` 已存在，专为 Ash 设计 | 可能直接用，或继承它 |
| 执行型 Role | `Runner` 已存在，专为 Ash 设计 | SmartCommand 的执行可能用它 |

#### 🔵 该推进到 auto-ai（v1 设计在 auto-shell，应改归 auto-ai；多数 v1 已放对）

| 功能 | v1 位置 | 问题 / 现状 | 应归属 |
|---|---|---|---|
| **`ai.generate` native（§A.6）** | auto-shell 手搓 `CompletionRequest`+`block_on` | **最典型越界**：auto-shell 自己拼 LLM 请求 | auto-ai 提供 high-level API / 走 Agent |
| **NL→AutoLang 反馈循环（§6.3）** | auto-shell 手写 mini-Agent | 违背「不重写 ReAct」决策 | 复用 `Agent::run` |
| **F4 ChatSession 历史回放（§4）** | auto-shell 手搓 replay | 会话编排是 harness 抽象 | auto-ai 提供「带历史的 ChatSession」 |
| **OllamaProvider（§2.1）** | ✅ v1 已正确放 auto-ai-daemon | 但尚不存在；可用 `kind="openai"` 临时跑（无 auth 已支持） | auto-ai（保持，需新建） |
| **preferred_provider 链路（§2.1）** | ✅ v1 已正确放 auto-ai | **3 处断点**需补全 | auto-ai（保持，需补全） |
| **SmartCommandRole（§2.1 #8）** | ✅ v1 已正确放 auto-ai-agent | 尚不存在；但已有 Translator/Runner 需重新评估关系 | auto-ai（保持，需新建，见 0.7） |

**preferred_provider 链路的 3 处断点**（必须改 auto-ai）：
1. `Role::preferred_provider()` 方法已有（`role_def.rs:116`）✓
2. `TierRouter::resolve(tier, pref)` 能接收 pref（`tier_router.rs:113`）✓
3. ✗ **`Agent::build_request`（`agent.rs:536`）不读 `role.preferred_provider()`**
4. ✗ **`CompletionRequest`（`wire.rs:156`）无 `preferred_provider` 字段**
5. ✗ **daemon `server.rs` 用 `candidates(tier)` 而非 `resolve(tier, pref)`**（`server.rs:129`）

#### 🟡 留在 auto-shell 合理（领域特定，非 harness）

| 功能 | 为什么留 auto-shell |
|---|---|
| **AshCommandTool 桥（§2.2）** | 需访问 `Shell` 私有类型，桥是适配层。参考 `auto-ai-cli/src/tools.rs` 的 `RunCommand` |
| **SmartCommand 的 command.at 加载/executor/body.ash（§3）** | ash 特有领域模型 + AutoLang 执行 |
| **F3 NL→pipeline 的验证/多步预览（§5）** | shell 侧逻辑 |
| **`Shell::eval_auto`（§6 的 pub 包装本身）** | 包装私有方法，合理 |
| **Shell 访问器 + Warp 式建议（§7）** | REPL 体验层 |

#### ⚪ 边界灰区（可工作，理想形态是 auto-ai 提供辅助 API）

| 功能 | 现状 | 理想形态 |
|---|---|---|
| **context builder 的 system prompt 拼装（§2.3）** | auto-shell 拼 prompt | auto-ai 提供「prompt chunk 注入」API，auto-shell 只填数据 |
| **`register_all_ash_commands` + schema 推导（§2.2）** | auto-shell 推导 | auto-ai 提供「签名→Tool schema」辅助（但 `Command` trait 是 ash 私有，当前放 auto-shell 可接受） |

### 0.4 三个被推翻的设计决策

| v1 决策 | 问题 | 修订 |
|---|---|---|
| ❌ **§A.6 `ai.generate` 手搓 `CompletionRequest`** | auto-shell 直连 provider，自实现 LLM 调用 | 走 auto-ai high-level API（`Agent` 或专用 generate 方法） |
| ❌ **§6.3 NL→AutoLang 手写反馈循环** | 自写 max_retries + 错误回喂，是 mini-ReAct | 复用 `Agent::run`（符合 v1 自己的「不重写 ReAct」决策） |
| ❌ **§4 F4 ChatSession 手搓历史回放** | 每个 app 各自手搓 replay，会话编排应共享 | auto-ai 提供「带历史的 ChatSession」抽象 |

### 0.5 修订后的实施计划（明确拆分两侧）

#### 📦 auto-ai 侧（跨仓库 PR，auto-shell 的前置依赖）

| 工作 | 改动点 | 说明 |
|---|---|---|
| **preferred_provider 链路补全** | `wire.rs` 加字段 + `agent.rs:536` 读 role + `server.rs:129` 改用 `resolve` | 3 处断点，纯加法+默认 None，零破坏 |
| **OllamaProvider** | `auto-ai-daemon/src/provider/ollama.rs` 新建 + `mod.rs` 加 `"ollama"` 分支 | 薄包装委托 OpenAiProvider（Ollama 暴露 OpenAI 兼容 API） |
| **SmartCommandRole** | `auto-ai-agent/src/builtin_roles/smart_command.rs` 新建 + 注册 | tier=Min / preferred_provider="ollama" / max_turns=3（见 0.7） |
| **（可选）ChatSession 抽象** | `auto-ai-agent` 新增带历史回放的会话类型 | 解耦 §4 的手搓 replay |
| **（可选）high-level generate API** | `auto-ai-agent` 或 `auto-ai-client` 暴露非 Agent 的单次 generate | 替代 §A.6 的手搓 |

#### 📦 auto-shell 侧（依赖 auto-ai 侧完成）

| 工作 | 改动点 | 说明 |
|---|---|---|
| **新增 `auto-ai-agent` 依赖** | `ash/auto-shell/Cargo.toml` | 从 Layer 2（client）升到 Layer 3（agent） |
| **AshCommandTool 桥** | `ash/auto-shell/src/ai_bridge.rs` 新建 | 参考 `auto-ai-cli/src/tools.rs:127` 的 `RunCommand`，但调 `Shell::execute_for_agent` |
| **SmartCommand 领域逻辑** | `smart_command/{config,loader,executor}.rs` + CLI | command.at 加载 / body.ash 执行 / `ash smart` |
| **前端集成** | `frontend/ai.rs` ChatSession 改造 + F3 预览 | 用 Agent + 渲染 StreamEvent |

#### 依赖关系

```
auto-ai 侧（跨仓库 PR）
  preferred_provider 链路 ──┐
  OllamaProvider ───────────┼─→ auto-shell 侧可启动
  SmartCommandRole ─────────┘
  (可选) ChatSession 抽象 ──→ 解锁 F4
  (可选) generate API ──────→ 解锁 NL→AutoLang

auto-shell 侧
  新增 auto-ai-agent 依赖 ──→ 一切的前提
  AshCommandTool 桥 ────────→ F4 / SmartCommand 共用
  SmartCommand 领域逻辑 ────→ 依赖桥 + SmartCommandRole
  前端集成 ─────────────────→ 依赖 Agent + 桥
```

两侧可部分并行：auto-shell 的 AshCommandTool 桥、SmartCommand 领域逻辑不依赖 auto-ai 侧的 provider/链路（只要能编译），但端到端验证需要 auto-ai 侧完成。

### 0.6 工作量修订

| 侧 | 范围 | 估算 | vs v1 |
|---|---|---|---|
| **auto-ai 侧** | preferred_provider 链路 + OllamaProvider + SmartCommandRole +（可选）抽象 | 3-4 周 | v1 未单列（散在各 milestone 里） |
| **auto-shell 侧** | Tool 桥 + SmartCommand 领域 + 前端集成 | 6-8 周 | v1 估 12-16 周，**因不再自实现 harness 而减半** |

**为什么 auto-shell 侧大幅下降**：v1 的 M2/M3（F4 tool-calling、NL→AutoLang）原需在 auto-shell 自实现 ReAct 循环、历史回放、反馈循环——这些现在由 `auto-ai-agent` 提供。

### 0.7 需后续决策：SmartCommandRole vs Translator/Runner

auto-ai 已有三个相关 Role，定位不同：

| Role | tier | provider | tools | 定位 |
|---|---|---|---|---|
| `Translator`（已有） | Pro | 云端 | 无 | 纯翻译：NL→命令字符串，**不执行** |
| `Runner`（已有） | Mid | 云端 | 有 | 执行：拿精确指令，用 tool 执行并报告 |
| `SmartCommandRole`（v1 设计） | Min | **Ollama 本地** | 有 | 智能命令：几步确定性 + 一步 AI 判断 |

**结论：SmartCommandRole 仍需新建**——它的核心差异是**本地小模型（tier=Min + preferred_provider="ollama"）**，用于低延迟、零成本的参数解析/NLU 判断，与 Translator（云端、纯翻译）、Runner（云端、执行）定位不重叠。但实施时需明确三者分工边界：
- SmartCommand 的 **body.ash 确定性步骤** → ash 侧 AutoLang 执行（不经 LLM）
- SmartCommand 的 **AI 判断步骤** → SmartCommandRole（本地 Ollama）
- 用户的 **NL 兜底请求**（无对应 SmartCommand）→ Translator（云端）或 F3

---



> **ash 不只是"能被 AI Agent 调用"的 shell(那是 Plan 028 做的),更是"内置 AI 能力"的 shell**。本设计统一规划 ash 的所有 AI 子能力,让本地小模型和云端大模型各司其职,覆盖从"自然语言到结构化命令"到"上下文感知的对话式助手"的完整光谱。

### 五个 AI 子能力(本设计的范围)

| 子能力 | 一句话定位 | 当前状态 | 模型层 |
|---|---|---|---|
| **SmartCommand** | "几步确定性操作 + 一步 AI 判断"封装成一条命令(如 `git.finish-worktree`) | 设计完成,未实施 | 本地小模型(Ollama)为主 |
| **F4 tool-calling** | F4 chat 从纯对话升级为能调命令的 Agent(看到 tool_calls,执行,回填结果) | 完全 greenfield(`tools: Vec::new()` 硬编码) | 云端大模型(tier:mid/pro) |
| **F3 NL→pipeline 增强** | F3 从"返回一条命令"升级到"pipeline + 多步预览 + AutoLang 检测" | 地基已有(prompt 已提 pipeline),缺验证/预览 | 云端大模型 |
| **NL→AutoLang 脚本** | 自然语言生成多步 AutoLang 脚本(带 `fn`/`while`/`try-catch`),比 F3 单命令强 | VM 可行(探勘确认),缺公开 API + 反馈循环 | 云端大模型 |
| **上下文感知** | AI 知道 cwd / 最近命令 / exit code / aliases / 环境(对标 Warp) | 极薄(只注入 cwd),Shell 内部状态大多私有 | 所有层共享 |

### 五者的依赖关系

```
                ┌─────────────────────────────────────┐
                │  共享基础设施(§2,所有子能力的地基)   │
                │  ┌──────────────────────────────┐    │
                │  │ OllamaProvider               │    │ ← §2.1(原 029 §1,保留)
                │  │ + preferred_provider 链路    │    │
                │  └──────────────────────────────┘    │
                │  ┌──────────────────────────────┐    │
                │  │ ash → auto-ai-agent 桥       │    │ ← §2.2(新)
                │  │ (80 命令 → auto_ai_agent::   │    │
                │  │  tool::Tool)                 │    │
                │  └──────────────────────────────┘    │
                │  ┌──────────────────────────────┐    │
                │  │ 上下文 builder               │    │ ← §2.3(新)
                │  │ (cwd/cmd/exit/aliases →      │    │
                │  │  system prompt)              │    │
                │  └──────────────────────────────┘    │
                └─────────────────────────────────────┘
                              ↑
            ┌─────────────────┼─────────────────┐
            │                 │                 │
   ┌────────┴──────┐  ┌───────┴───────┐  ┌──────┴────────┐
   │ SmartCommand  │  │ F4 tool-calling│  │ F3 NL→pipeline│
   │ (本地小模型)  │  │ (云端大模型)   │  │ (云端大模型)  │
   │ §3(详细)     │  │ §4(重概要)   │  │ §5(重概要)   │
   └───────────────┘  └───────────────┘  └───────────────┘
            │                 │                 │
            └─────────────────┼─────────────────┘
                              ↓
                    ┌─────────────────┐
                    │ NL→AutoLang 脚本│  ← §6(重概要)
                    │ 依赖 F3/F4 的    │
                    │ LLM 调用基础设施 │
                    └────────┬────────┘
                             │
                    ┌────────┴────────┐
                    │ 上下文感知       │  ← §7(重概要)
                    │ 所有子能力共享   │
                    └─────────────────┘
```

**关键设计原则**:**共享基础设施先建**(§2),各子能力(§3-§7)在其之上特化。这避免了"每个子能力各自实现 LLM 调用/Tool 注册/上下文构建"的重复。

### 范围内 / 范围外

| 范畴 | 本 Plan 包含 | 不包含 |
|---|---|---|
| **auto-ai 改造** | OllamaProvider + preferred_provider 链路补全 + SmartCommandRole | 新 provider 协议、daemon 架构调整 |
| **Tool 桥** | ash 命令 → `auto_ai_agent::tool::Tool` 桥 | 重写 auto-ai-agent 的 ReAct 循环 |
| **SmartCommand** | `command.at` 格式 + 加载器 + body.ash + `ash smart` CLI + git.finish-worktree 实例 | SmartCommand 编辑器/REPL 集成 |
| **F4 tool-calling** | ChatSession 用 Agent::run + Tool 注册 + 流式渲染 | 自定义 Agent UI(留给 030 ash-gui) |
| **F3 增强** | 验证 + 多步预览 + AutoLang 检测 | F3 改为 chat 模式(保持 one-shot) |
| **NL→AutoLang** | `Shell::eval_auto` 公开方法 + 生成-执行-反馈循环 | 完整 AutoLang IDE(留给 AutoCoder) |
| **上下文** | 公开 Shell 访问器 + context builder | **输出留存(v1 不做,留后续 Plan)** |
| **skill.md 转译** | sidecar `.md` 直接用 | AutoDown→Markdown(探勘证实不存在) |

### 四条核心架构决策(已在 brainstorming 阶段确认)

1. **Tool trait = `auto_ai_agent::tool::Tool`** —— ash 依赖 auto-ai-agent,**不**在 ash-core 新建 Tool trait(修正原 029 的错误引用)。SmartCommand 和 F4 tool-calling 都注册成 `auto_ai_agent::tool::Tool`。
2. **v1 不做输出留存** —— Shell 不保留上一次输出。AI 上下文只含 cwd/最近命令/exit code/aliases/环境变量。输出留存是独立后续 Plan。
3. **F4 tool-calling 用 `Agent::run`** —— 不重写 ReAct 循环,直接用 auto-ai-agent 现成的(已完整实现 + 测试)。
4. **provider 路径 = A** —— 先补全 auto-ai 的 `preferred_provider` 链路,SmartCommandRole 用 `preferred_provider = "ollama"`(本地小模型),F4 用云端(tier:mid/pro)。

---

## 第 1 节:子能力总览(给阶段 2 横向检查用)

> 本节是**给后续横向一致性检查用的快速索引**——每个子能力一表行,标注它跟其他方向(028/030/方向 B 等)的接触点。

| 子能力 | 主要消费者 | 依赖的基础设施 | 跟其他方向的接触点 |
|---|---|---|---|
| **SmartCommand** | 用户(CLI)/ AI Agent(外部) | OllamaProvider + preferred_provider + `auto_ai_agent::tool::Tool` + body.ash(AutoLang) | 跟 030 ash-gui §4.4(SmartCommand 表单)协同;跟方向 B(补全)的"SmartCommand 补全"接触 |
| **F4 tool-calling** | 用户(REPL) | `auto_ai_agent::Agent::run` + 命令 Tool 桥 + 上下文 builder | 跟 030 ash-gui §4.3(AI 面板)协同;**F4 升级为 Agent 后会替代 F3 的部分场景**(潜在融合点) |
| **F3 NL→pipeline** | 用户(REPL) | 完整 pipeline 执行(已有)+ 上下文 builder + 多步预览 | 跟 SmartCommand 的"NLU 参数解析"是同一能力的两种形态(自然语言→命令),阶段 2 检查是否能合并 |
| **NL→AutoLang 脚本** | 用户(REPL)/ SmartCommand | `Shell::eval_auto`(新)+ 生成-反馈循环 | 跟方向 #3(实例库)协同:生成的脚本可沉淀为实例 |
| **上下文感知** | 所有子能力 | 公开 Shell 访问器 + context builder | 是所有 AI 子能力的公共依赖;跟方向 A(文档:context schema 要文档化)接触 |

**阶段 2 要重点检查的三个潜在融合点**:
1. **F3 vs SmartCommand NLU** —— 都是"自然语言 → 命令",但 F3 是"生成一条命令直接执行",SmartCommand 是"匹配到结构化命令 + 填参数"。阶段 2 决定:合并?还是分工(F3 兜底无 SmartCommand 时的请求)?
2. **F4 tool-calling vs SmartCommand** —— F4 Agent 能调任意命令,SmartCommand 是预封装的命令组合。阶段 2 决定:F4 调 SmartCommand 吗?(答案应该是"是",SmartCommand 注册成 Tool,F4 自动可调)
3. **NL→AutoLang vs SmartCommand body.ash** —— SmartCommand 的 body.ash 是手写脚本;NL→AutoLang 是 AI 生成脚本。阶段 2 决定:能否让 AI 生成的脚本自动成为新 SmartCommand 的 body?(长期愿景)

---

## 第 2 节:共享基础设施(所有子能力的地基)

> 本节是 §3-§7 的共同前置。三个子模块:**OllamaProvider**(原 029 §1,保留)、**ash → auto-ai-agent 桥**(新)、**上下文 builder**(新)。

### 2.1 OllamaProvider + preferred_provider 链路补全

**(原 029 第 1 节内容,完整保留。摘要如下,详细见原设计)**

**改造范围**(跨 3 个 crate,9 个改动点):

| # | 文件 | 改动 | crate |
|---|---|---|---|
| 1 | `auto-ai-daemon/src/provider/ollama.rs` | **新增** `OllamaProvider`(薄包装,委托 OpenAiProvider) | daemon |
| 2 | `auto-ai-daemon/src/provider/mod.rs:78` | 加 `"ollama" =>` 分支 | daemon |
| 3 | `auto-ai-agent/src/config/role_config.rs` | `RoleConfig` 加 `preferred_provider` 字段 + parse/serialize | agent |
| 4 | `auto-ai-agent/src/config/role_config.rs:328` | `ConfigRole` 覆盖 `preferred_provider()` | agent |
| 5 | `ai-config/src/wire.rs:156` | `CompletionRequest` 加 `preferred_provider: Option<String>` | ai-config |
| 6 | `auto-ai-agent/src/agent.rs:535` | `build_request` 读 role.preferred_provider 填进 req | agent |
| 7 | `auto-ai-daemon/src/server.rs:133` | 读 `req.preferred_provider`,改调 `TierRouter::resolve(tier, pref)` | daemon |
| 8 | `auto-ai-agent/src/builtin_roles/smart_command.rs` | **新增** SmartCommandRole 内置 Role | agent |
| 9 | `auto-ai-agent/src/builtin_roles/mod.rs` | 注册 smart-command Role | agent |

**OllamaProvider 设计**(薄包装):

```rust
// auto-ai-daemon/src/provider/ollama.rs
pub struct OllamaProvider {
    inner: OpenAiProvider,  // Ollama 暴露 OpenAI 兼容 API,直接委托
}

impl OllamaProvider {
    pub fn new(name: String, base_url: String, models: Vec<String>) -> Self {
        Self {
            inner: OpenAiProvider::new(name, base_url, "no-key-needed".to_string(), models),
        }
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    fn name(&self) -> &str { self.inner.name() }
    fn models(&self) -> Vec<String> { self.inner.models() }
    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.inner.complete(req).await
    }
    async fn complete_stream(...) -> ... { self.inner.complete_stream(...).await }
}
```

**`ai-daemon.at` 配置示例**:
```autolang
daemon {
    // ... 既有云端 provider ...
    ollama {
        kind : ollama
        base_url : "http://localhost:11434/v1"
        models : ["ornith-9b", "qwen2.5-coder:7b"]
        max_concurrency : 1   // 本地模型通常单并发
    }
}
```

**preferred_provider 链路补全**(5 个串联小改动,详见原 029 §1.3):
- `RoleConfig` 加字段 → `ConfigRole` 覆盖 trait 方法 → `CompletionRequest` 加字段 → `Agent::build_request` 填入 → daemon server 改用 `TierRouter::resolve(tier, pref)`

**SmartCommandRole 定义**(§3 的 SmartCommand 用,§4 的 F4 不用):
```rust
pub struct SmartCommandRole { system_prompt: String, allowed_tools: Vec<String> }
impl Role for SmartCommandRole {
    fn name(&self) -> &str { "smart-command" }
    fn model_tier(&self) -> ModelTier { ModelTier::Min }       // 本地小模型
    fn preferred_provider(&self) -> Option<String> { Some("ollama".to_string()) }
    fn temperature(&self) -> f64 { 0.1 }                        // 参数解析要确定性
    fn max_turns(&self) -> usize { 3 }
    fn allowed_tools(&self) -> Vec<String> { self.allowed_tools.clone() }
}
```

**兼容性**:所有改动是加法 + 默认 None,零破坏性。

### 2.2 ash → auto-ai-agent 桥(新)

**目的**:让 ash 的 80 个命令能被 auto-ai-agent 的 `Agent::run` 调用。这是 F4 tool-calling 和 SmartCommand 的 NLU 路径的共同依赖。

**关键事实**(探勘确认):
- `auto_ai_agent::tool::Tool` 是唯一的 Tool trait(`async fn execute(&self, args: &JsonValue) -> Result<String, ToolError>`)
- ash 的 `Command` trait(`ash/auto-shell/src/cmd.rs`)是另一个不相关类型
- 需要新建桥接 struct

**桥接设计**:

```rust
// ash/auto-shell/src/ai_bridge.rs(新增)

use async_trait::async_trait;
use auto_ai_agent::tool::{Tool, ToolError};
use serde_json::Value;
use std::sync::{Arc, Mutex};

use crate::cmd::CommandRegistry;
use crate::shell::Shell;

/// 把一个 ash 命令包装成 auto_ai_agent::tool::Tool。
/// Agent 调它时,桥接器把 JSON args 转回 CLI 字符串,调 Shell::execute_for_agent。
pub struct AshCommandTool {
    name: String,
    description: String,
    parameters: Value,  // JSON Schema(从 Command::signature() 推导)
    /// 持有 Shell 的 Arc<Mutex>(因为 Agent::run 是 async,Tool 必须 Send+Sync)
    shell: Arc<Mutex<Shell>>,
}

impl AshCommandTool {
    pub fn new(name: String, description: String, parameters: Value, shell: Arc<Mutex<Shell>>) -> Self {
        Self { name, description, parameters, shell }
    }
}

#[async_trait]
impl Tool for AshCommandTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn parameters(&self) -> Value { self.parameters.clone() }

    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        // 把 JSON args 转成 CLI 字符串
        let cmd_str = json_args_to_cli(&self.name, args);
        // 锁 Shell 执行(阻塞,但 Agent::run 在 async 上下文)
        let mut shell = self.shell.lock().map_err(|_| ToolError::Internal("shell mutex poisoned".into()))?;
        match shell.execute_for_agent(&cmd_str, true) {  // true = JSON 模式
            Ok(Some(output)) => Ok(output),
            Ok(None) => Ok(String::new()),
            Err(e) => Err(ToolError::Execution(format!("{}", e))),
        }
    }
}

/// 把 80 个命令全部包装成 Tool,注册进 auto-ai-agent 的 ToolRegistry。
pub fn register_all_ash_commands(
    agent: &mut auto_ai_agent::Agent,
    shell: Arc<Mutex<Shell>>,
) {
    // 遍历 CommandRegistry,每个命令造一个 AshCommandTool
    // ... (复用 Plan 028 的 derive_schema_from_signature 思路推 JSON Schema)
}
```

**关键技术挑战**:
- **Shell 不是 Send+Sync**:`Agent::run` 要求 Tool 是 Send+Sync,但 `Shell` 持有 AutoVM session 等非 Send 状态。用 `Arc<Mutex<Shell>>` 解决,但意味着所有 tool 调用串行化(可接受,因为 Shell 本来就是单线程交互式)。
- **async 执行 + 同步 Shell**:`execute` 是 async,但 `Shell::execute_for_agent` 是同步。在 async 里直接同步调用会阻塞 executor 线程。v1 接受这个(用 `tokio::task::spawn_blocking` 包装更好,但 v1 简化)。

**这是 §3 SmartCommand 和 §4 F4 的共同基础**。两者的差异只在 Role(SmartCommandRole vs ash 内置的 chat Role)和 system prompt 上。

### 2.3 上下文 builder(新)

**目的**:把 Shell 的当前状态(cwd / 最近命令 / exit code / aliases / 环境)汇总成 system prompt 的一部分,让所有 AI 子能力共享。

**当前状态**(探勘确认):
- `build_system_prompt(cwd)` 只注入 cwd(`ai.rs:17-25`)
- Shell 内部跟踪了 `last_command_line`、`last_command_args`、`last_exit_code`、`aliases`、`dir_stack`、`bookmarks`,但**全是私有,无公开 reader**
- **Shell 不保留上一次输出**(format_output 返回 String 后丢弃)

**设计**:

```rust
// ash/auto-shell/src/ai_context.rs(新增)

/// 构建 AI 上下文字符串(注入 system prompt)。
/// 所有 AI 子能力(F3/F4/SmartCommand/NL→脚本)共用。
pub fn build_context_block(shell: &Shell) -> String {
    let mut lines = Vec::new();
    lines.push(format!("当前目录: {}", shell.pwd().display()));
    lines.push(format!("操作系统: {}", std::env::consts::OS));

    if let Some(last) = shell.last_command_line_pub() {  // 新增 pub 访问器
        lines.push(format!("上一条命令: {} (exit {})", last, shell.last_exit_code()));
    }

    let aliases = shell.aliases_pub();  // 新增 pub 访问器
    if !aliases.is_empty() {
        lines.push(format!("用户别名({} 个): {}", aliases.len(), aliases.iter().take(5).map(|(k,v)| format!("{}='{}'", k, v)).collect::<Vec<_>>().join(", ")));
    }

    // 不含上一次输出(v1 不做输出留存,见决策 2)
    lines.join("\n")
}
```

**需要的 Shell 访问器(新增 pub)**:
- `pub fn last_command_line_pub(&self) -> Option<String>`
- `pub fn last_command_args_pub(&self) -> &[String]`
- `pub fn aliases_pub(&self) -> &HashMap<String, String>`
- `pub fn dir_stack_pub(&self) -> &[PathBuf]`

**输出留存(明确不做)**:
v1 不实现"AI 知道上一次输出"。原因:
- Shell 现在 format_output 后就丢弃,要留存需要环形 buffer + 内存管理
- 输出可能很大(grep 几万行),留存成本高
- **留作独立后续 Plan**(可能跟 030 ash-gui 的 Block 模型协同——Block 天然留存输出)

**上下文的分层注入**:
```
完整 system prompt = base_prompt + context_block + 子能力专属 prompt
                     ↑              ↑                ↑
              "你是 ash 助手"   §2.3 共享       各子能力加(F4: "可调命令见 tools" / SmartCommand: skill 列表 / F3: "返回单条命令")
```

---

## 第 3 节:SmartCommand(详细,原 029 主体)

> **SmartCommand 是"混合执行"的轻量扩展**:把"几步确定性操作 + 一步 AI 判断"封装成一条命令,本地小模型负责 NLU 和 AI 步骤,确定性步骤走现有 Shell。对 AI Agent 透明(注册成 `auto_ai_agent::tool::Tool`,跟 ls/grep 一样)。

**核心洞察**:真实任务通常是混合的——`finish-worktree` 4 步里只有 1 步(commit message 生成)真需要 AI。SmartCommand 正好填这个空:**本地小模型 + 确定性脚本 + Tool 透明注册**。

**(本节内容详细保留原 029 §2-§7 的设计,此处为索引,完整细节见本文件下半部分)**

### 3.1 文件布局(三位一体,沿用 roles 模式)

每个 SmartCommand 是一个目录,含配置 + sidecar:
```
~/.config/ash/smart/                    # 用户级
./smart/                                # 项目级(优先级更高)
ash/auto-shell/smart/                   # 内置(随 ash 发布,优先级最低)
└── git.finish-worktree/
    ├── command.at                      # 配置(Atomic DSL)
    ├── body.ash                        # 执行体(sidecar)
    └── skill.md                        # Skill 文档(sidecar,给 SmartCommandRole 读)
```

### 3.2 `command.at` schema(详见原设计 §2.2)

```autolang
smart_command {
    name        : "git.finish-worktree"
    description : "..."
    script_file : "body.ash"
    skill_file  : "skill.md"
    args : [ { name : "target", type : str, default : "auto", description : "..." }, ... ]
    capabilities : { reads_fs : true, writes_fs : true, spawns_process : true, uses_network : true }
    confirm_before : true
    timeout_sec    : 120
}
```

### 3.3 SmartCommandRole + NLU 流程(详见原设计 §3)

- Role 动态构造(不进 RoleRegistry),`build_smart_role(specs)`
- NLU 流程:加载 specs → 构造 SmartCommandRole → 注册成 `auto_ai_agent::tool::Tool` → `Agent::run(user_msg)`
- LLM 只做"选命令 + 填参数",`max_turns: 3`
- 三路径统一(NLU / 显式 flag / `ash smart <name>`),都调同一个 `execute_body()`

### 3.4 body.ash 执行体(详见原设计 §4)

- body.ash 是 AutoLang 脚本,复用 MS3 的 shell bridge
- 新增 natives:`args.*` / `ai.generate` / `confirm` / `read_file`
- 两阶段执行(async LLM 决策 + 同步 body 执行,规避 Shell 借用难题)
- 受 SecurityPolicy 约束(system() 调用自动受限)

### 3.5 `ash smart` CLI(详见原设计 §5)

```bash
ash smart list
ash smart show <name>
ash smart "<nl>"           # NLU 路径
ash smart <name> --flag    # 显式路径
ash smart reload
# 加上通过 ash agent run <smart-command>(Plan 028)的间接调用
```

### 3.6 `git.finish-worktree` 完整实例(详见附录 A)

三件套(command.at + body.ash + skill.md),验证所有设计点。

### 3.7 SmartCommand 关键修正(相对原 029)

**修正 1**:SmartCommand 注册成 **`auto_ai_agent::tool::Tool`**(通过 §2.2 的桥),不是虚构的 `ash_core::tool::Tool`。原 029 设计里所有"Tool Registry"引用应理解为 auto-ai-agent 的 `ToolRegistry`。

**修正 2**:SmartCommandRole 用 §2.1 的 `preferred_provider = "ollama"`,NLU 走本地小模型。body.ash 里的 `ai.generate` 也走 Ollama(经 aaid daemon)。

**修正 3**:SmartCommand 的 NLU 路径现在跟 F4 tool-calling 共享 §2.2 的 ash→agent 桥。两者都用 `Agent::run`,差异只在 Role(SmartCommandRole vs ChatRole)和 system prompt。

---

## 第 4 节:F4 tool-calling(重概要)

> **F4 chat 从纯对话升级为能调命令的 Agent**。看到 LLM 返回的 `tool_calls`,执行对应命令,把结果回填给 LLM,继续对话。这是对标 Warp Agent 模式的核心能力。

### 4.1 当前状态(探勘确认)

- F4 chat 硬编码 `tools: Vec::new()`(`ai.rs:163`),完全 greenfield
- 只读 `resp.content`(纯文本),忽略 `resp.tool_calls`
- wire 层齐备(`Message::tool_result` / `ContentBlock::ToolUse` / `CompletionResponse::wants_tool()`)
- `auto-ai-agent` 有完整 ReAct 循环(`Agent::run` / `Agent::run_stream`),但**不是 ash 的依赖**

### 4.2 设计:F4 改用 Agent::run

**核心改动**:`ChatSession` 不再直接调 `client.complete_stream`,而是构造 `auto_ai_agent::Agent`,注册 ash 命令为 Tool,跑 `Agent::run_stream`。

```rust
// frontend/ai.rs 改造后的 ChatSession(骨架)
pub struct ChatSession {
    messages: Vec<Message>,       // 保留:历史持久化
    history_path: PathBuf,
    client: AiClient,             // 保留:daemon 连接
    shell: Arc<Mutex<Shell>>,     // 新增:供 AshCommandTool 用
}

impl ChatSession {
    pub async fn send_turn_streaming(&mut self, user: &str, system: &str) -> Result<String, String> {
        // 1. 构造一个 ChatRole(用 ash 内置的对话 Role,云端 tier:mid)
        let role = ChatRole::new(system.to_string());
        // 2. 构造 Agent,注册 80 命令
        let client_arc: Arc<dyn Client> = Arc::new(self.client.clone());
        let mut agent = Agent::new(role, client_arc);
        crate::ai_bridge::register_all_ash_commands(&mut agent, self.shell.clone());
        // 3. 注入历史(memory)
        for msg in &self.messages { agent.replay_message(msg); }  // 假设有此 API
        // 4. 流式跑
        let on_event = |ev: StreamEvent| { /* 渲染 Delta/ToolStart/Tool 到终端 */ };
        let result = agent.run_stream(user, Arc::new(on_event), Arc::new(AtomicBool::new(false))).await?;
        // 5. 持久化(只存 user/assistant 文本,不存 tool 中间步)
        self.push_user(user);
        self.push_assistant(&result.output);
        Ok(result.output)
    }
}
```

### 4.3 ChatRole 设计

F4 用一个简单的 ChatRole(不走 SmartCommandRole,不走 Ollama):

```rust
pub struct ChatRole { system_prompt: String }
impl Role for ChatRole {
    fn name(&self) -> &str { "ash-chat" }
    fn system_prompt(&self) -> &str { &self.system_prompt }
    fn model_tier(&self) -> ModelTier { ModelTier::Mid }   // 云端大模型
    fn temperature(&self) -> f64 { 0.4 }                   // 对话要有一定创造性
    fn max_turns(&self) -> usize { 20 }                    // 复杂任务可能多轮
    fn allowed_tools(&self) -> Vec<String> { vec!["*".into()] }  // 全部命令可选
}
```

注意:**不用 `preferred_provider`**(走默认 tier 路由,即云端)。F4 的对话/工具调用质量要高,本地小模型不够。

### 4.4 流式事件渲染

`Agent::run_stream` 发出 `StreamEvent::{Delta, Thinking, ToolStart, Tool, Done, Cancelled, Error}`。F4 的渲染器要把它们漂亮地呈现:

| 事件 | 渲染 |
|---|---|
| `Delta(text)` | 实时打印文本(现有 `print!("{}", text)` 行为) |
| `Thinking(text)` | 灰色斜体(模型思考过程) |
| `ToolStart { name, input }` | `🛠 调用 ls {path: "/tmp"}`(蓝色) |
| `Tool { name, result }` | `  → 返回 3 行`(灰色,折叠结果) |
| `Done` | 换行 |
| `Error(msg)` | 红色错误 |

这是 Warp 式的"看到 AI 怎么干活"体验。

### 4.5 F4 vs F3 的明确分工

| 维度 | F3(one-shot) | F4(chat + tool-calling) |
|---|---|---|
| 交互 | 一次输入 → 一条命令 → 执行/取消 | 多轮对话,可调多命令 |
| 适用 | "我想做 X"(简单翻译) | "帮我分析这个问题"(探索、调试、组合) |
| 模型层 | 云端(tier:mid,low temperature) | 云端(tier:mid/pro) |
| 命令数 | 0 或 1(生成一条) | 任意(多轮 tool 调用) |
| 上下文 | 仅当前 cwd | 完整对话历史 + 命令结果 |

**F3 不会被 F4 废弃**——它覆盖"快速单命令"场景,F4 覆盖"复杂任务"场景。两者并存。

### 4.6 持久化策略

F4 现在持久化 `~/.auto-shell-ai-chat.json`(纯文本对话)。升级后,Tool 中间步骤(`ToolUse`/`ToolResult` blocks)是否持久化?

**v1 决策**:**只持久化 user/assistant 文本**,不持久化 tool 中间步。原因:
- tool 结果可能很大(ls 几万行),持久化成本高
- 对话历史用于"上下文回忆",tool 步骤是过程不是结论
- 重启后加载历史,LLM 看到的是"用户问 X,我答 Y",中间工具调用由 LLM 重新决定

如果用户想保留 tool 步骤(可追溯),那是 **030 ash-gui 的 Block 模型**(Block 完整记录所有 sub_block)的范畴,不是 F4 的。

### 4.7 风险

- **`Arc<Mutex<Shell>>` 在 async 下的死锁**:如果 Shell 在持锁时回调到 Agent(不太可能,但要验证)。缓解:`Agent::run` 在 `Tool::execute` 里只同步调 Shell,不回调。
- **历史回放的语义**:把旧文本对话喂给新 Agent,可能改变 LLM 行为(它看到没有 tool_calls 的纯对话)。缓解:回放时包装成"历史摘要"而非原始 messages。

---

## 第 5 节:F3 NL→pipeline 增强(重概要)

> **F3 从"返回一条命令"升级到"pipeline + 多步预览 + AutoLang 检测"**。地基已有(prompt 已提 pipeline、Shell::execute 处理 pipeline),主要补验证/预览/AutoLang 触发。

### 5.1 当前状态(探勘确认)

- F3 的 system prompt(`repl.rs:355-367`)明说 "SINGLE ash shell command (or pipeline)",且给了 ash pipeline 示例
- `Shell::execute` 已处理 pipeline(`ls | .size > 10.mb | sort .name`)
- F3 当前:LLM 返回字符串 → 去 markdown fence → 用户确认 → 执行
- **缺**:验证生成的命令是否合法、多步预览、AutoLang 脚本检测

### 5.2 增强点

#### 5.2.1 命令验证(新)

LLM 可能生成非法命令(语法错、不存在的命令)。F3 增强加一步 dry-run 验证:

```rust
fn validate_suggestion(shell: &Shell, cmd: &str) -> Result<(), Vec<String>> {
    let mut warnings = Vec::new();
    // 1. 语法检查:管道/重定向是否平衡
    if !is_balanced(cmd) { warnings.push("管道/重定向可能不平衡".into()); }
    // 2. 命令存在性:第一个 token 是否在 CommandRegistry
    let first = cmd.split_whitespace().next().unwrap_or("");
    if !shell.has_command(first) && !is_external_command(first) {
        warnings.push(format!("命令 '{}' 可能不存在", first));
    }
    // 3. 危险模式:rm -rf / 等
    if is_dangerous(cmd) { warnings.push("⚠️ 危险命令".into()); }
    if warnings.is_empty() { Ok(()) } else { Err(warnings) }
}
```

如果验证失败,F3 给用户警告(仍允许执行,只是提示)。

#### 5.2.2 多步预览(新)

如果 LLM 返回 `cmd1 && cmd2 && cmd3`,F3 把它拆成多步预览:

```
F3 建议(3 步):
  1. find . -name "*.log"
  2. grep ERROR
  3. wc -l

[Enter] 全部执行  [1/2/3] 只执行某步  [e] 编辑  [Esc] 取消
```

#### 5.2.3 AutoLang 检测(新)

如果用户的问题暗示多步逻辑(循环、条件),F3 检测到后**切换到 §6 的 NL→AutoLang 路径**:

- "把所有 .log 文件按日期分组" → AutoLang 脚本(for 循环)
- "找出最大的 10 个文件" → 单命令(`ls | sort | head`)
- "如果 disk 超过 80% 就发邮件" → AutoLang 脚本(if + system)

检测方式:LLM 在 system prompt 里被告知"如果需要循环/条件,返回 `[AUTO]` 前缀 + AutoLang 代码"。F3 看到前缀就转 §6。

### 5.3 F3 增强 vs F4 tool-calling

F3 增强后跟 F4 有功能重叠。分工见 §4.5:F3 是"一次性翻译",F4 是"多轮探索"。F3 增强不引入 tool-calling(仍是单次 LLM 调用 + 执行),F4 才有 Agent loop。

### 5.4 范围外

- F3 不变成对话式(保持 one-shot)
- F3 不主动调用 SmartCommand(那是用户显式 `ash smart` 的事)
- F3 不做"基于历史的命令建议"(那是 §7 上下文感知的"Warp 式建议下一条")

---

## 第 6 节:NL→AutoLang 脚本(重概要)

> **自然语言生成多步 AutoLang 脚本**。比 F3 单命令更强:支持 `fn`/`while`/`try-catch`/`if`,处理复杂逻辑。

### 6.1 可行性(探勘确认)

- `AutovmReplSession::run(code: &str)` 接受任意 AutoLang(含 `fn` 定义),跨调用持久化
- 这就是 `~/.ashrc` 的工作方式(`repl.rs:51-62` 加载)
- 缺:公开 API + 生成-反馈循环

### 6.2 设计:`Shell::eval_auto` 公开方法

```rust
// shell.rs 新增
/// 公开包装私有 session_run,供 AI 子能力用。
/// 执行 AutoLang 代码(可以是表达式、语句、fn 定义),返回格式化结果。
pub fn eval_auto(&mut self, code: &str) -> Result<Option<String>> {
    self.execute_auto(code)  // 已存在的私有方法
}
```

`execute_auto` 已经做了 `begin_run/end_run` 的安全包装(Plan 011 的 host bridge),直接 pub 即可。

### 6.3 NL→AutoLang 的生成-反馈循环

```
用户:"把所有 .log 文件按日期分组"
   ↓
LLM 生成 AutoLang 脚本(带 fn 定义):
   fn group_logs_by_date() {
       var logs = system("find . -name '*.log'")
       var grouped = {}
       for log in split(logs, "\n") {
           var date = system("stat -c %y " + log)
           grouped[date] = log
       }
       return grouped
   }
   print(group_logs_by_date())
   ↓
ash eval_auto(脚本)
   ↓ 成功 → 打印结果
   ↓ 失败 → 把错误喂回 LLM,让它修复(最多 N 轮)
```

**关键**:这是 LLM-in-the-loop 的代码生成。每轮:
1. LLM 生成脚本
2. `eval_auto` 执行
3. 成功 → 输出
4. 失败 → 错误信息 + 原脚本喂回 LLM,"修复这个错误"

`max_retries: 3`,超过则放弃,提示用户手动。

### 6.4 安全约束

`eval_auto` 走 shell bridge,所以脚本里的 `system()` 调用**自动受 SecurityPolicy 约束**:
- `--read-only` 模式下,生成脚本里的写操作被拦
- `--sandbox` 模式下,路径受限
- `--no-network` 模式下,网络调用被拦

这是 SmartCommand 的 body.ash 共享的安全机制(§3.4)。

### 6.5 NL→AutoLang vs SmartCommand body.ash

两者都生成/执行 AutoLang,但:
- **NL→AutoLang**:即时生成 + 执行 + 丢弃(一次性)
- **SmartCommand body.ash**:手写 + 持久化 + 可重复调用(封装成命令)

**长期融合点**(阶段 2 检查):NL→AutoLang 生成的脚本,用户满意后可"保存为 SmartCommand"。这是 §1 提的潜在融合点之一。

### 6.6 范围外

- 完整 AutoLang IDE(语法检查、补全、调试)—— 留给 AutoCoder(在 auto-musk)
- 脚本持久化(自动存为 SmartCommand)—— v2,需 §1 融合点决策
- 跨会话脚本记忆 —— 留给后续

---

## 第 7 节:上下文感知(重概要)

> **AI 知道 shell 的完整状态**。对标 Warp 的"建议下一条命令"和"知道你在干什么"。

### 7.1 当前状态(探勘确认)

- `build_system_prompt(cwd)` 只注入 cwd
- Shell 内部跟踪:cwd / `last_command_line` / `last_command_args` / `last_exit_code` / `aliases` / `dir_stack` / `bookmarks` / `vars`
- **几乎全是私有,无公开 reader**
- **不保留上一次输出**(决策 2:v1 不做输出留存)

### 7.2 设计:公开 Shell 访问器 + 增强 context builder

§2.3 已定义 `build_context_block(shell)`。本节细化"上下文"的层级:

| 层级 | 内容 | 用途 |
|---|---|---|
| **L0 静态** | OS / shell 版本 / 当前 cwd | 所有子能力共享 |
| **L1 会话** | 最近 N 条命令 / exit codes / 当前 dir_stack | F4 对话、F3 翻译参考 |
| **L2 别名** | 用户 aliases(前 5 个) | F3/F4 知道用户的快捷方式 |
| **L3 输出** | (v1 不做)上一次命令的输出摘要 | 留给后续 Plan(可能跟 030 Block 协同) |

```rust
pub fn build_context_block(shell: &Shell) -> String {
    let mut lines = vec![
        format!("操作系统: {}", std::env::consts::OS),
        format!("当前目录: {}", shell.pwd().display()),
    ];
    if let Some(last) = shell.last_command_line_pub() {
        lines.push(format!("上一条命令: {} (exit {})", last, shell.last_exit_code()));
    }
    let aliases = shell.aliases_pub();
    if !aliases.is_empty() {
        let preview = aliases.iter().take(5).map(|(k,v)| format!("{}='{}'", k, v)).collect::<Vec<_>>().join(", ");
        lines.push(format!("用户别名({} 个): {}", aliases.len(), preview));
    }
    lines.join("\n")
}
```

### 7.3 Warp 式"建议下一条命令"(新)

这是上下文感知的高阶应用:命令执行完后,**主动**建议下一条。

```
$ ls -la
README.md  src/  tests/

💡 接下来可能想:
   - cat README.md(查看 readme)
   - cd src(进入源码目录)
   - find . -name "*.rs"(找所有 Rust 文件)
[Tab] 接受建议  [Esc] 忽略
```

**实现**:命令执行后,异步调 LLM(tier:min,本地 Ollama 即可),给它上下文(cwd + 刚执行的命令 + 输出摘要),让它返回 3 个建议。建议显示在 prompt 下方,用户 Tab 接受。

**约束**:
- 异步预取(不等 LLM 不能阻塞 shell)
- 用户可关(`~/.config/ash/config.at` 里 `ai.suggest_next: false`)
- 仅在交互式 REPL 触发(脚本模式不触发)

### 7.4 上下文的安全考虑

上下文块进 system prompt,可能包含敏感信息(路径、变量名)。约束:
- **不注入环境变量值**(只注入 keys,不注入 values——避免泄露 token)
- **不注入文件内容**(那是输出留存,决策 2 不做)
- 用户可审查(`ash ai context` 命令打印当前上下文块)

### 7.5 范围外

- 输出留存(L3,v1 不做)
- 跨会话上下文(F4 已有 `~/.auto-shell-ai-chat.json`,其他子能力不做)
- 学习用户习惯(个性化模型训练)—— 远期

---

## 第 8 节:里程碑、依赖、风险、非目标(总览)

### 8.1 子能力实施顺序(依赖驱动)

```
M0(前置):共享基础设施
  ├─ §2.1 OllamaProvider + preferred_provider(auto-ai 改造)
  ├─ §2.2 ash → auto-ai-agent 桥(AshCommandTool)
  └─ §2.3 上下文 builder + Shell 公开访问器
       ↓
M1: SmartCommand(原 029 的 M1-M4)
  └─ 完整 SmartCommand 引擎 + git.finish-worktree
       ↓
M2: F4 tool-calling
  └─ ChatSession 改用 Agent::run
       ↓
M3: F3 增强 + NL→AutoLang
  └─ 验证 + 多步预览 + eval_auto + 反馈循环
       ↓
M4: 上下文感知 + Warp 式建议
  └─ 主动建议 + 安全约束
```

**为什么这个顺序**:
- M0 是所有子能力的地基,必须先做
- SmartCommand(M1)最完整(已有详细设计),先交付价值
- F4(M2)依赖 M0 的桥,做完 F4 后 Agent 基础设施完全成熟
- F3 增强 + NL→AutoLang(M3)复用 F4 的 LLM 调用模式
- 上下文感知(M4)在所有子能力都就绪后,体验层打磨

### 8.2 工作量估算

| 里程碑 | 范围 | 估算 |
|---|---|---|
| **M0 共享基础设施** | auto-ai 改造(§2.1)+ 桥(§2.2)+ 上下文(§2.3) | 3-4 周 |
| **M1 SmartCommand** | 完整 SmartCommand 引擎 + git.finish-worktree | 3-4 周(原 029 的 M1-M4) |
| **M2 F4 tool-calling** | ChatSession 改造 + ChatRole + 流式渲染 | 2-3 周 |
| **M3 F3 + NL→AutoLang** | F3 增强 + eval_auto + 反馈循环 | 2-3 周 |
| **M4 上下文 + 建议** | Warp 式建议 + 安全约束 | 2 周 |
| **总计** | 全部 5 个子能力 | **12-16 周(3-4 个月)** |

### 8.3 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| **`Arc<Mutex<Shell>>` 死锁**(§2.2) | 中 | 高 | 严格限制:Tool::execute 只同步调 Shell,不回调 Agent;加 timeout |
| **Ollama 模型质量**(SmartCommand commit message) | 中 | 中 | M1 验收对比 Ornith-9B / Qwen2.5-Coder |
| **auto-ai PR 不被接受**(§2.1 改造) | 中 | 中 | 改动是加法+默认 None,review 阻力小;最坏 fork |
| **F4 历史回放语义**(§4.7) | 中 | 中 | 包装成"历史摘要"而非原始 messages;v1 只回放文本 |
| **Shell 借用难题**(SmartCommand 两阶段执行) | 中 | 中 | §3.4 已定两阶段(async 决策 + 同步执行) |
| **NL→AutoLang 生成质量**(§6) | 高 | 中 | max_retries + 错误反馈循环;失败时降级回 F3 单命令 |
| **上下文泄露敏感信息**(§7.4) | 中 | 高 | 不注入 env values;`ash ai context` 可审查 |
| **auto-lang/auto-ai 反复被打断** | 高 | 中 | pin 到稳定 commit;跟维护者协调 |

### 8.4 非目标(明确排除)

- ❌ **输出留存**(Shell 保留上一次输出)—— v1 不做,独立后续 Plan
- ❌ **重写 ReAct 循环** —— 用 auto-ai-agent 现成的
- ❌ **AutoDown → Markdown 转译** —— 探勘证实不存在,sidecar 模式
- ❌ **SmartCommand 编辑器/IDE** —— 用户手写 command.at + body.ash
- ❌ **远程 SmartCommand 分发**(central registry)—— 独立后续 Plan
- ❌ **个性化模型训练**(学习用户习惯)—— 远期
- ❌ **F3 变对话式** —— 保持 one-shot
- ❌ **F4 替代 F3** —— 两者分工并存(§4.5)

### 8.5 成功指标

1. **M0**:Ollama 可调;preferred_provider 端到端通;`build_context_block` 工作
2. **M1**:用 `ash smart "finish this worktree"` 完成真实 worktree,commit message 质量可接受
3. **M2**:F4 chat 里问"列出当前目录最大的 3 个文件",LLM 调 ls+sort+head 三次 tool,给出答案
4. **M3**:NL→AutoLang 生成"按日期分组 .log 文件"脚本,执行成功
5. **M4**:命令执行后看到"💡 接下来可能想"建议,Tab 能接受
6. **老接口零破坏**:F4 升级后老对话历史仍能加载;F3 行为兼容

### 8.6 跟其他方向的关系(给阶段 2 横向检查)

| 方向 | 关系 |
|---|---|
| **Plan 028**(Agent 执行引擎,已落地) | F4 tool-calling 的 AshCommandTool 复用 028 的 `execute_for_agent` JSON 路径 |
| **Plan 030**(ash-gui) | F4 在 GUI 里是 AI 面板(§4.3);SmartCommand 在 GUI 是表单(§4.4);Block 模型可能解决"输出留存" |
| **方向 B**(智能补全) | "建议下一条命令"(§7.3)跟补全系统有重叠,阶段 2 决定融合点 |
| **方向 #3**(实例库) | NL→AutoLang 生成的脚本可沉淀为实例(§6.5 融合点) |
| **方向 #5**(数据处理) | SmartCommand 的 ai.generate 可能调用数据处理能力 |
| **方向 C**(插件生态) | SmartCommand 是"轻量插件",C 是"重量插件",阶段 2 决定关系 |

---

## 附录 A:SmartCommand 详细设计(原 029 §2-§6 完整保留)

> 本附录保留原 SmartCommand 设计的完整细节(command.at 格式、加载器、Role 集成、body.ash 执行、CLI、git.finish-worktree 实例)。修正点:所有原 "Tool Registry" 引用理解为 `auto_ai_agent::tool::ToolRegistry`;所有 "ash_core::tool::Tool" 引用理解为 `auto_ai_agent::tool::Tool`。

### A.1 文件布局(三位一体)

```
~/.config/ash/smart/              # 用户级
./smart/                          # 项目级(优先级更高)
ash/auto-shell/smart/             # 内置(随 ash 发布,优先级最低)
└── git.finish-worktree/
    ├── command.at                # 配置(Atomic DSL)
    ├── body.ash                  # 执行体(sidecar)
    └── skill.md                  # Skill 文档(sidecar,给 SmartCommandRole 读)
```

### A.2 `command.at` schema

```autolang
smart_command {
    name        : "git.finish-worktree"
    description : "完成 worktree:生成 commit message → 提交 → merge 回主分支 → 删除 worktree → push"
    script_file : "body.ash"
    skill_file  : "skill.md"

    args : [
        { name : "target", type : str, default : "auto", description : "merge 回哪个分支" },
        { name : "push", type : bool, default : true, description : "是否 push 到远端" },
        { name : "message_source", type : str, enum : ["diff", "plan", "manual"], default : "diff", description : "commit message 来源" },
        { name : "plan_file", type : str, required : false, description : "plan 文件路径" }
    ]

    capabilities : { reads_fs : true, writes_fs : true, spawns_process : true, uses_network : true }
    confirm_before : true
    timeout_sec    : 120
}
```

**顶层字段**:`name`(必需)、`description`(必需)、`script_file`(必需)、`skill_file`(可选)、`args`(可选)、`capabilities`(可选)、`confirm_before`(默认 false)、`timeout_sec`(默认 60)。

**args 每项字段**:`name`(必需)、`type`(必需,str/bool/int/float)、`description`(必需)、`required`(默认 false)、`default`(可选)、`enum`(可选)。

### A.3 SmartCommandSpec + parse/serialize(照搬 role_config.rs 模式)

```rust
// ash/auto-shell/src/smart_command/config.rs
pub struct SmartCommandSpec {
    pub name: String,
    pub description: String,
    pub script_file: String,
    pub skill_file: Option<String>,
    pub args: Vec<SmartArg>,
    pub capabilities: Capabilities,
    pub confirm_before: bool,
    pub timeout_sec: u64,
    pub base_dir: PathBuf,
}

pub struct SmartArg { pub name: String, pub ty: SmartArgType, pub description: String, pub required: bool, pub default: Option<String>, pub enum_values: Option<Vec<String>> }
pub enum SmartArgType { Str, Bool, Int, Float }

pub fn parse_smart_command(content: &str, base_dir: PathBuf) -> Result<SmartCommandSpec, SmartError>;
pub fn serialize_smart_command(spec: &SmartCommandSpec) -> String;
```

### A.4 加载器:扫 smart/ 目录

```rust
// ash/auto-shell/src/smart_command/loader.rs
pub fn load_all() -> Result<Vec<SmartCommandSpec>, SmartError>;
// 搜索路径优先级: $CWD/smart/ > ~/.config/ash/smart/ > 内置
// 同名时高优先级覆盖低优先级
```

### A.5 SmartCommandRole + NLU 流程

**关键决策**:SmartCommandRole 是**动态构造**(不进 RoleRegistry),由加载器 `build_smart_role(specs)` 实例化。

```rust
// ash/auto-shell/src/smart_command/role.rs
pub fn build_smart_role(specs: &[SmartCommandSpec]) -> SmartCommandRole {
    let available = render_available_skills(specs);  // 把每个 SmartCommand 渲染成 markdown
    let allowed: Vec<String> = specs.iter().map(|s| s.name.clone()).collect();
    SmartCommandRole::new(build_prompt(&available), allowed)
}
```

**NLU 流程**:
1. 加载 specs → `build_smart_role(specs)`
2. 构造 Agent::new(smart_role, ai_client)
3. 注册每个 SmartCommand 为 `auto_ai_agent::tool::Tool`(经 §2.2 的桥)
4. `Agent::run(user_msg)` → LLM 选命令 + 填参数 → Tool::execute
5. 输出信封(复用 Plan 028 的 build_envelope)

**LLM 只做"选命令 + 填参数",max_turns: 3**。这把 LLM 限制在"参数解析器"角色,确保 SmartCommand 经济性(本地小模型够用)。

### A.6 body.ash 执行体

body.ash 是普通 AutoLang 脚本(MS3 已支持),复用 shell bridge。新增 natives:

| Native | 作用 |
|---|---|
| `system(cmd)` | 执行 shell 命令(已有,MS3) |
| `read_file(path)` / `write_file(path, content)` | 文件 I/O(新) |
| `confirm(prompt)` | 交互确认(新) |
| `ai.generate(prompt, context)` | 一次性本地 AI 调用(新,直连 OllamaProvider) |
| `args.str(name)` / `args.bool(name)` | 取解析后的参数(新) |

**`execute_body` 调度函数**(三路径共同终点):

```rust
// ash/auto-shell/src/smart_command/executor.rs
pub fn execute_body(spec: &SmartCommandSpec, args_json: &Value, shell: &mut Shell) -> ToolResult {
    // 1. 参数校验 + 类型转换
    // 2. 读 body.ash
    // 3. 构造 AutoVM,注入 smart_command natives
    // 4. 执行(受 shell policy 约束)
    // 5. 包装成 ToolResult 返回
}
```

**`ai.generate` native 的实现**(直连 provider,不走 Agent ReAct):

```rust
pub fn ai_generate(prompt: &str, context: &str) -> String {
    let client = AiClient::new().expect("aaid daemon unavailable");
    let req = CompletionRequest {
        model: "tier:min".to_string(),
        preferred_provider: Some("ollama".to_string()),  // §2.1 补全的字段
        messages: vec![Message::system("..."), Message::user(format!("{}\n\n{}", prompt, context))],
        max_tokens: Some(256),
        temperature: Some(0.3),
        ..Default::default()
    };
    // 同步 block_on(复用 Plan 027 的 block_on_async)
}
```

**两阶段执行**(规避 Shell 借用难题):LLM 决策(async)+ body 执行(同步),天然分离。

**SecurityPolicy 协同**:body.ash 里 system() 调用自动过 MS2 policy;SmartCommand 额外有 `confirm_before` 层。

### A.7 `ash smart` CLI

```bash
ash smart list                                  # 列出所有 SmartCommand
ash smart show <name>                           # 查看详情
ash smart "<nl>"                                # NLU 路径(走 SmartCommandRole + Ollama)
ash smart <name> --flag                         # 显式路径(零 AI)
ash smart "<nl>" --dry-run                      # 只解析不执行
ash smart reload                                # 重新加载
# 加上通过 ash agent run <smart-command>(Plan 028)间接调用
```

**自然语言 vs 显式判定**:第一个参数匹配 SmartCommand name → 显式;否则 → 自然语言。

**Shell 借用难题的解法**(§3.4):两阶段执行 —— async 阶段 Agent 只决策(选命令 + 填参数),同步阶段 execute_body 执行。LLM 决策和 Shell 执行天然可以分离。

### A.8 `git.finish-worktree` 完整三件套

**command.at**(详见原设计 §6.2,此处不重复,实施时从 brainstorming 历史复制)

**body.ash**(完整,~80 行 AutoLang 脚本):
- 步骤 1:确定目标分支(确定性,`detect_main_branch()`)
- 步骤 2:生成 commit message(AI,`ai.generate(...)`,根据 message_source 走 diff/plan/manual 三路)
- 步骤 3:确认步骤(`confirm(...)`,展示所有副作用)
- 步骤 4:执行 git 操作(确定性,`system("git ...")` × 6 次)
- 辅助函数:`detect_main_branch` / `current_branch` / `current_worktree_path` / `guess_plan_file` / `shell_escape`

**skill.md**(给 SmartCommandRole 读,定义何时调用 + 参数选择指引 + 示例对照表)

### A.9 验证场景(§7.3-§7.4 原设计)

四种调用方式(NLU / 显式 flag / Agent CLI / --dry-run)+ 四个安全场景(主分支拒绝 / read-only 拦 / network 拦 / 确认取消)。

---

## 附录 B:实施前置勘探记录(2026-07-20 + 2026-07-21)

### B.1 原勘探(2026-07-20,SmartCommand 设计基础)

**auto-ai 架构发现**:
- Provider trait 在 `auto-ai-daemon/src/provider/mod.rs`(不在 client),只有 Anthropic + OpenAI 两个实现,**无 Ollama**
- Ollama 兼容性已在配置层预留(`ProviderConfig::resolve_key()` 返回 `"no-key-needed"`)
- Role 系统已存在:14 内置 Role + 用户自定义
- preferred_provider 半搭好:`Role::preferred_provider()` 和 `TierRouter::resolve(tier, pref)` 存在,但 RoleConfig/ConfigRole/CompletionRequest/agent.rs/server.rs 五处未接通
- 无 tier:local:Tiers 是能力档,本地性是 provider 层的事
- function-calling 完整实现:`Agent::run` / `run_stream` 有完整 ReAct 循环
- aaid daemon 是唯一 LLM 网关

**AutoLang 配置 DSL 发现**:
- AutoLang 配置 DSL 完全成熟(`daemon.at` / `roles/<name>.at` / `config.at` 是生产级配置)
- 有 `key : value` / 嵌套块 / 数组 / 对象数组 / 注释 / 裸标识符枚举
- `parse_at_role` + `serialize_at_role` 已验证可往返
- AutoDown 是真实模块但只转 Typst/HTML,**无 Markdown transpiler**
- AutoLang → Markdown codegen 不存在:现有解法是 sidecar 文件(roles 的 `.soul.md` 模式)
- AutoLang 无三引号多行字符串:大段文本走 sidecar 是已设计模式

### B.2 扩展勘探(2026-07-21,吸收 #1 AI 能力增强)

**F4/F3/AutoLang/context 现状**:

**(a) F4 tool-calling 状态**:
- `ChatSession::send_turn_streaming` 硬编码 `tools: Vec::new()`(`ai.rs:163`)
- 只读 `resp.content`,忽略 `resp.tool_calls`
- wire 层齐备(`Message::tool_result`、`ContentBlock::ToolUse`、`CompletionResponse::wants_tool()`)
- `auto-ai-agent::Agent::run` 完整 ReAct 循环已实现 + 测试
- **关键纠正**:`auto-ai-agent` 不是 ash 的依赖(Cargo.toml 只有 `auto-ai-client`)
- **关键纠正**:**`ash_core::tool::Tool` trait 不存在**(grep 整个 ash workspace 零命中),Plan 028 没有创建 Tool trait,只有 `auto_ai_agent::tool::Tool`

**(b) F3 NL→pipeline 状态**:
- F3 system prompt(`repl.rs:355-367`)已明说 "SINGLE ash shell command (or pipeline)"
- `Shell::execute` 已处理 pipeline
- F3 当前:LLM 返回字符串 → 去 markdown fence → 用户确认 → 执行
- 缺:验证、多步预览、AutoLang 检测

**(c) NL→AutoLang 可行性**:
- `AutovmReplSession::run(code: &str)` 接受任意 AutoLang(含 fn 定义),跨调用持久化
- 这就是 `~/.ashrc` 的工作方式
- 缺:公开 API(私有 `session_run` 已存在,需 pub 包装为 `eval_auto`)

**(d) 上下文感知现状**:
- `build_system_prompt(cwd)` 只注入 cwd
- Shell 内部跟踪:`last_command_line`、`last_command_args`、`last_exit_code`、`aliases`、`dir_stack`、`bookmarks`、`vars`,**几乎全私有,无公开 reader**
- **Shell 不保留上一次输出**(format_output 返回 String 后丢弃)
- 公开访问器只有 `pwd()` / `last_exit_code()` 等少数

### B.3 基于勘探的设计修正

1. **Tool trait 修正**:原 029 错误引用 `ash_core::tool::Tool` → 改为 `auto_ai_agent::tool::Tool`(唯一真实存在)
2. **新增 §2 共享基础设施**:OllamaProvider + ash→agent 桥 + 上下文 builder,作为所有子能力的共同地基
3. **新增 AshCommandTool 桥**(§2.2):用 `Arc<Mutex<Shell>>` 让 80 命令可被 async Agent 调用
4. **v1 不做输出留存**(决策 2):Shell 不保留上一次输出,上下文只含 cwd/命令/exit code/aliases
5. **F4 用 Agent::run**(决策 3):不重写 ReAct 循环

---

## 参考

- `designs/028-agent-execution-engine.md`(已删除,委托给 auto-ai)—— 原始 Agent 引擎设计
- `designs/030-ash-gui.md` —— §4.3 AI 面板 / §4.4 SmartCommand 表单 协同
- `D:\autostack\auto-ai\ARCHITECTURE.md` —— auto-ai 架构文档
- `D:\autostack\auto-ai\crates\ai-config\examples\daemon.at` —— 配置 DSL 范例
- `D:\autostack\auto-ai\crates\auto-ai-agent\src\config\role_config.rs` —— 类型化 .at parse/serialize 模式
- `D:\autostack\auto-ai\crates\auto-ai-agent\src\agent.rs` —— Agent::run / run_stream ReAct 循环
- `D:\autostack\auto-ai\crates\auto-ai-agent\src\tool.rs` —— Tool trait 定义
- `D:\autostack\auto-lang\crates\auto-val\src\emit.rs:26-28` —— "大段文本用 sidecar" 的设计依据
- `D:\autostack\auto-shell\ash\auto-shell\src\frontend\ai.rs:163` —— F4 硬编码 `tools: Vec::new()`(待改造)
- `D:\autostack\auto-shell\ash\auto-shell\src\frontend\repl.rs:355-367` —— F3 system prompt(已支持 pipeline)
- `D:\autostack\auto-shell\ash\auto-shell\src\shell.rs:1364` —— 私有 `session_run`(NL→AutoLang 的基础)
