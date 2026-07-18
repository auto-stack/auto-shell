# Plan 028: ASH Agent 执行引擎设计

> **日期**: 2026-07-17
> **状态**: 设计中(待评审)
> **战略驱动**: Agent 执行引擎优先 —— 让 ASH 成为 AI Agent 首选的命令执行引擎
> **范围**: 外部 Agent CLI + 内置 F4 loop 接口预留 + 统一 Tool Registry
> **预估**: M1-M4 共约 4-6 周(详见第 7 节)

---

## 愿景

> 让 ASH 成为 AI Agent(无论是外部的 Claude Code / Cursor / Codex,还是 ASH 内置的 F4 chat loop)首选的命令执行引擎 —— **安全、确定、结构化、跨平台一致**。

本 Plan 是项目从"一个能跑的 shell"升级为"AI 时代的命令执行基础设施"的关键一步。它是 `docs/roadmap.md` 里所述护城河(*"make ash the safest, most reliable command execution tool for AI Agents"*)的工程化落地。

### 战略定位

ASH 的终极目标是替代 bash/pwsh 成为 AI Agent 的 tool use 执行层。这意味着两个产品形态都要服务好:

1. **外部 Agent CLI** —— Claude Code / Cursor / Codex 等通过 `ash -c "..." --json` 或 `ash agent ...` 调用 ash。
2. **内置 Agent loop** —— ASH 自己的 F4 chat(Plan 027)升级为带 tool-calling 的 Agent。

这两个形态共享同一套 tool 描述(Tool Registry),这是本 Plan 的核心设计决策。

### 范围内 / 范围外

| 范畴 | 本 Plan 包含 | 本 Plan 不包含(留给后续 Plan) |
|---|---|---|
| **外部 Agent 接口** | CLI `run_command` + `--describe` + `check_command` + policy 描述 | MCP server 封装(独立 Plan) |
| **bash 兼容** | L1(命令名)+ L2(长/短 flag、`--`、flag 规范化) | L3 语法(`for/if/$()` 等,AI 转写为 AutoLang) |
| **沙箱感知** | 双 tool(check / run)+ `--describe-policy` + 结构化错误 | policy 的高级规则引擎(独立 Plan) |
| **Tool Registry** | ash-core 内统一 `Tool` trait + JSON Schema 序列化 | 内置 F4 agent loop 完整实现(留接口,Plan 029+) |
| **结构化输出** | `--json` 完备化、错误 schema 化、Atom→JSON Schema | Polars / dataframe / lazy 框架(#5 独立 spec) |
| **跨平台一致性** | 关键命令行为对齐(差异矩阵 + 测试套件) | 新命令开发 |

---

## 核心架构

```
┌──────────────────────────────────────────────────────────────┐
│                  AI Agent(消费者)                            │
│  ┌─────────────────┐         ┌─────────────────────────────┐ │
│  │ 外部 Agent      │         │ 内置 F4 chat agent loop     │ │
│  │ Claude Code /   │         │ (Plan 029+,接口预留)        │ │
│  │ Cursor / Codex  │         │                             │ │
│  └────────┬────────┘         └────────────┬────────────────┘ │
└───────────┼───────────────────────────────┼──────────────────┘
            │ ash -c "..." --json            │ in-process
            │ ash agent run ...              │ function-calling
            ▼                                ▼
┌──────────────────────────────────────────────────────────────┐
│           统一 Tool Registry (ash-core 新增)                  │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ Tool trait:每个命令实现它,产出 JSON Schema + 执行      │  │
│  │ - 对外:序列化为 tool catalog(可 --describe 拉取)      │  │
│  │ - 对内:F4 loop 的 function-calling 工具列表            │  │
│  └────────────────────────────────────────────────────────┘  │
│           ▲              ▲              ▲                     │
│           │              │              │                     │
│  ┌────────┴───────┐ ┌────┴─────────┐ ┌──┴──────────────┐     │
│  │ run_command    │ │ check_command│ │ describe_policy │     │
│  │  (执行)        │ │  (探测)       │ │   (查询边界)     │     │
│  └────────────────┘ └──────────────┘ └─────────────────┘     │
└──────────────────────────────────────────────────────────────┘
            │
            ▼
┌──────────────────────────────────────────────────────────────┐
│              现有执行引擎 + SecurityPolicy                    │
│  Shell::execute_pipeline_with_auto()  +  sandbox/path filter │
│  (MS1 --json + MS2 沙箱 + Plan 024 Atom pipeline)            │
└──────────────────────────────────────────────────────────────┘
```

### 三大支柱

1. **统一的 Tool 描述层** —— 每个命令一次定义,内外两个出口共享。Agent 能"看见"命令的边界。
2. **沙箱可见性** —— Agent 不再"试错",而是"先问后做"。结构化错误让恢复成本最低。
3. **输出确定化** —— `--json` 是契约,不是选项。每条命令的输出 schema 可被静态获取。

### 三条核心架构决策(已在 brainstorming 阶段确认)

- **Tool 抽象 = 大 tool + schema 注册表** —— 对外暴露统一 `run_command` 入口,内部维护每命令的 JSON Schema;Agent 可拉取整个 catalog,也可走单 tool。
- **沙箱接口 = 双 tool + policy 描述** —— `check_command`(dry-run 探测)+ `run_command`(执行)两个 tool,启动时用 `--describe-policy` 拉 policy 摘要。Agent 可"先问后做"。
- **内外共享 = 统一 Tool Registry** —— ash-core 定义 `Tool` trait,每个命令实现;对外序列化 JSON Schema,对内给 F4 chat agent loop 当作 function-calling 工具列表。一份描述,两个出口。

---

## 第 1 节:Tool Registry 详细设计

### 1.1 新增模块结构

在现有 `ash-core` 里新增 `tool` 模块(不新建 crate,YAGNI——单一职责已由 ash-core 承担):

```
ash-core/src/
├── tool/
│   ├── mod.rs              # Tool trait, ToolRegistry, ToolSchema
│   ├── schema.rs           # JSON Schema 生成(基于 serde_json::Map)
│   ├── catalog.rs          # 全局 catalog 序列化(对外 --describe)
│   ├── bridge.rs           # CommandToolBridge:现有 Command → Tool
│   ├── agent_loop.rs       # ToolExecuting trait(仅接口,无实现)
│   └── error.rs            # ToolError 标准化错误类型
└── ... (现有 pipeline/, security/, parser/)
```

### 1.1b 关于 serde_json 依赖的技术决策

**背景**:`ash-core` 当前**完全无 serde/serde_json 依赖**(刻意的轻依赖设计,见 `security.rs` 注释 `// Hand-built JSON`)。但 Tool Registry 的本质是产出/消费 JSON,且现代 JSON Schema 实践以 `serde_json::Value` 为事实标准。

**决策**:为 `ash-core` **新增** `serde = { version = "1", features = ["derive"] }` 和 `serde_json = "1"` 两个依赖,理由:

1. Tool Registry 的核心职责就是 JSON Schema 产出,手写 JSON 会让代码膨胀且易错。
2. `auto-shell` 早已依赖 serde_json,跨 crate 一致性更好。
3. serde 是 Rust 生态的事实标准,引入它不算"重依赖"。

**边界**:serde 只用于 `tool/` 模块(Tool/ToolResult/ToolError 的序列化)。现有的 `Atom`/`AtomPipeline`/`AtomType`/`SecurityPolicy` **保持无 derive**,避免触动已有代码。需要序列化时,在 `tool/` 模块里写转换函数(而非给老类型加 derive)。

> **备选方案**(若 reviewer 强烈反对加 serde):沿用 `security.rs::AuditRecord::to_jsonl()` 的手写 JSON 模式。代价是 schema 推导代码量翻倍、容易出 bug。本 Plan 默认走 serde 方案。

### 1.2 `Tool` trait

```rust
// ash-core/src/tool/mod.rs

use serde_json::{Value, Map};
use crate::pipeline::AtomPipeline;

/// 一个可被 AI Agent 调用的工具。
/// 既是 CLI 入口的契约,也是内置 agent loop 的 function-calling 工具。
pub trait Tool: Send + Sync {
    /// 工具的唯一名称(例如 "ls", "grep", "run_command")
    fn name(&self) -> &str;

    /// 一行人类可读描述(LLM 读它来决定何时调用)
    fn description(&self) -> &str;

    /// 参数的 JSON Schema(OpenAPI/MCP 风格)
    /// 返回形如 {"type":"object", "properties":{...}, "required":[...]}
    fn parameters_schema(&self) -> Map<String, Value>;

    /// 输出 schema(可选;None 表示输出是自由文本)
    /// Agent 用它做后处理规划(例如"我知道 ls 会返回 FileList")
    fn output_schema(&self) -> Option<Map<String, Value>> { None }

    /// 执行工具。args 是已解析的 JSON 对象(来自 LLM 或 CLI)。
    /// 返回结构化的 ToolResult(成功/失败 + 数据 + 诊断)。
    fn invoke(&self, args: &Value, ctx: &ToolContext) -> ToolResult;

    /// 该工具需要的权限能力(供 policy 校验前查询)
    fn capabilities(&self) -> Capabilities { Capabilities::default() }
}
```

### 1.3 `ToolContext` 与 `ToolResult`

```rust
pub struct ToolContext {
    pub cwd: PathBuf,
    pub policy: SecurityPolicy,         // 复用 MS2 的现有 policy
    pub env: HashMap<String, String>,
    pub output_format: OutputFormat,    // Json | Atom | Text
    pub timeout: Option<Duration>,
    pub confirmation_mode: ConfirmationMode,  // 见第 4 节
    pub limits: OutputLimits,                 // 见第 5 节
}

pub enum OutputFormat { Json, Atom, Text }

pub struct ToolResult {
    pub status: ToolStatus,
    pub data: ToolData,
    pub diagnostics: Vec<Diagnostic>,   // 警告/降级提示
    pub timing: Timing,                 // wall/user/sys,Agent 可用于决策
}

pub enum ToolStatus {
    Success,
    Denied(DeniedReason),              // policy 拒绝,Agent 应停止重试
    Failed(FailureKind, String),       // 执行失败,Agent 可重试/换方案
    PartialSuccess(String),            // 部分完成,附带说明
}

pub enum ToolData {
    Json(Value),                       // 已 schema 化的结构化输出
    Atom(AtomPipeline),                // 走现有 Atom pipeline
    Text(String),                      // 纯文本(兼容传统命令)
    Empty,
}

pub struct DeniedReason {
    pub rule_id: &'static str,         // 例如 "path-outside-sandbox"
    pub message: String,
    pub remediation: Option<String>,   // Agent 可读的恢复建议
}
```

**关键设计点**:
- `ToolStatus::Denied` 与 `Failed` **显式区分** —— `Denied` 告诉 Agent"别再试了,换路径或请求权限",`Failed` 告诉 Agent"可以重试/换方案"。这是降低 Agent 试错成本的核心。
- `remediation` 字段让 Agent 拿到机器可读的恢复路径(如 "path under /sandbox is writable; use that path")。
- **与现有 `Decision` 枚举的关系**:`ash-core::security::Decision` 只有 `Allow`/`DryRun` 两个变体,**拒绝是 `Err(...)`**(不是 `Decision::Deny`)。`Tool::invoke()` 实现里需要把 `SecurityPolicy::check()` 返回的 `Err(e)` 翻译成 `ToolStatus::Denied(DeniedReason { ... })`,把 `Ok(DryRun)` 翻译成 `ToolStatus::PartialSuccess` 或按配置走 dry-run 路径。

### 1.4 `ToolRegistry`

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    aliases: HashMap<String, String>,   // bash 兼容别名,如 "ll" -> "ls"
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>) { ... }

    /// 导出整个 catalog(供 --describe 和 MCP 注册使用)
    pub fn catalog(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|t| ToolDescriptor {
            name: t.name().into(),
            description: t.description().into(),
            parameters: t.parameters_schema(),
            output: t.output_schema(),
            capabilities: t.capabilities(),
        }).collect()
    }

    /// 按 Agent 的 context-budget 过滤 catalog
    /// (例如只导出最常用的 30 个 tool 的完整 schema,其余只给名字)
    pub fn catalog_compact(&self, max_tools: usize) -> CatalogSummary { ... }
}
```

### 1.5 现有命令的渐进迁移

采用**渐进迁移**,避免一次性重写 80 个命令:

**阶段 0(本 Plan 范围)**:所有现有命令自动获得**最小 Tool 描述**(通过 bridge),包含:
- name(已有)
- description(从现有 `Signature::description` 提取)
- 最小 parameters schema(从 `Signature::arguments` 推导,粗粒度)
- `invoke()` 委托给现有 `run()` / `run_atom()`

```rust
// ash-core/src/tool/bridge.rs —— 让现有 Command 自动成为 Tool
impl<T: Command> Tool for CommandToolBridge<T> {
    fn name(&self) -> &str { self.inner.name() }
    fn description(&self) -> &str { self.inner.signature().description }
    fn parameters_schema(&self) -> Map<String, Value> {
        derive_schema_from_signature(&self.inner.signature())  // 自动推导
    }
    fn invoke(&self, args: &Value, ctx: &ToolContext) -> ToolResult {
        // 把 JSON args 转回 PipelineData,调用现有 run()
        bridge_invoke(&self.inner, args, ctx)
    }
}
```

**阶段 1(后续 Plan)**:逐个为高价值命令(`ls` / `grep` / `find` / `git` 系列等)写**精细 schema**,替换自动推导版。

这样 Plan 完成后,所有 80 个命令立即对 Agent 可见,无需等待逐个迁移。

---

## 第 2 节:外部 Agent CLI 接口

### 2.1 现状回顾

Plan 007(MS1)已实现 `ash -c "cmd" --json`,但 Agent 场景有四个缺口:

1. **没有 catalog 发现机制** —— Agent 不知道 ash 有哪些命令、参数是什么。
2. **错误不是结构化的** —— Agent 得从 stderr 文本解析原因,幻觉风险高。
3. **policy 不可见** —— Agent 不知道 sandbox 边界,被拒不知为何。
4. **批量调用低效** —— 每条命令 fork 一个进程,启动开销大。

### 2.2 CLI 子命令设计

在现有 `ash -c` 之上,新增**显式的 Agent 模式子命令**(`ash agent ...`),映射三个元 tool:

```bash
# (1) 拉取 tool catalog —— Agent 启动时调一次
ash agent describe-tools [--format json|compact]
ash agent describe-tools --filter "file,git"   # 按类别过滤,节省 context

# (2) 拉取 policy 摘要 —— Agent 启动时调一次,塞进 system prompt
ash agent describe-policy

# (3) Dry-run 探测 —— Agent 对可疑命令先 check
ash agent check "rm -rf /tmp/foo"

# (4) 执行(现有 --json 的规范化版本)
ash agent run "ls -la /sandbox" [--timeout 30] [--format json|text]

# (5) 批量执行(避免反复 fork)—— 从 stdin 读 NDJSON
ash agent run-batch --input commands.ndjson

# (6) 兼容性自检 —— 启动期可选调用
ash agent compat-check
```

**为什么用 `agent` 子命令而非新 flag**:`-c --json` 已稳定(不破坏现有用户),而 `agent` 子命令是干净命名空间,后续可加 `agent mcp-serve` / `agent replay` 等。

### 2.3 NDJSON 协议(批量 + 流式)

**输入**(stdin,每行一条):
```json
{"seq": 1, "command": "ls -la /sandbox"}
{"seq": 2, "command": "grep ERROR *.log"}
{"seq": 3, "command": "check: rm -rf /old"}
```

**输出**(stdout,每行一条,顺序保证):
```json
{"seq": 1, "status": "success", "data": {"files": [...]}, "timing": {"wall_ms": 12}}
{"seq": 2, "status": "failed", "error": {"kind": "nonzero_exit", "message": "no matches"}, "timing": {"wall_ms": 3}}
{"seq": 3, "status": "denied", "denied_reason": {"rule_id": "path-outside-sandbox", "remediation": "use /sandbox/old"}}
```

**设计要点**:
- `seq` 字段关联请求与响应(即便乱序完成也能对齐)。
- 每条独立 `status`,失败不中断后续。
- 流式输出 → Agent 可以边读边决策。

### 2.4 与 MCP 的关系

本 Plan **不实现** MCP server(留给独立 Plan),但**保证接口对齐** —— `ash agent describe-tools` 的输出格式直接映射到 MCP `tools/list` 响应,`ash agent run` 映射到 `tools/call`。未来 `ash agent mcp-serve` 就是加一层 stdio JSON-RPC 包装。

---

## 第 3 节:内置 F4 Agent loop 接口预留

### 3.1 目标

Plan 027 的 F4 chat 当前是**纯对话**。本 Plan **不实现** loop 本身(那是 Plan 029+),但要保证:

1. **Tool Registry 已为 in-process function-calling 准备好** —— 后续 Plan 直接消费。
2. **ChatSession 数据模型留好 tool_call / tool_result 字段** —— 避免 Plan 027 已持久化的对话格式破坏性迁移。
3. **权限模型想清楚** —— 内置 loop 是"用户在场"场景,权限语义跟外部 Agent 不同。

### 3.2 ChatSession 数据模型扩展(向前兼容)

Plan 027 当前的 `~/.auto-shell-ai-chat.json` 是:
```json
{"turns": [{"role": "user|assistant", "content": "..."}]}
```

本 Plan **向前兼容地**扩展(serde `default`,旧文件仍能加载):

```rust
#[derive(Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: Role,
    pub content: String,

    // 新增:tool-call 相关(默认空,旧文件兼容)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TurnMetadata>,
}

pub struct ToolCall {
    pub id: String,              // "call_abc123"
    pub tool_name: String,       // "ls" / "grep" / ...
    pub arguments: Value,        // 已解析 JSON
}

pub struct TurnMetadata {
    pub timing_ms: Option<u64>,
    pub tokens: Option<TokenUsage>,
}
```

### 3.3 内置 Agent loop 的调用约定(接口预留)

定义 `ToolExecuting` trait,作为"未来 F4 agent loop"与"Tool Registry"的契约。**本 Plan 只定义 trait,不实现 loop**:

```rust
// ash-core/src/tool/agent_loop.rs —— 仅 trait,无实现

pub trait ToolExecuting {
    fn execute_tool_call(
        &self,
        tool_name: &str,
        arguments: &Value,
        ctx: &ToolContext,
    ) -> ToolResult;

    /// 把整个 catalog 序列化成 LLM provider 期望的格式
    fn export_for_provider(&self, provider: LlmProvider) -> Vec<ProviderToolSpec>;
}

pub enum LlmProvider {
    Anthropic,    // tool_use 格式
    OpenAI,       // function-calling 格式
    Mcp,          // MCP tools/list 格式
    Generic,      // 通用 JSON Schema
}
```

### 3.4 内置 loop 的权限语义(关键差异)

**这是本 Plan 要明确的最重要一点**:内置 F4 loop 和外部 Agent 的权限模型**不同**,因为用户在场:

| 维度 | 外部 Agent(Claude Code 等) | 内置 F4 loop(用户在场) |
|---|---|---|
| 默认 policy | 严格(sandbox + read-only + no-network) | **沿用用户当前 Shell 的 policy**(可能更宽松) |
| Denied 时 | 直接返回 denied,Agent 自行决策 | **可降级为交互确认**:"允许执行 `rm -rf /old`? (y/n)" |
| 网络访问 | 默认禁用 | 沿用用户 Shell 设置 |
| 危险命令 | 永远拒 | 可配置:拒 / 确认 / 放行 |

为此,`ToolContext` 增加 `confirmation_mode` 字段:

```rust
pub enum ConfirmationMode {
    None,                  // 外部 Agent 默认:静默按 policy 执行/Denied
    Interactive,           // 内置 loop 默认:Dangerous 操作弹确认
    AlwaysConfirm,         // 用户显式 paranoid 模式:每步都问
}
```

`ConfirmationMode::Interactive` 下,`Denied` 会被翻译成"等用户确认",由 REPL 层处理。**交互层实现不在本 Plan**(留给 Plan 029),但 `ToolContext` 已把这个维度建模进去。

### 3.5 明确不在本 Plan 的部分

- ❌ Agent loop 本身(轮询 LLM → tool call → 执行 → 回填)
- ❌ 确认对话框 UI 实现
- ❌ 多步规划 / 反思链
- ❌ markdown 渲染
- ❌ 流式 tool_call 增量解析

留给 Plan 029+。本 Plan 只保证**地基就位**:Registry、Context、数据模型、权限维度。

---

## 第 4 节:结构化输出与错误模型

### 4.1 现状与缺口

Plan 007 的 `--json` 已存在,但存在三个 Agent 场景缺口:

1. **输出 shape 不稳定** —— 不同命令返回的 JSON 字段不一致。
2. **错误和成功混在同一 JSON** —— Agent 靠 `exit_code` 判别,易幻觉。
3. **Atom 的 18 种语义类型没暴露给 Agent** —— `ls` 返回 `FileList`、`ps` 返回 `ProcessList`,这些语义信息对 Agent 决策极有价值,但目前没暴露。

### 4.2 统一输出信封(Envelope)

所有 `ash agent run` / `ash agent run-batch` 的成功输出,都包成下面这个信封:

```json
{
  "schema_version": "1",
  "status": "success",
  "data": {
    "kind": "file_list",
    "atom_type": "FileList",
    "value": [ /* 命令特定的结构化数据 */ ],
    "pipeline_hint": "可接 filter/sort/select 等 DSL 操作"
  },
  "diagnostics": [],
  "timing": {"wall_ms": 12, "user_ms": 5, "sys_ms": 3},
  "command_echo": "ls -la /sandbox"
}
```

| 字段 | 作用 |
|---|---|
| `schema_version` | 允许未来不破坏地演进(旧 Agent 看到 `"2"` 知道要降级) |
| `status` | `"success"` / `"failed"` / `"denied"` / `"partial"`(顶层判别) |
| `data.kind` | snake_case 语义标签(如 `file_list`、`process_list`、`table`、`record`、`text`) |
| `data.atom_type` | 对应 Atom 系统的 18 种类型 |
| `data.pipeline_hint` | 提示 Agent 这个输出可以接哪些 DSL 操作 |
| `diagnostics` | 非致命警告 |
| `command_echo` | 回显执行的命令 |

### 4.3 Atom 语义类型到 JSON 的映射

把 `ash-core/src/pipeline/atom.rs` 的 18 种 `AtomType` 暴露成 `kind` 标签,并定义每种的标准 JSON shape:

| AtomType | `kind` | JSON shape 示例 |
|---|---|---|
| FileList | `file_list` | `[{"name":"a.txt","size":1024,"modified":"...","type":"file"}]` |
| FileEntry | `file_entry` | `{"name":"a.txt","size":...}` (单对象) |
| ProcessList | `process_list` | `[{"pid":123,"name":"ash","cpu":1.2,"mem":"10M"}]` |
| Table | `table` | `{"columns":["a","b"],"rows":[[1,2],[3,4]]}` |
| Record | `record` | `{"field":"value",...}` |
| Text | `text` | `{"content":"..."}` (大文本时附 truncation 信息) |
| Path | `path` | `{"path":"/sandbox/x","exists":true,"type":"file"}` |
| Empty | `empty` | `{}` |

**Agent 拿到 `kind` 后的决策路径**:
- 看到 `file_list` → 知道可以接 `filter .size > 1k | sort .name`
- 看到 `table` → 知道可以接 `select col1,col2` 或转 CSV
- 看到 `text` → 知道只能接 `grep` / `head` 等文本命令

### 4.4 错误模型统一

第 2 节已给 CLI 的错误 schema。这里**统一到 ToolResult**,所有路径(CLI、内置 loop、Registry 内部)共用:

```rust
// ash-core/src/tool/error.rs —— 单一错误源

#[derive(Serialize, Deserialize)]
pub struct ToolError {
    pub kind: ErrorKind,
    pub message: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub remediation: Option<String>,
    pub stderr_excerpt: Option<String>,
}

pub enum ErrorKind {
    NonzeroExit,         // 命令正常执行但返回非零
    NotFound,            // 命令不存在 / 文件不存在
    PermissionDenied,    // OS 权限(非 sandbox)
    InvalidArgs,         // 参数解析失败
    Timeout,             // 超时
    SandboxViolation,    // 命中 SecurityPolicy
    ParseError,          // 输出解析失败(如 from_json 收到非法 JSON)
    Internal,            // ash 内部 bug
}
```

`ErrorKind` 是**枚举不是字符串** —— Agent 恢复策略直接 match 枚举,不解析自由文本。

### 4.5 `--json` 的兼容性策略

现有 `ash -c "..." --json`(Plan 007)的输出格式**保持不变**(已有用户依赖)。新信封只对**新接口**生效:

| 接口 | 输出格式 |
|---|---|
| `ash -c "..." --json`(现有) | 旧格式(保持兼容) |
| `ash agent run ...`(新) | 新信封 |
| `ash agent run-batch`(新) | 新信封 + NDJSON |
| 内置 loop(未来) | 新信封(in-memory) |

旧格式可在下个 major 版本标 deprecated,本 Plan 不强制迁移。

### 4.6 Truncation 策略(大输出)

```json
{
  "data": {
    "kind": "text",
    "atom_type": "Text",
    "value": {"content": "前 10000 字符..."},
    "truncation": {
      "truncated": true,
      "original_bytes": 1048576,
      "returned_bytes": 10000,
      "resume_hint": "用 `head -c 10000 --skip 10000 file.log` 获取下一段"
    }
  }
}
```

`resume_hint` 给 Agent **机器可读的"继续读"指令**,避免瞎猜偏移。

### 4.7 默认安全限额

为防止 Agent 失控(如 `cat` 一个 10GB 日志),`ToolContext` 加全局限额:

```rust
pub struct OutputLimits {
    pub max_output_bytes: usize,    // 默认 1MB
    pub max_command_seconds: u64,   // 默认 60s
    pub max_recursion_depth: u32,   // 默认 8(AutoLang 脚本递归)
}
```

超限时返回 `ErrorKind::Timeout` 或带 truncation 的成功响应。

---

## 第 5 节:跨平台一致性 + bash 兼容测试

### 5.1 为什么这是 Agent 场景的核心

Agent 的痛点是**跨平台行为不确定**:同一个 `ls -la` 在 Linux/macOS/Windows 行为各异。ASH 作为"替代 bash/pwsh 的统一执行层",核心承诺是:**同一个命令在三个平台上行为一致**。

但"完全一致"不现实。本节定义**哪些必须一致、哪些差异允许**,并用 CI 测试矩阵固化。

### 5.2 一致性的三层承诺

#### 第一层:命令存在性(必须一致)

30+ 核心命令在三平台都必须存在、可调用。零容忍:

```
ls cd pwd cat cp mv rm mkdir touch ln find grep wc head tail sort uniq cut tr
echo printf date sleep which stat du file tee column diff
http_get http_post url_encode
from_json to_json from_csv to_csv from_toml to_toml from_yaml to_yaml from_xml to_xml
```

#### 第二层:flag 行为(语义一致)

| Flag | 承诺 |
|---|---|
| `-l`(ls) | 长格式,字段顺序统一(name/size/modified/permissions) |
| `-a`(ls) | 显示隐藏文件(`.` 开头 或 hidden 属性) |
| `-r`/`-f`(rm) | 递归删除 / 强制 |
| `-r`(grep) | 递归搜索 |
| `-n`(head/tail) | 行数(字节用 `-c`) |
| `--name`(find) | 按名称匹配(glob 语义) |
| `--` | 终止选项解析(所有命令统一) |

#### 第三层:已知差异(显式文档化)

| 差异点 | Linux/macOS | Windows |
|---|---|---|
| 路径分隔符 | `/` | `/`(ash 内部统一,转换 native) |
| 行结尾 | `\n` | 输出统一 `\n` |
| 权限位 | rwx 三元组 | 显示为 `rw-`(无 x 位) |
| 符号链接 | 完整支持 | 开发者模式时支持,否则降级为复制 |
| `/tmp` 路径 | `/tmp` | 解析为 `%TEMP%` |
| `ps` 字段 | 完整 Unix 字段 | 仅 pid/name/cpu/mem 子集 |

**核心原则**:ash 内部统一用 POSIX 风格路径(`/`、`/tmp`、`/sandbox`),调 OS 时转换。Agent 因此可写**平台无关**命令。

### 5.3 测试矩阵设计

新增 `tests/bash_compat/`、`tests/cross_platform/`、`tests/agent_contract/`:

```
tests/
├── bash_compat/             # 对照 bash 的行为
│   ├── ls_test.at
│   ├── rm_test.at
│   └── ...
├── cross_platform/          # 同一 .at 在三平台跑
│   ├── path_handling.at
│   ├── line_endings.at
│   └── ...
└── agent_contract/          # Agent 契约测试(新信封)
    ├── envelope_test.at
    ├── error_kinds.at
    └── truncation.at
```

**测试形式**:用 ash 自己的 `.at` 脚本写断言(吃自己狗粮):

```ash
# tests/bash_compat/ls_test.at
result = system("ls -la /sandbox", format="json")
assert result.status == "success"
assert result.data.kind == "file_list"
assert result.data.value[0].has_key("size")
```

### 5.4 CI 矩阵

GitHub Actions 三平台 matrix:

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, macos-latest, windows-latest]
steps:
  - run: cargo test --test bash_compat
  - run: cargo test --test cross_platform
  - run: cargo test --test agent_contract
```

### 5.5 `docs/compat.md`

新建**Agent 可读**的兼容性文档:

- 命令清单(30+ 核心命令的存在性)
- flag 矩阵(哪些 flag 行为一致)
- 已知差异表(第三层)
- 等价命令映射(如 bash 的 `find -exec` → ash 的 `find | each`)

这份文档**也是给 AI 训练用的语料** —— 未来可被 Agent 检索或塞进 system prompt。

### 5.6 工具:compat-check

新增 `ash agent compat-check`,跑一次自检输出当前平台的兼容性状态:

```
$ ash agent compat-check
✓ 30/30 core commands present
✓ ls -la: returns file_list (size field present)
✓ rm -rf: recursive delete works
⚠ symlink support: limited (Windows developer mode required)
✗ /dev/null: not supported on Windows (use ash's null device)
```

Agent 启动时可调一次,把结果纳入决策。

---

## 第 6 节:实施里程碑、依赖与风险

### 6.1 里程碑分解

本 Plan 拆成 **4 个递进里程碑**,每个能独立交付、独立验证:

#### M1 — Tool Registry 骨架 + 桥接(基础就位)

**目标**:80 个命令自动获得最小 Tool 描述,可被 `ash agent describe-tools` 拉取。

**交付物**:
- `ash-core/src/tool/` 新模块(`mod.rs` / `schema.rs` / `catalog.rs` / `bridge.rs` / `agent_loop.rs` / `error.rs`)
- `Tool` / `ToolRegistry` / `ToolContext` / `ToolResult` 类型定义
- `CommandToolBridge` —— 让现有 `Command` trait 自动满足 `Tool`
- `ash agent describe-tools [--filter]` CLI 子命令

**验证**:80 个命令的 catalog 能导出为合法 JSON,每个含 name/description/最小 schema。

**规模估计**:中等(新增 ~800 行,无现有代码破坏)。

#### M2 — CLI Agent 接口 + 结构化输出

**目标**:外部 Agent 能完整调用 ash,拿到稳定信封输出。

**交付物**:
- `ash agent run [--timeout] [--format]`
- `ash agent check`(dry-run policy 探测)
- `ash agent describe-policy`
- 第 4 节定义的输出信封 + Atom→kind 映射
- 统一的 `ToolError` / `ErrorKind`
- Truncation 策略 + 全局限额

**依赖**:M1。

**验证**:用伪 Agent 跑端到端流程(describe-tools → describe-policy → check → run),所有响应符合 schema。

**规模估计**:中等偏大(新增 ~1200 行,改 `Shell::execute_for_agent` 路径)。

#### M3 — 批量调用 + NDJSON

**目标**:支持高效批量执行,减少 fork 开销。

**交付物**:
- `ash agent run-batch --input commands.ndjson`
- NDJSON 流式输出
- `seq` 关联请求/响应

**依赖**:M2。

**验证**:100 条命令批量执行,流式输出顺序正确、失败隔离。

**规模估计**:小(~400 行)。

#### M4 — 跨平台 + bash 兼容测试

**目标**:把"跨平台一致"从口号变成 CI 守护。

**交付物**:
- `tests/bash_compat/`(30+ 命令的行为测试)
- `tests/cross_platform/`(三平台一致性)
- `tests/agent_contract/`(信封契约)
- `docs/compat.md`(差异文档)
- `ash agent compat-check`
- GitHub Actions 三平台 matrix

**依赖**:M2(测试需要信封输出)。

**验证**:三平台 CI 全绿;现有命令的破坏性改动会被测试拦住。

**规模估计**:中等(测试代码 ~1500 行,文档 ~500 行,CI 配置)。

### 6.2 与现有代码的衔接点

| 现有模块 | 改动类型 |
|---|---|
| `ash-core/src/pipeline/atom.rs` | 暴露 AtomType 到 JSON kind(只增不改) |
| `ash-core/src/security.rs` | 复用 `SecurityPolicy`,加 `summarize()` 方法 |
| `ash/auto-shell/src/shell.rs` `execute_for_agent` | 包装为 `ToolResult`(新增路径,旧路径保留) |
| `ash/auto-shell/src/frontend/ai.rs` `ChatSession` | 加向前兼容字段(serde default) |
| `ash/auto-shell/src/cmd.rs` `Command` trait | 不动,通过 `CommandToolBridge` 桥接 |
| `ash/auto-shell/src/main.rs` | 新增 `agent` 子命令分发 |

**核心原则**:所有改动都是**加法**。旧接口(`-c --json`)继续工作。

### 6.3 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| **80 命令的 schema 自动推导质量参差** | Agent 拿到的 schema 不准,影响调用成功率 | M1 只承诺"最小 schema";高价值命令在 M4 后逐个精修(独立 Plan) |
| **三平台 CI 维护成本** | Windows runner 慢、flaky | 只对核心 30 命令做三平台测试,长尾命令只在 Linux 测 |
| **NDJSON 流式的边界情况** | 长输出 / 部分失败 / 超时 | M3 单独验证,加 chaos 测试(随机注入失败) |
| **`describe-policy` 暴露敏感信息** | Agent 把 policy 塞 prompt → 可能进日志 | policy 摘要不包含具体路径,只含能力位(read/write/network) |
| **向后兼容包袱** | 双格式(旧 `--json` + 新信封)长期并存 | 明确 deprecation 时间表(v0.7 标 deprecated,v1.0 移除) |
| **bash 兼容测试维护** | bash 行为本身有版本差异 | 测试不对照真实 bash,而是对照"文档化的承诺" |

### 6.4 非目标(明确排除)

- ❌ 完整 bash parser(L3 语法,留给 AI 转写)
- ❌ MCP server 实现(独立 Plan,本 Plan 只保证接口对齐)
- ❌ 内置 F4 agent loop 实现(Plan 029+)
- ❌ Polars / dataframe / lazy 框架(#5 独立 spec)
- ❌ SmartCommand(#2 独立 spec,依赖本 Plan 的 Tool Registry)
- ❌ 插件分发系统(独立 spec)
- ❌ 第三方命令包注册(独立 spec)

### 6.5 成功指标

本 Plan 完成后,可量化的成功标志:

1. **80 个命令**可通过 `ash agent describe-tools` 拉到合法 JSON Schema。
2. **三平台 CI** 跑 bash_compat / cross_platform / agent_contract 全绿。
3. **一个外部 Agent demo**(用伪 Claude Code 跑端到端)能:启动期拉 catalog+policy → 规划期 check → 执行期 run,全程零文本解析。
4. **`docs/compat.md`** 覆盖 30+ 核心命令 + 第三层差异表。
5. **现有 `ash -c --json`** 行为不变(回归测试通过)。

---

## 附录:全局方向 roadmap(本 Plan 之外的轻量规划)

### 全局方向图

```
                    ┌──────────────────────────────────────┐
                    │  终极愿景:AI 时代替代 bash/pwsh 的    │
                    │  跨平台、安全、结构化的新一代 shell    │
                    └──────────────────────────────────────┘
                                     ▲
            ┌────────────────────────┼─────────────────────────┐
            │                        │                         │
   ┌────────┴────────┐    ┌──────────┴──────────┐    ┌────────┴─────────┐
   │ Agent 执行引擎  │    │  AutoCoder Agent TUI │    │  日常终端体验     │
   │ (Plan 028,M1-4)│    │  (D,栈顶产品)        │    │  (对标 Warp)     │
   └────────┬────────┘    └──────────┬──────────┘    └────────┬─────────┘
            │                        │                        │
   ┌────────┴───────┐                │               ┌────────┴─────────┐
   │ #5 数据处理    │                │               │ B 智能补全系统    │
   │ (Polars-like)  │                │               │ (Plan 021 续)    │
   └────────┬───────┘                │               └──────────────────┘
            │                        │
   ┌────────┴───────┐                │
   │ #2 SmartCommand│────────────────┤  (SmartCommand = 给 AI 的本地 tool)
   │ (本地小模型)   │                │
   └────────┬───────┘                │
            │                        │
   ┌────────┴───────┐                │
   │ #3 脚本实例库  │                │
   │ (AutoLang)     │                │
   └────────────────┘                │
                                     │
   平行地基(任何时候都可并行推进):  │
   ┌────────────────┐ ┌──────────────┴───┐ ┌──────────────────┐
   │ A 分发+文档    │ │ C 插件/扩展生态   │ │ #1 AI 能力增强    │
   │ (采用前置)     │ │ (长期护城河)      │ │ (Warp 式 chat)   │
   └────────────────┘ └──────────────────┘ └──────────────────┘
```

### 各方向轻量规划

#### #1 — AI 能力增强(对标 Warp)
- F4 chat 加 tool-calling(Plan 029,依赖本 Plan 完成)
- 命令建议增强:F3 从"一条命令"升级到"一条 pipeline + 解释"
- 自然语言→AutoLang 脚本翻译(NL→ash pipeline,不走 bash)
- markdown 渲染(F4 chat 里的代码块/表格)
- 上下文感知:chat 知道当前 cwd / 最近命令 / pipeline 状态
- **不在范围**:Warp 的 Blocks 交互(对 Agent 无价值)
- **依赖**:本 Plan(Tool Registry + 内置 loop 接口)

#### #2 — SmartCommand
- 介于 bash 命令和 Skill 之间的轻量扩展,本地小模型(如 9B Ornith)就能跑
- 典型例子:`git.ship`(智能提交)、`deploy.safe`(安全部署)、`refactor.rename`(跨文件重命名)
- 设计骨架:`smart/` 目录,每个 `.smart.yaml` 定义一个 SmartCommand(NLU 提示词 + AutoLang 执行体 + tool schema)
- 注册进 Tool Registry,对 Agent 透明(就是另一个 tool)
- **依赖**:本 Plan(Tool Registry)+ #3(AutoLang 实例)+ 本地小模型集成

#### #3 — AutoLang 脚本实例库
- `examples/` 扩充到 30+ 脚本,覆盖 bash 常见场景
- 每个例子配 README,对照 bash 版本展示差异
- 建立 `bash-to-ash` 速查表
- **依赖**:基本独立。靠 #5 才能在数据场景体现碾压

#### #5 — 统一数据处理框架(独立 spec)
- 在现有 Atom pipeline 上加 lazy 求值(查询计划 + 谓词下推)
- Polars-like dataframe 抽象(复用 Atom 的 18 种语义类型,不另起炉灶)
- 流式处理(大于内存的数据集)
- **依赖**:本 Plan(把 Atom 类型暴露给 Agent)+ Plan 024 DSL

#### A — 分发 + 文档(采用前置条件)**[紧急]**
- **全项目目前没有任何 README**
- 交付物:根 README、`brew`/`cargo`/`winget` 安装、quickstart、bash-to-ash
- **优先级**:立即启动,与本 Plan 并行

#### B — 智能补全系统(Plan 021 续)
- 命令/flag/路径补全(静态)
- 动态补全(git 分支名、docker 容器名、host 列表)
- AI 建议下一条命令(基于历史)
- 补全源注册机制(让第三方命令提供自己的补全)
- **依赖**:基本独立,AI 建议部分依赖 #1

#### C — 插件/扩展生态
- 插件包格式(`ash-plugin.toml` + 命令/SmartCommand/补全源)
- 中央 registry(类似 cargo crates.io)
- `ash plugin install <name>`
- 与 Tool Registry 集成(插件自动注册成 tool)
- **优先级**:中长期,在 #2 稳定后

#### D — AutoCoder TUI Agent 应用
- roadmap 里的栈顶产品。ratatui 全屏 Agent UI,ash 作为执行引擎
- 当前 `ash-gui-bin` 只是 scaffold
- ash 的 Tool Registry 直接成为 AutoCoder 的 tool 源
- **优先级**:中长期

### 推荐的总优先级排序

| 顺序 | 方向 | 时机 |
|---|---|---|
| **现在** | **本 Plan(Agent 执行引擎,M1-M4)** | 主线 |
| **现在(并行)** | **A 分发+文档** | 不阻塞主线,采用率前置 |
| **现在(并行)** | **#3 脚本实例库(小批)** | 不阻塞主线,为后续积累素材 |
| **本 Plan 后** | **#5 数据处理 spec** | Agent 弹药,与 #2 并行设计 |
| **本 Plan 后** | **#2 SmartCommand spec** | 依赖本 Plan + #5 |
| **#2 后** | **#1 AI 能力增强(F4 tool-calling 等)** | 依赖本 Plan 接口 |
| **#1 后** | **B 智能补全 + D AutoCoder** | 体验与栈顶产品 |
| **长期** | **C 插件生态** | 护城河 |

---

## 附录 B:M1+M2 实现偏差记录(2026-07-18)

M1+M2 实现过程中发现原设计与实际代码的 3 处偏差,均已就地修正。记录于此供 M3/M4 及后续 Plan 参考。

### 偏差 1:`ToolData::Atom(AtomPipeline)` 不可行

**原设计**(第 1.3 节):`ToolData` 有 `Atom(AtomPipeline)` 变体,`ToolResult` derive `Clone`。

**实际情况**:`AtomPipeline`(`ash-core/src/pipeline/atom_pipeline.rs`)**不实现 `Clone`**(deliberately —— 它持有流式资源)。而 `ToolResult` 需要 `Clone`(信封序列化要克隆)。两者冲突。

**修正**:
- 从 `ToolData` 移除 `Atom(AtomPipeline)` 变体(只保留 `Json(Value)` / `Text(String)` / `Empty`)。
- 新增 `ToolData::from_atom_pipeline(&AtomPipeline) -> Self` 方法,在调用点把 AtomPipeline 转成 JSON 再包进 `ToolData::Json`。
- 新增顶层函数 `atom_pipeline_to_json(&AtomPipeline) -> Value`。
- 未来内置 F4 loop 若需要 in-process 传递 AtomPipeline,可在 Plan 029 重新设计(可能用 `Arc<AtomPipeline>` 或改 ToolResult 不 derive Clone)。

### 偏差 2:`auto_val::Value` ≠ `serde_json::Value`

**原设计**:假设 `Atom.value`(类型 `auto_val::Value`)可直接当 `serde_json::Value` 用。

**实际情况**:`auto_val::Value`(`auto-lang/crates/auto-val/src/value.rs`)是 AutoLang 语言的值类型,有 50+ 变体(含 `Lambda`、`Closure`、`Widget`、`Model`、`Future` 等语言级概念),与 `serde_json::Value` 是**完全不同的两个类型**,无内置转换。

**修正**:新增 `ash-core/src/tool/value_convert.rs` 模块,提供 `auto_value_to_json(&auto_val::Value) -> serde_json::Value`:
- 数值/布尔/字符串/数组/对象 → 对应 JSON
- `Nil/Null/None/Void` → JSON null
- `Some(x)/Ok(x)` → unwrap 递归
- `Future` → `{"pending": "future not resolved"}`
- `Error/Err(msg)` → `{"error": msg}`
- 其余语言级变体(`Lambda/Closure/Widget/...`)→ 降级为 `"<auto_val:TypeName>"` 描述字符串

**影响**:这是 ash 项目"自有值系统 vs JSON 世界"的根本张力点。未来任何需要把 Atom 数据暴露给外部(信封、MCP、日志)的地方都要经过此转换。

### 偏差 3:`Command` trait 无 `Send + Sync` bound

**原设计**(第 1.5 节):`DynamicCommandTool` 持有 `Arc<dyn Command>`,实现 `Tool`(要求 `Send + Sync`)。

**实际情况**:`Command` trait(`auto-shell/src/cmd.rs`)**没有 `Send + Sync` supertrait bound**,且 79 个现有 `impl Command` 不保证都满足。所以 `Arc<dyn Command>` 不能直接放进需要 `Send + Sync` 的 `Tool` 实现里。

**修正**:`DynamicCommandTool` **不持有** `Arc<dyn Command>`。它在构造时(`from_command(&dyn Command)`)只从 `Command::signature()` **拷贝出** name/description,然后用 `derive_schema_from_signature()` 算出 schema。之后这个 Tool 就是个纯数据壳(name + description + parameters,都是 `Send + Sync + Clone`)。

**后果**:`DynamicCommandTool::invoke()` 返回 `Failed(Internal, "...use ash agent run")`——它是**纯内省**(给 catalog/describe-tools 用),不负责执行。真正的执行在 `ash agent run` 路径(M2.4),那里有 `&mut Shell` 可直接调 `Command::run`。这其实跟原设计的"bridge 的 invoke 是 shell-less,真正执行在 agent run"方向一致,只是更彻底——连 Command 引用都不存。

### 经验教训(给 M3/M4)

1. **勘探要到位**:写 plan 前的代码勘探漏了 `AtomPipeline` 不 Clone、`auto_val::Value` 类型、`Command` 无 Send+Sync 这三个关键事实。M3/M4 展开 plan 前要补做针对性勘探。
2. **`ash-core` 测试从 `ash-core/` 目录跑**:`cargo test -p ash-core` 在 workspace 外会失败(它不是 workspace member,dev-deps 解析不到)。必须 `cd ash-core && cargo test`。
3. **`auto-lang` 是活跃开发仓库**:实现期间被外部改动打断了两次。M3/M4 期间若再遇,优先让用户先稳定 auto-lang。

---

## 参考

- `docs/roadmap.md` —— 项目战略 roadmap,护城河定位
- `plans/007-ms1-agent-invocable.md` —— MS1 `--json` agent 接口的原始设计
- `plans/008-ms2a-security-policy.md` / `plans/009-ms2b-path-sandbox.md` —— MS2 沙箱基础
- `plans/024-ash-structured-pipeline-dsl.md` —— Plan 024 Atom pipeline DSL
- `plans/027-ash-ai-chat-mode.md` —— Plan 027 F4 chat v1(本 Plan 的内置 loop 接口预留服务于它的升级)
- `ash-core/src/pipeline/atom.rs` —— 18 种 Atom 语义类型
- `ash-core/src/security.rs` —— SecurityPolicy
