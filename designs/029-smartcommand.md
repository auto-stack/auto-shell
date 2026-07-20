# Plan 029: ASH SmartCommand 设计

> **日期**: 2026-07-20
> **状态**: 设计中(待评审)
> **战略驱动**: 把"几步确定性操作 + 一步 AI 判断"的真实任务封装成轻量命令,本地小模型负责 NLU 和 AI 步骤
> **范围**: auto-ai 改造(前置)+ auto-shell SmartCommand 引擎 + git.finish-worktree 首个实例
> **跨仓库**: auto-shell + auto-ai
> **预估**: M1-M4 共约 5-7 周(详见 §8.1)

---

## 愿景

> **SmartCommand 是"混合执行"的轻量扩展**:把"几步确定性操作 + 一步 AI 判断"封装成一条命令,本地小模型(经 Ollama provider)负责自然语言参数解析和 AI 步骤,确定性步骤走现有 Shell。对 AI Agent 透明(是 Tool Registry 里的一项,跟 `ls`/`grep` 一样)。

### 核心洞察:为什么 SmartCommand 独特

真实任务通常是混合的——`finish-worktree` 4 步里只有 1 步(commit message 生成)真需要 AI。三种现有方案都不合适:

| 方案 | 问题 |
|---|---|
| 普通 ash 命令 | 失去 AI 生成 commit message 的便利 |
| 调云端大模型 Agent | 为 4 步 git 启动 ReAct 循环,算力浪费、网络依赖 |
| bash function / alias | 没法接 AI、没法接 Tool Registry |

SmartCommand 正好填这个空:**本地小模型 + 确定性脚本 + Tool 透明注册**。

### 范围内 / 范围外

| 范畴 | 本 Plan 包含 | 不包含 |
|---|---|---|
| **auto-ai 改造** | OllamaProvider + preferred_provider 链路补全 + SmartCommandRole | 新 provider 协议、daemon 架构调整 |
| **SmartCommand 格式** | `command.at` schema + 加载器 + body.ash 执行 | 新脚本语法(用现有 AutoLang) |
| **CLI** | `ash smart <name>` + `ash smart "<nl>"` + 通过 `ash agent run` 调用 | SmartCommand 编辑器/REPL 集成 |
| **实例** | `git.finish-worktree` 作为完整首个实例 | 其他 SmartCommand(后续积累) |
| **NLU** | 基于 SmartCommandRole 的参数解析 | 复杂多轮对话 |
| **skill.md 转译** | sidecar `.md` 直接用 | AutoDown→Markdown(探勘证实不存在,不做) |

### 三条核心架构决策(已在 brainstorming 阶段确认)

1. **provider 路径 = A**:先补全 auto-ai 的 `preferred_provider` 链路(5 个文件),SmartCommandRole 用 `preferred_provider = "ollama"`。
2. **三位一体**:每个 SmartCommand 目录同时是 AutoLang 配置 + Tool + Skill(通过 sidecar 文件实现,不搞 codegen)。
3. **全覆盖范围**:spec 跨 auto-shell + auto-ai 两个仓库,含 OllamaProvider 实现。

---

## 架构总图

```
┌─────────────────────────────────────────────────────────────────────┐
│                    调用方式(三种统一入口)                            │
│  ash smart git.finish-worktree --push                                │
│  ash smart "finish this worktree and push"  ← 自然语言(NLU 解析)     │
│  ash agent run 'git.finish-worktree' {...}   ← 外部 Agent tool call  │
└──────────────────────────┬──────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────────┐
│                  SmartCommand Engine(auto-shell 新增)                │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐   │
│  │ smart/ 加载器    │  │ NLU 参数解析     │  │ 执行体调度       │   │
│  │ 扫 smart/*/*.at  │→ │ 本地小模型把 NL  │→ │ AutoLang body    │   │
│  │ → SmartCommandSpec│ │ → 结构化 args    │  │ system() 调用    │   │
│  └──────────────────┘  └──────────────────┘  └────────┬─────────┘   │
│         │                                              │             │
│         │ 注册为                                        │ 受约束       │
│         ▼                                              ▼             │
│  ┌────────────────────────────┐         ┌────────────────────────┐  │
│  │ Tool Registry(Plan 028)   │         │ SecurityPolicy(MS2)   │  │
│  │ 每个 SmartCommand = 1 Tool │         │ git 操作自动受限        │  │
│  └────────────────────────────┘         └────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
                           │
                           ▼ (AI 步骤时)
┌─────────────────────────────────────────────────────────────────────┐
│              auto-ai 经 aaid daemon(前置改造)                       │
│  ┌──────────────────┐  ┌──────────────────────────────────────────┐ │
│  │ OllamaProvider   │  │ preferred_provider 链路补全              │ │
│  │ (新增,OpenAI 兼容)│  │ RoleConfig + ConfigRole +               │ │
│  │ 本地模型推理      │  │ CompletionRequest + agent.rs + server.rs│ │
│  └──────────────────┘  └──────────────────────────────────────────┘ │
│                                                                       │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │ SmartCommandRole(auto-ai-agent 内置 Role)                      │ │
│  │ model_tier=min, preferred_provider=ollama, allowed_tools=smart │ │
│  │ system_prompt: "你是 SmartCommand 助手,以下是可用命令: <skills>"│ │
│  └────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

### 三大设计支柱

1. **三位一体文件结构**(沿用 roles 模式):每个 SmartCommand 是一个目录,含 `command.at`(配置)+ `body.ash`(执行体 sidecar)+ `skill.md`(文档 sidecar)。
2. **三层执行模型**:body.ash 里可以有三类操作 —— 确定性 Shell 调用、本地 AI 调用、交互确认。
3. **Tool Registry 透明注册**(Plan 028 协同):每个 SmartCommand 自动注册进 ToolRegistry,外部 Agent 调用跟 `ls` 一样。

---

## 第 1 节:auto-ai 改造(OllamaProvider + preferred_provider 链路补全)

### 1.1 改造范围(基于代码勘探的精确事实)

auto-ai 有 5 个改造点(preferred_provider 链路),加上 1 个新增(OllamaProvider)。全部在 `auto-ai` 仓库,跨 3 个 crate:

| # | 文件 | 改动 | crate |
|---|---|---|---|
| 1 | `auto-ai-daemon/src/provider/ollama.rs` | **新增** `OllamaProvider` | daemon |
| 2 | `auto-ai-daemon/src/provider/mod.rs:78` | 加 `"ollama" =>` 分支 | daemon |
| 3 | `ai-config/src/provider.rs` | `ProviderConfig` 文档说明 ollama kind(已预留) | ai-config |
| 4 | `auto-ai-agent/src/config/role_config.rs` | `RoleConfig` 加 `preferred_provider` 字段 + parse/serialize | agent |
| 5 | `auto-ai-agent/src/config/role_config.rs:328` | `ConfigRole` 覆盖 `preferred_provider()` | agent |
| 6 | `ai-config/src/wire.rs:156` | `CompletionRequest` 加 `preferred_provider: Option<String>` | ai-config |
| 7 | `auto-ai-agent/src/agent.rs:535` | `build_request` 读 role.preferred_provider 填进 req | agent |
| 8 | `auto-ai-daemon/src/server.rs:133` | 读 `req.preferred_provider`,改调 `TierRouter::resolve(tier, pref)` | daemon |
| 9 | `auto-ai-agent/src/builtin_roles/smart_command.rs` | **新增** SmartCommandRole 内置 Role | agent |

### 1.2 OllamaProvider(薄包装)

Ollama 暴露 OpenAI 兼容 API(`/v1/chat/completions`),所以 `OllamaProvider` 非常薄——组合 `OpenAiProvider`:

```rust
// auto-ai-daemon/src/provider/ollama.rs
pub struct OllamaProvider {
    inner: OpenAiProvider,   // 委托给 OpenAI 兼容的 HTTP 逻辑
}

impl OllamaProvider {
    pub fn new(name: String, base_url: String, models: Vec<String>) -> Self {
        Self {
            inner: OpenAiProvider::new(
                name,
                base_url,                       // 通常 http://localhost:11434/v1
                "no-key-needed".to_string(),    // Ollama 无需 API key
                models,
            ),
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
    async fn complete_stream(&self, req: &CompletionRequest,
        on_delta: Arc<dyn Fn(String) + Send + Sync>,
        cancel: CancellationToken,
    ) -> Result<CompletionResponse, LlmError> {
        self.inner.complete_stream(req, on_delta, cancel).await
    }
}
```

**为什么薄包装而非复用**:v1 可直接复用,但薄包装为未来"本地模型管理"(拉模型、查 modelfile、`/api/show`)留干净扩展点。组合而非继承。

`provider/mod.rs` 加分支:
```rust
let provider: Arc<dyn AiProvider> = match pc.kind.as_str() {
    "anthropic" => Arc::new(AnthropicProvider::new(...)),
    "ollama" => Arc::new(OllamaProvider::new(
        name.clone(), pc.base_url.clone(), model_ids.clone(),
    )),
    "openai" | _ => Arc::new(OpenAiProvider::new(...)),
};
```

`ai-daemon.at` 配置示例:
```autolang
daemon {
    // ... 既有云端 provider 配置 ...
    ollama {
        kind : ollama
        base_url : "http://localhost:11434/v1"
        models : ["ornith-9b", "qwen2.5-coder:7b"]
        max_concurrency : 1   // 本地模型通常单并发
    }
}
```

### 1.3 preferred_provider 链路补全(5 个串联小改动)

核心思路:让 Role 能声明 preferred_provider,经 CompletionRequest 传到 daemon,daemon 用 `TierRouter::resolve(tier, pref)` 而非 `candidates(tier)`。

**改造 4 — RoleConfig 加字段**:
```rust
pub struct RoleConfig {
    // ... 既有字段 ...
    pub preferred_provider: Option<String>,
}
// parse_at_role 加:if let Some(p) = node.get_str("preferred_provider") { cfg.preferred_provider = Some(p.into()); }
// serialize_at_role 加对应输出
```

**改造 5 — ConfigRole 覆盖 trait 方法**:
```rust
impl Role for ConfigRole {
    fn preferred_provider(&self) -> Option<String> {
        self.cfg.preferred_provider.clone()
    }
}
```

**改造 6 — CompletionRequest 加字段**:
```rust
pub struct CompletionRequest {
    // ... 既有字段 ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_provider: Option<String>,
}
```

**改造 7 — Agent::build_request 填入**:
```rust
let req = CompletionRequest {
    // ... 既有字段 ...
    preferred_provider: self.role.preferred_provider(),
};
```

**改造 8 — daemon server 读并改用 resolve**:
```rust
let candidates = match &req.preferred_provider {
    Some(pref) => state.tier_router.resolve(tier, Some(pref.as_str())),
    None => state.tier_router.candidates(tier),
};
```

`TierRouter::resolve(tier, pref)` 已存在并已单测覆盖,直接用。

### 1.4 SmartCommandRole 定义

在 `auto-ai-agent/src/builtin_roles/` 加 `smart_command.rs`:

```rust
pub struct SmartCommandRole {
    system_prompt: String,
    allowed_tools: Vec<String>,
}

impl SmartCommandRole {
    pub fn new(system_prompt: String, allowed_tools: Vec<String>) -> Self {
        Self { system_prompt, allowed_tools }
    }
}

impl Role for SmartCommandRole {
    fn name(&self) -> &str { "smart-command" }
    fn system_prompt(&self) -> &str { &self.system_prompt }
    fn model_tier(&self) -> ModelTier { ModelTier::Min }   // 本地小模型够用
    fn preferred_provider(&self) -> Option<String> {
        Some("ollama".to_string())   // ← 整个 §1 改造的 payoff
    }
    fn temperature(&self) -> f64 { 0.1 }   // 低温度,参数解析要确定性
    fn max_turns(&self) -> usize { 3 }     // 单次解析不需要多轮
    fn allowed_tools(&self) -> Vec<String> { self.allowed_tools.clone() }
}
```

**注意**:SmartCommandRole 的 system_prompt 和 allowed_tools 是动态的(取决于加载了哪些 SmartCommand)。所以它不是纯静态内置 Role,而是由 auto-shell 的 SmartCommand 加载器实例化时传入(见 §3)。

### 1.5 兼容性

所有改动是**加法 + 默认 None**:
- 老 `RoleConfig` 无 `preferred_provider` → 解析为 `None` → 走默认 tier 路由(旧行为)
- 老 `CompletionRequest` 无 `preferred_provider` → serde default None → 旧行为
- 老 `ai-daemon.at` 无 ollama 块 → OllamaProvider 不实例化 → 旧行为

**零破坏性**。

### 1.6 验证策略

| 改造点 | 测试 |
|---|---|
| OllamaProvider | mock Ollama HTTP,验证 complete/complete_stream |
| mod.rs 分支 | `ProviderRegistry::build` 加 ollama 配置,验证 provider 类型 |
| RoleConfig 字段 | 复用 `serialize_and_reparse_role_roundtrips` 模式 |
| CompletionRequest | serde 序列化测试 |
| daemon server | mock 客户端发带 preferred_provider 的请求,验证选了 ollama |

---

## 第 2 节:`.smart.at` 格式 + 加载器

### 2.1 文件布局(沿用 roles 模式)

每个 SmartCommand 是一个**目录**,含一个配置 + N 个 sidecar:

```
~/.config/ash/smart/                    # 用户级
./smart/                                # 项目级(优先级更高)
ash/auto-shell/smart/                   # 内置(随 ash 发布,优先级最低)
└── git.finish-worktree/                # 一个 SmartCommand = 一个目录
    ├── command.at                      # 配置(必需)
    ├── body.ash                        # 执行体(必需,sidecar)
    ├── skill.md                        # Skill 文档(可选,给 SmartCommandRole 读)
    └── tests/                          # 测试用例(可选,后续 Plan)
```

**为什么用目录而非单文件**:沿用 `roles/<name>.at + <name>.soul.md` 的成熟模式。AutoLang 配置不宜嵌大段文本(`emit.rs:26-28` 明说"大段文本用 sidecar")。

### 2.2 `command.at` schema

模式严格照搬 `daemon.at` 和 `role_config.rs` 的 verified 语法:

```autolang
// smart/git.finish-worktree/command.at
smart_command {
    name        : "git.finish-worktree"
    description : "完成 worktree:生成 commit message → 提交 → merge 回主分支 → 删除 worktree → push"

    script_file : "body.ash"
    skill_file  : "skill.md"

    args : [
        {
            name        : "target"
            type        : str
            default     : "auto"
            description : "merge 回哪个分支(auto=自动检测 main/master)"
        },
        {
            name        : "push"
            type        : bool
            default     : true
            description : "是否 push 到远端"
        },
        {
            name        : "message_source"
            type        : str
            enum        : ["diff", "plan", "manual"]
            default     : "diff"
            description : "commit message 来源"
        },
        {
            name        : "plan_file"
            type        : str
            required    : false
            description : "plan 文件路径(message_source=plan 时用)"
        }
    ]

    capabilities : {
        reads_fs      : true
        writes_fs     : true
        spawns_process : true
        uses_network  : true
    }

    confirm_before : true
    timeout_sec    : 120
}
```

**顶层字段规则**:

| 字段 | 必需 | 类型 | 说明 |
|---|---|---|---|
| `name` | ✅ | str | 全局唯一,点分命名空间(`git.finish-worktree`) |
| `description` | ✅ | str | LLM 读它决定何时调用 |
| `script_file` | ✅ | str | body.ash 的文件名(相对本目录) |
| `skill_file` | ❌ | str | skill.md 的文件名 |
| `args` | ❌ | array | 参数定义,见下 |
| `capabilities` | ❌ | obj | Tool 能力位 |
| `confirm_before` | ❌ | bool | 默认 false |
| `timeout_sec` | ❌ | int | 默认 60 |

**args 每项字段**:

| 字段 | 必需 | 类型 | 说明 |
|---|---|---|---|
| `name` | ✅ | str | 参数名 |
| `type` | ✅ | enum | `str` / `bool` / `int` / `float` |
| `description` | ✅ | str | 给 LLM 的参数说明 |
| `required` | ❌ | bool | 默认 false(有 default 时自动 false) |
| `default` | ❌ | str | 默认值(字符串形式,加载器按 type 转换) |
| `enum` | ❌ | array | 枚举可选值 |

### 2.3 类型化 parse/serialize(照搬 role_config.rs 模式)

在 `auto-shell` 新增 `smart_command/config.rs`,实现 `parse_smart_command` / `serialize_smart_command`:

```rust
// ash/auto-shell/src/smart_command/config.rs
use ash_core::tool::Capabilities;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SmartCommandSpec {
    pub name: String,
    pub description: String,
    pub script_file: String,
    pub skill_file: Option<String>,
    pub args: Vec<SmartArg>,
    pub capabilities: Capabilities,
    pub confirm_before: bool,
    pub timeout_sec: u64,
    pub base_dir: PathBuf,   // 加载时填:command.at 所在目录
}

#[derive(Debug, Clone)]
pub struct SmartArg {
    pub name: String,
    pub ty: SmartArgType,
    pub description: String,
    pub required: bool,
    pub default: Option<String>,
    pub enum_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SmartArgType { Str, Bool, Int, Float }

/// 解析 command.at → SmartCommandSpec。
/// 模式照搬 auto-ai-agent/src/config/role_config.rs:101 的 parse_at_role。
pub fn parse_smart_command(content: &str, base_dir: PathBuf) -> Result<SmartCommandSpec, SmartError>;

/// 反向:SmartCommandSpec → command.at 文本(用于 `ash smart edit` 回写)。
pub fn serialize_smart_command(spec: &SmartCommandSpec) -> String;
```

估算约 150 行,跟 `role_config.rs` 同规模。

### 2.4 加载器:扫 smart/ 目录

```rust
// ash/auto-shell/src/smart_command/loader.rs
/// 扫描 smart/ 目录,加载所有 SmartCommand。
///
/// 搜索路径(优先级从高到低):
///   1. $CWD/smart/              (项目级)
///   2. ~/.config/ash/smart/     (用户级)
///   3. 内置(随二进制分发)      (优先级最低,用户可覆盖)
/// 同名时高优先级覆盖低优先级。
pub fn load_all() -> Result<Vec<SmartCommandSpec>, SmartError>;
```

### 2.5 内置 SmartCommand 的分发

M4 的 `git.finish-worktree` 随 ash 二进制分发。用 `include_dir` crate 把整个 `smart/git.finish-worktree/` 目录嵌入二进制,运行时解压到临时目录或内存解析。用户安装后零外部文件依赖,开箱即用。

### 2.6 Schema → Tool parameters_schema 推导

```rust
// ash/auto-shell/src/smart_command/schema.rs
pub fn args_to_json_schema(args: &[SmartArg]) -> Map<String, Value>;
```

这让每个 SmartCommand 自动满足 Plan 028 的 `Tool::parameters_schema()` 契约——外部 Agent 在 `describe-tools` 里能看到它。

---

## 第 3 节:SmartCommandRole + Skill 集成

### 3.1 核心问题:Role 是动态的

SmartCommandRole 的 `system_prompt` 和 `allowed_tools` 取决于"加载了哪些 SmartCommand"——运行时才知道。这跟 auto-ai 现有的静态内置 Role 不同。

**决策(方案 C)**:工厂函数 + Agent 注入。SmartCommand 加载器提供 `build_smart_role(loaded_specs) -> SmartCommandRole`,由 `ash smart` CLI 调用时构造,注入 Agent。**不进 RoleRegistry**——它不是用户可配置的静态 Role,而是 SmartCommand 系统专属的动态 Role。

```rust
// ash/auto-shell/src/smart_command/role.rs
pub fn build_smart_role(specs: &[SmartCommandSpec]) -> SmartCommandRole {
    let available = render_available_skills(specs);
    let allowed: Vec<String> = specs.iter().map(|s| s.name.clone()).collect();
    SmartCommandRole::new(build_prompt(&available), allowed)
}

fn render_available_skills(specs: &[SmartCommandSpec]) -> String {
    // 把每个 SmartCommand 渲染成 markdown,含 name/description/args schema
    // 如果有 skill.md,把它的摘要也注入
    // 这就是给 LLM 看的 "skill 列表"
}
```

### 3.2 与 auto-ai 现有 Skill 系统的关系

**不复用**(推荐):
1. auto-ai 的 Skill 是给通用 Agent 用的,扫的是全局目录;SmartCommand 的 skill.md 跟 command.at 绑定。
2. auto-ai 的 Skill 暴露成一个 `skill` 工具(too overhead);SmartCommand 要的是每个命令直接作为独立 tool。
3. 复用会让两套机制耦合,调试困难。

SmartCommand 的 skill.md 用途:注入 SmartCommandRole 的 system prompt 作为该命令的详细说明,不是独立 Tool,而是 Tool 的"长描述"。

### 3.3 NLU 参数解析流程(核心智能)

当用户输入自然语言:

```
1. 加载所有 SmartCommand spec → build_smart_role(specs)
2. 构造 Agent::new(smart_role, ai_client)
3. 把每个 SmartCommand 注册成 Agent 的 Tool
4. Agent::run(user_msg)
   → LLM(经 Ollama provider)看到 system prompt 里的 skill 列表
   → LLM 选 git.finish-worktree,填参数 {target:"main", push:true}
   → Tool::execute 被调用
   → 执行 body.ash(见 §4)
   → 返回结果给 LLM
   → LLM 看到 tool_result,生成最终回复(或终止)
5. 输出 SmartCommand 的执行信封(复用 Plan 028 的 build_envelope)
```

### 3.4 LLM 只做"选命令+填参数",不做"执行"

SmartCommandRole 的 system prompt 强约束 LLM:
- **必须**调用一个 SmartCommand 工具
- **不要**直接回答或解释
- `max_turns: 3`(选命令→执行→看结果→结束)

这把 LLM 限制在"参数解析器"角色。这是 SmartCommand 经济性的核心——**本地小模型做最小决策,确定性脚本做执行**。

### 3.5 SmartCommandRole 的 system prompt 模板

```
你是 SmartCommand 助手,负责把用户的自然语言请求解析为一次 SmartCommand 调用。

## 可用 SmartCommand
<available_skills>

## 行为规则
1. 你必须调用恰好一个 SmartCommand 工具,不要直接回答
2. 参数严格符合 schema;不确定的用 default
3. 如果用户的请求跟所有命令都不相关,返回简短说明(不超过一句)
4. 不要解释你的选择,直接调工具

## 输出
一次 tool_call,然后看 tool_result 决定是否补充说明(通常不需要)。
```

低温度(0.1)+ max_turns=3 确保它是个"快速的参数解析器"。

### 3.6 与 Plan 028 Tool Registry 的双重身份

每个 SmartCommand 有三个"被调用"出口,共用同一个执行体:

| 出口 | 路径 | 谁选参数 |
|---|---|---|
| **Plan 028 Agent CLI**(`ash agent run git.finish-worktree`) | 外部 Agent 直接填好 JSON args | 外部 Agent |
| **`ash smart` NLU**(`ash smart "finish and push"`) | SmartCommandRole(本地小模型)解析 | 本地 Ollama |
| **`ash smart` 显式**(`ash smart git.finish-worktree --push`) | CLI flag 直接解析 | 用户/脚本 |

三条路径最终都调同一个 `execute_body(spec, args) -> ToolResult`(§4)。**body.ash 是唯一的执行真相**。

---

## 第 4 节:执行路径 + body.ash 调度

### 4.1 body.ash 是 AutoLang 脚本

SmartCommand 的执行体就是普通的 AutoLang 脚本(MS3 已支持)。它通过 shell bridge 调用 shell 能力,通过新增的 native 模块调本地小模型和交互确认。

**SmartCommand 给 AutoLang VM 注入的 native 模块**:

| Native | 作用 | 实现 |
|---|---|---|
| `system(cmd)` / `system(cmd, format)` | 执行 shell 命令(已有,MS3) | `host::ShellHostImpl` |
| `read_file(path)` / `write_file(path, content)` | 文件 I/O | 新增 |
| `confirm(prompt)` / `confirm(prompt, default)` | 交互确认 | 新增 |
| `ai.generate(prompt, context)` | 一次性本地 AI 调用(不走 Agent) | 新增,直连 OllamaProvider |
| `ai.embed(text)` | 向量嵌入(后续 Plan) | 新增(预留) |
| `args.get(name)` / `args.str(name)` / `args.bool(name)` | 取解析后的参数 | 新增 |

### 4.2 `execute_body` 调度函数

这是三条调用路径的共同终点:

```rust
// ash/auto-shell/src/smart_command/executor.rs
pub fn execute_body(
    spec: &SmartCommandSpec,
    args_json: &serde_json::Value,
    shell: &mut Shell,
) -> ToolResult {
    // 1. 参数校验 + 类型转换(JSON → AutoLang args 对象)
    // 2. 读 body.ash
    // 3. 构造 AutoVM,注入 smart_command natives(args/ai/confirm/read_file)
    // 4. 执行(受 shell policy 约束 —— system() 调用自动过 SecurityPolicy)
    // 5. 包装成 ToolResult 返回
}
```

### 4.3 `ai.generate` native 的实现

body.ash 调本地小模型的入口。**关键**:不走 Agent ReAct 循环(那是 NLU 路径的事),而是一次性直连 provider。

```rust
// ash/auto-shell/src/smart_command/natives/ai.rs
pub fn ai_generate(prompt: &str, context: &str) -> String {
    let client = AiClient::new().expect("aaid daemon unavailable");
    let req = CompletionRequest {
        model: "tier:min".to_string(),
        preferred_provider: Some("ollama".to_string()),  // ← §1 补全的字段
        messages: vec![
            Message::system("You are a concise code assistant. Reply with ONLY the requested content."),
            Message::user(format!("{}\n\nContext:\n{}", prompt, context)),
        ],
        max_tokens: Some(256),
        temperature: Some(0.3),
        ..Default::default()
    };
    // 同步 block_on async 调用(复用 Plan 027 的 block_on_async 模式)
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        client.complete(&req).await
            .map(|r| r.content.trim().to_string())
            .unwrap_or_else(|e| format!("[AI error: {}]", e))
    })
}
```

**为什么 ai.generate 跟 NLU 路径不同**:
- NLU 路径(`ash smart "自然语言"`)走 **Agent + SmartCommandRole**(ReAct 循环选命令)
- body.ash 里的 `ai.generate` 走**单次 complete 调用**(直连 provider,无循环)

两者都用 Ollama provider,但调用形态不同。body.ash 里的 AI 是确定性的"给我一段文本的摘要",不需要 Agent 决策能力。

### 4.4 与 SecurityPolicy 的协同

body.ash 里所有 `system()` 调用都**自动过 MS2 的 SecurityPolicy**(shell bridge 已这么做):
- `--read-only` 模式下,`git commit` / `git push` 被拒
- `--sandbox` 模式下,worktree 路径必须在 sandbox 内
- `--no-network` 模式下,`git push` 被拦

这是 SmartCommand 的**自动安全网**——不需要作者操心,继承 shell 的 policy。

SmartCommand 有自己的额外安全层:`confirm_before`(从 command.at 读)。这是 SmartCommand 专属的"用户在场确认",比 SecurityPolicy 更细粒度。

### 4.5 执行的统一出口:Plan 028 信封

所有三条路径执行完 body.ash,都返回 `ToolResult`,最终由 Plan 028 的 `build_envelope` 包装成统一信封。跟 `ash agent run ls` 的信封是同一个 schema——Agent 端无感知差异。

---

## 第 5 节:`ash smart` CLI

### 5.1 子命令设计

```bash
# (1) 列出所有已加载的 SmartCommand
ash smart list
ash smart list --format json

# (2) 查看某个 SmartCommand 的详情
ash smart show git.finish-worktree
ash smart show git.finish-worktree --format json   # 含完整 args schema

# (3) 自然语言调用(NLU 路径)
ash smart "finish this worktree and push to main"
ash smart "finish this worktree and push" --dry-run

# (4) 显式调用(直接执行 body.ash,零 AI)
ash smart git.finish-worktree
ash smart git.finish-worktree --push --target main --message-source plan

# (5) 通过 ash agent run 间接调用(Plan 028 已有,非新增)
ash agent run 'git.finish-worktree' --args '{"push":true}'

# (6) 重新加载(开发 SmartCommand 时用)
ash smart reload
```

### 5.2 自然语言 vs 显式的判定

用户输入 `ash smart <arg>` 时:
- 如果第一个参数匹配某个已加载的 SmartCommand name → **显式模式**
- 否则 → **自然语言模式**

```rust
pub fn dispatch(args: &[String]) -> i32 {
    // ... 处理 list/show/reload 子命令 ...
    let specs = load_all_smart_commands();
    if specs.iter().any(|s| s.name == args[0]) {
        cmd_explicit(&specs, args)      // 显式
    } else {
        let nl = args.join(" ");
        cmd_natural(&specs, &nl)        // 自然语言
    }
}
```

### 5.3 Shell 借用难题:两阶段执行(关键设计)

NLU 路径有根本性张力:`Agent::run` 是 async 的、内部回调 `Tool::execute`,但 `execute_body` 需要 `&mut Shell`,而 Shell 不是 Send+Sync。

**决策(方案 C):两阶段——LLM 只决策,执行在同步上下文**

SmartCommand 的 LLM 决策是"一次性"的(选命令+填参数),不需要 LLM 在执行中追加决策。所以天然可以分离:

```rust
fn cmd_natural(specs: &[SmartCommandSpec], nl: &str) -> i32 {
    // ── 阶段 1:async,Agent 决策(选命令 + 填参数)──
    let decision = rt.block_on(async {
        let role = build_smart_role(specs);
        let agent = Agent::new(Box::new(role), client);
        // agent 的 Tool 是"假的"——execute 只返回 "ok",不真执行
        // 目的:让 LLM 走完 ReAct,产出 tool_call
        agent.run(nl).await
    });
    let tool_call = extract_single_tool_call(&decision)?;

    // ── 阶段 2:同步,真实执行 ──
    let mut shell = Shell::new();
    let spec = specs.iter().find(|s| s.name == tool_call.name).unwrap();
    let result = execute_body(spec, &tool_call.input, &mut shell);  // ← 同步,&mut Shell OK
    print_envelope(result, &format!("smart \"{}\"", nl));
}
```

**关键洞察**:LLM 决策和 Shell 执行天然可以分离。这跟通用 Agent(可能执行中观察结果再决策)不同,但 SmartCommand 的定位就是"轻量",不需要复杂 ReAct。这让实现**简单一个数量级**——不需要 Send+Sync 的 Shell,不需要 Mutex。

### 5.4 `--dry-run` 的语义

NLU 路径的 `--dry-run` 只让 LLM 解析参数,不执行 body.ash:

```bash
$ ash smart "finish this worktree and push" --dry-run
{
  "schema_version": "1",
  "status": "success",
  "data": {
    "kind": "smart_dry_run",
    "value": {
      "selected_command": "git.finish-worktree",
      "resolved_args": { "target": "auto", "push": true, "message_source": "diff" },
      "note": "未执行(--dry-run)。用 ash smart git.finish-worktree --push 真实执行。"
    }
  }
}
```

### 5.5 错误信封

| 场景 | status | error.kind |
|---|---|---|
| NLU 无法匹配任何命令 | failed | not_found |
| 参数类型不对 | failed | invalid_args |
| body.ash 里 system() 被 policy 拒 | denied | (DeniedReason) |
| body.ash 执行出错 | failed | nonzero_exit |
| Ollama daemon 没起 | failed | internal |
| 本地模型超时 | failed | timeout |

---

## 第 6 节:`git.finish-worktree` 完整实例

### 6.1 为什么选它作为首个 SmartCommand

1. **真实痛点**:每个用 worktree 的开发者每天都做这 4 步
2. **混合执行**:4 步里只有 1 步(commit message)真需要 AI
3. **本地小模型够用**:写 commit message 不需要 GPT-4 级推理,9B Ornith 完全胜任
4. **展示协同**:SecurityPolicy + Plan 028 + auto-ai 三方协同
5. **安全敏感**:涉及 merge/push/删除分支,必须有 confirm 层

### 6.2 三件套文件

完整的 `command.at` / `body.ash` / `skill.md` 内容见附录 A(实施时作为内置 SmartCommand 嵌入 ash 二进制)。

### 6.3 三种调用方式验证

```bash
# 方式 A:自然语言(NLU)
ash smart "finish this worktree"
ash smart "merge to main without pushing"

# 方式 B:显式 flag(零 AI)
ash smart git.finish-worktree --target main --push
ash smart git.finish-worktree --message-source manual --message "fix: login bug"

# 方式 C:Agent tool(Plan 028 协同)
ash agent run 'git.finish-worktree' --args '{"target":"main","push":true}'
ash agent describe-tools --filter git

# 方式 D:--dry-run(安全预检)
ash smart "finish this worktree" --dry-run
```

### 6.4 安全场景验证

```bash
# 在主分支上执行 → 应拒绝
$ (on main) ash smart git.finish-worktree
# → "当前已在 main 分支,finish-worktree 应在 worktree 分支上执行"

# --read-only 模式 → commit 应被拦
$ ash --read-only smart git.finish-worktree
# → denied: write command blocked

# --no-network 模式 → push 应被拦
$ ash --no-network smart git.finish-worktree --push
# → denied at push step

# 确认步骤取消 → 应干净退出
$ ash smart git.finish-worktree
# 继续? (y/n) n
# → "已取消,未做任何改动",exit 0,无副作用
```

### 6.5 这个实例验证了什么

| 设计点 | 验证方式 |
|---|---|
| 三位一体文件结构 | command.at + body.ash + skill.md 三文件齐全且能加载 |
| 混合执行 | body.ash 里既有 system() 又有 ai.generate() |
| 本地小模型 | commit message 由 Ollama 生成 |
| SecurityPolicy 协同 | --read-only/--no-network 正确拦截 |
| Plan 028 Tool 透明注册 | `ash agent describe-tools` 能看到 git.finish-worktree |
| 三路径统一 | NLU/显式/Agent CLI 三种调用产生相同结果 |
| --dry-run | 只解析不执行,显示选中命令+参数 |
| 错误信封 | 各类失败场景返回正确的 ErrorKind |
| 安全确认 | confirm_before 生效,n 能干净取消 |

---

## 第 7 节:里程碑、依赖、风险、非目标

### 7.1 里程碑分解

#### M1 — auto-ai 改造(前置,在 auto-ai 仓库)

**目标**:Ollama provider 可用,SmartCommandRole 能声明 `preferred_provider = "ollama"`。

**交付物**:
- `auto-ai-daemon/src/provider/ollama.rs` —— OllamaProvider(薄包装)
- `provider/mod.rs` 加 `"ollama" =>` 分支
- `RoleConfig` 加 `preferred_provider` 字段 + parse/serialize
- `ConfigRole` 覆盖 `preferred_provider()`
- `CompletionRequest` 加 `preferred_provider` 字段
- `Agent::build_request` 填入字段
- `server.rs:133` 改用 `TierRouter::resolve(tier, pref)`
- 单元测试覆盖每处改动

**验证**:老配置零破坏(回归测试);带 `preferred_provider = "ollama"` 的 Role 请求确实路由到 OllamaProvider;`serialize_at_role` 往返保留字段。

**规模**:中等偏大(跨 3 个 crate,~800 行)。**这部分要给 auto-ai 仓库提 PR**。

#### M2 — SmartCommand 格式 + 加载器(在 auto-shell 仓库)

**目标**:`ash smart list` / `ash smart show` 可用,内置 SmartCommand 能被加载。

**交付物**:
- `smart_command/config.rs` —— `SmartCommandSpec` + parse/serialize
- `smart_command/loader.rs` —— 扫 smart/ 目录(用户级 + 项目级 + 内置)
- `smart_command/schema.rs` —— `args_to_json_schema`
- 内置 smart 目录 + `include_dir!` 嵌入
- `ash smart list` + `ash smart show` CLI

**验证**:`ash smart list` 列出内置命令,`ash smart show git.finish-worktree` 显示完整配置 + schema。

**规模**:中等(~1000 行)。依赖 M1。

#### M3 — 执行引擎(在 auto-shell 仓库)

**目标**:SmartCommand 能执行 body.ash,三路径统一。

**交付物**:
- `smart_command/executor.rs` —— `execute_body(spec, args, &mut Shell)`
- `smart_command/natives/` —— args/ai/confirm/read_file 的 AutoLang native 实现
- `smart_command/role.rs` —— `build_smart_role(specs)` + system prompt
- `smart_command/cli.rs` —— NLU 路径(两阶段执行)+ 显式路径
- `--dry-run` 模式
- SmartCommand 注册进 Plan 028 ToolRegistry

**验证**:`ash smart git.finish-worktree`(内置空 body)能跑通,`ash agent run git.finish-worktree` 也能调到。

**规模**:大(~1500 行)。依赖 M1 + M2。

#### M4 — `git.finish-worktree` 完整实例

**目标**:三件套完整实现,四种调用方式全部可用。

**交付物**:
- `smart/git.finish-worktree/command.at` 完整配置
- `smart/git.finish-worktree/body.ash` 完整执行体
- `smart/git.finish-worktree/skill.md` 完整文档
- 集成测试覆盖三种调用 + 安全场景

**验证**:§6.3 的四种调用 + §6.4 的安全场景全部通过。

**规模**:小(~400 行 body.ash + skill.md + 测试)。依赖 M1+M2+M3。

### 7.2 跨仓库协作策略

Plan 029 跨 auto-shell + auto-ai。决策:**并行 + 临时路径依赖**(路径 B)。auto-shell 分支先依赖一个本地 auto-ai 路径(M1 改在 auto-ai 的 feature 分支),auto-shell 全部完成后,等 auto-ai M1 合入再切回正式 auto-ai 版本。

### 7.3 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| **Ollama 模型质量不够** | commit message 生成质量差,价值打折 | M4 验收时对比 Ornith-9B / Qwen2.5-Coder 7B 输出;不达标则降级 message_source=manual 走 template |
| **auto-lang 又被改动打断** | 实施期间 auto-lang 反复坏(028 已遇两次) | 依赖 pin 到 auto-lang 某个稳定 commit;跟维护者协调冻结期 |
| **Shell 借用难题未解决** | NLU 路径无法在 async 里调 execute_body | §5.3 已定方案(两阶段执行);若 Agent 决策复杂化则需重设计 |
| **auto-ai PR 不被接受** | preferred_provider 链路无法合入,M1 阻塞 | M1 改动是加法 + 默认 None,零破坏,review 阻力小;最坏 fork |
| **SmartCommand 数量爆发** | 用户写很多 .smart.at,加载慢/冲突 | M2 加载器做缓存;name 冲突时报错(不静默覆盖) |
| **本地小模型未安装** | Ollama provider 连不上 | 启动时探测 Ollama,给清晰错误;显式路径(零 AI)作 fallback |
| **AutoVM session 复用冲突** | execute_body 复用 shell 的 session,native 注入可能污染 | 每次 execute_body 前后用 begin_run/end_run 包裹,隔离 native 注入(Plan 011 已有机制) |

### 7.4 非目标(明确排除)

- ❌ 复杂多轮 NLU —— 一次性参数解析(max_turns=3),不是对话式 Agent
- ❌ 通用 Agent 能力 —— SmartCommand 不是 AutoCoder 替代品,是"轻量命令增强"
- ❌ AutoDown → Markdown 转译 —— 探勘证实不存在,skill.md 走 sidecar
- ❌ 多个 SmartCommand 链式调用 —— `finish-worktree` 调 `another-smart` 不在 v1
- ❌ SmartCommand 编辑器 —— 手写 command.at + body.ash,无 GUI
- ❌ 远程 SmartCommand 分发 —— 不做中央 registry(留给后续 Plan)
- ❌ Skill 系统统一 —— SmartCommand 的 skill.md 不并入 auto-ai 的 Skill 系统

### 7.5 成功指标

1. **Ollama provider** 配置后可调,本地模型推理成功
2. **preferred_provider 链路** 端到端通(有集成测试)
3. **`ash smart list`** 至少列出 1 个内置 SmartCommand
4. **`git.finish-worktree`** 四种调用全部成功,产出正确信封
5. **安全场景** 全部正确响应
6. **`ash agent describe-tools`** 能看到 git.finish-worktree
7. **老接口零破坏**:auto-ai 和 auto-shell 现有测试全过
8. **真人试用**:用 `ash smart "finish this worktree"` 真实完成一次 worktree,commit message 质量可接受

### 7.6 与其他 Plan 的关系

| 关联 Plan | 关系 |
|---|---|
| **Plan 028**(Agent 执行引擎) | **强依赖**:SmartCommand 注册进 ToolRegistry,复用 build_envelope。028 的 M3/M4 不是前置,可并行 |
| **Plan 027**(AI chat v1) | 弱关联:共享 auto-ai 基础设施但独立 |
| **MS2**(沙箱) | 强依赖:system() 调用自动过 SecurityPolicy |
| **MS3**(shell bridge) | 强依赖:body.ash 通过 shell bridge 调 system() |
| **Plan 023**(config 迁移) | 参照:.at 配置模式同构 |

### 7.7 后续 Plan(明确不在 029 范围)

- **Plan 030**:SmartCommand 集市(central registry,`ash smart install <name>`)
- **Plan 031**:更多内置 SmartCommand(`deploy.safe` / `refactor.rename` / `test.fix`)
- **Plan 032**:SmartCommand 测试框架
- **Plan 033**:F4 chat 集成 SmartCommand
- **Plan 034**:AutoCoder TUI 里用 SmartCommand 作为快捷动作

---

## 附录 A:`git.finish-worktree` 完整三件套

实施时作为内置 SmartCommand 嵌入 ash 二进制。完整内容见 brainstorming 过程中的 §7.2(此处不再重复,实施时从该节复制)。

---

## 附录 B:实施前置勘探记录(2026-07-20)

本设计基于两次代码勘探,关键发现:

### auto-ai 架构发现
- **Provider trait** 在 `auto-ai-daemon/src/provider/mod.rs`(不在 client),只有 Anthropic + OpenAI 两个实现,**无 Ollama**
- **Ollama 兼容性已在配置层预留**:`ProviderConfig::resolve_key()` 对无 key provider 返回 `"no-key-needed"`
- **Role 系统已存在**:14 内置 Role + 用户自定义(`~/.config/autoos/roles/<name>.at`)
- **preferred_provider 半搭好**:`Role::preferred_provider()` 和 `TierRouter::resolve(tier, pref)` 存在,但 RoleConfig/ConfigRole/CompletionRequest/agent.rs/server.rs 五处未接通
- **无 tier:local**:Tiers 是能力档(min/lite/mid/pro/max),本地性是 provider 层的事
- **function-calling 完整实现**:`Agent::run` / `run_stream` 有完整 ReAct 循环
- **aaid daemon 是唯一 LLM 网关**:所有 provider 注册和解析都在这里

### auto-lang / AutoLang 配置 DSL 发现
- **AutoLang 配置 DSL 完全成熟**:`daemon.at` / `roles/<name>.at` / `config.at` 是生产级配置文件
- **有 `key : value` / 嵌套块 / 数组 / 对象数组 / 注释 / 裸标识符枚举**
- **`parse_at_role` + `serialize_at_role` 已验证可往返**
- **三套配置实现**:auto-atom 通用层 / auto-ai-agent 类型化层 / ash 的 auto_config.rs(自包含但只支持两层)
- **AutoDown 是真实模块但只转 Typst/HTML**,**无 Markdown transpiler**
- **AutoLang → Markdown codegen 不存在**:现有解法是 sidecar 文件(roles 的 `.soul.md` 模式)
- **AutoLang 无三引号多行字符串**:大段文本走 sidecar 是已设计模式

### 关键设计推论
基于以上发现,SmartCommand 设计:
1. `.smart.at` 配置 + sidecar 文件(沿用 roles 模式)
2. OllamaProvider 薄包装(组合 OpenAiProvider)
3. 补全 preferred_provider 链路(5 个串联改动)
4. 两阶段执行(async 决策 + 同步执行,规避 Shell 借用难题)

---

## 参考

- `designs/028-agent-execution-engine.md` —— Plan 028,SmartCommand 注册进它的 ToolRegistry
- `D:\autostack\auto-ai\ARCHITECTURE.md` —— auto-ai 架构文档
- `D:\autostack\auto-ai\crates\ai-config\examples\daemon.at` —— 配置 DSL 范例
- `D:\autostack\auto-ai\crates\auto-ai-agent\src\config\role_config.rs` —— 类型化 .at parse/serialize 模式
- `D:\autostack\auto-ai\crates\auto-ai-agent\src\builtin_roles\` —— 内置 Role 实现模式
- `D:\autostack\auto-lang\crates\auto-val\src\emit.rs:26-28` —— "大段文本用 sidecar" 的设计依据
- `D:\autostack\auto-shell\plans\023-ash-unified-config-migration.md` —— ash 的 .at 配置迁移(模式参照)
