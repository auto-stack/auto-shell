# ASH for AutoStack 生态内部

> 给把 ash 作为底层引擎的 AutoStack 生态项目（AutoCoder / auto-musk 等）集成用的指南。

## 架构分层

ash 采用严格的分层架构（Plan 014），保证引擎纯逻辑、可复用：

```
┌─────────────────────────────────────────────────────┐
│  Frontend（终端依赖层）                              │
│  ┌──────────────┐  ┌──────────────┐                 │
│  │  TUI (CLI)   │  │  GUI (iced)  │  ← Plan 030     │
│  │  reedline    │  │  AutoUI      │                 │
│  │  ratatui     │  │              │                 │
│  └──────┬───────┘  └──────┬───────┘                 │
│         └────────┬────────┘                         │
│           Renderer trait (Plan 030 M1)              │
├─────────────────────────────────────────────────────┤
│  Backend（零终端依赖）                               │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────┐  │
│  │  ash-core    │  │  auto-shell  │  │  ash-gui  │  │
│  │  (纯逻辑)    │  │  (Shell 引擎)│  │  (GUI bin)│  │
│  └──────────────┘  └──────────────┘  └───────────┘  │
└─────────────────────────────────────────────────────┘
```

## 作为依赖使用

### ash-core（纯逻辑引擎，推荐）

`ash-core` 是零终端依赖的纯逻辑 crate。包含：parser、pipeline（Atom 类型系统）、security（SecurityPolicy）、completions 引擎。

```toml
# 你的 Cargo.toml
[dependencies]
ash-core = { path = "../auto-shell/ash-core" }
```

**能用到什么**：
- `ash_core::pipeline::{Atom, AtomPipeline, AtomType}` —— 18 种语义类型
- `ash_core::security::SecurityPolicy` —— 安全策略 + sandbox
- `ash_core::parser::*` —— pipeline/quote/redirect 解析
- `ash_core::completions::*` —— 补全引擎（CompletionSpec + CompletionProvider）

**适合**：需要 ash 的解析/pipeline/安全逻辑，但自己管 UI 的项目（如 AutoCoder）。

### auto-shell（完整 Shell 引擎）

`auto-shell` 包含 Shell 引擎 + 80 命令 + REPL + AI 集成。依赖 reedline/ratatui（终端库）。

```toml
[dependencies]
auto-shell = { path = "../auto-shell/ash/auto-shell" }
```

**能用到什么**：
- `auto_shell::Shell` —— 完整 Shell 实例（execute/format_output/registry）
- 80 个内置命令
- `auto_shell::Repl` —— reedline REPL 循环
- `auto_shell::frontend::ai::ChatSession` —— F4 chat

**注意**：auto-shell 是 binary crate（`[[bin]] name = "ash"`），作为 lib 依赖时需确认链接配置。

### feature flag（Plan 030 M0）

```toml
# 只用 Shell 引擎，不要 reedline/ratatui（给 GUI 用）
auto-shell = { path = "...", default-features = false }
# default-features = ["frontend-tui"]，关掉后纯引擎可嵌入 GUI
```

## 典型集成模式

### 模式一：嵌入 Shell 引擎（AutoCoder / GUI 产品）

```rust
use auto_shell::Shell;

let mut shell = Shell::new();
shell.load_env_persistence();
shell.set_policy(policy);  // 安全策略

// 执行命令
let output = shell.execute("ls | sort .size | head")?;
println!("{}", output.unwrap_or_default());

// Agent 模式（结构化 JSON 输出）
let json = shell.execute_for_agent("ls", true)?;
```

### 模式二：用 ash-core 的 pipeline 类型

```rust
use ash_core::pipeline::{Atom, AtomPipeline, AtomType};
use ash_core::security::SecurityPolicy;

// 构造结构化数据
let file_list = Atom::file_list(json_value);
let pipeline = AtomPipeline::from_atom(file_list);

// 安全策略
let policy = SecurityPolicy {
    sandbox_dir: Some("/project".into()),
    no_network: true,
    ..Default::default()
};
policy.check("rm", &["-rf", "/"], false)?;  // 会拒绝
```

### 模式三：子进程调用（Agent CLI）

最简单的集成——不嵌入，通过 `ash agent` CLI 调：

```rust
use std::process::Command;

// 拉工具 catalog
let tools = Command::new("ash")
    .args(["agent", "describe-tools", "--format", "compact"])
    .output()?;

// 执行命令拿结构化信封
let result = Command::new("ash")
    .args(["agent", "run", "ls -la /project"])
    .output()?;
let envelope: serde_json::Value = serde_json::from_slice(&result.stdout)?;
```

## 关键类型速查

### AtomPipeline（管道数据）

```rust
pub enum AtomPipeline {
    Atom(Atom),                    // 单个结构化值
    Stream(AtomStream),            // 流式（Plan 031 lazy）
    ExternalStream(ExternalStream),// 外部进程输出
    Text(String),                  // 纯文本
    Empty,
}

pub struct Atom {
    pub value: Value,              // auto_val::Value
    pub atom_type: AtomType,       // 18 种语义标签
}
```

### SecurityPolicy（安全策略）

```rust
pub struct SecurityPolicy {
    pub allow: Vec<String>,        // 白名单
    pub deny: Vec<String>,         // 黑名单
    pub no_exec: bool,             // 禁外部命令
    pub no_network: bool,          // 禁网络
    pub read_only: bool,           // 只读
    pub dry_run: bool,             // 只看不做
    pub sandbox_dir: Option<PathBuf>, // 路径沙箱
    pub audit_file: Option<PathBuf>,  // 审计日志
}

// 检查（返回 Allow / DryRun / Err(拒绝)）
policy.check(cmd_name, args, is_external)?;
```

### Shell（引擎入口）

```rust
impl Shell {
    pub fn new() -> Self;
    pub fn execute(&mut self, input: &str) -> Result<Option<String>>;
    pub fn execute_for_agent(&mut self, input: &str, json_mode: bool) -> Result<Option<String>>;
    pub fn set_policy(&mut self, policy: SecurityPolicy);
    pub fn registry(&self) -> &CommandRegistry;
    pub fn build_tool_registry(&self) -> ToolRegistry;  // Plan 028
}
```

## 相关仓库

| 仓库 | 作用 |
|------|------|
| `auto-lang` | AutoLang 语言引擎（脚本、VM、AutoUI） |
| `auto-ai` | AI 基础设施（aaid daemon、AiClient、Agent ReAct 循环） |
| `auto-shell`（本仓库） | Shell 引擎 + CLI + GUI |
| `auto-musk` | AutoCoder（用 ash 作引擎的 Agent 产品） |

## 设计文档

生态相关的扩展方向设计：
- [Plan 029 AI 能力增强](../designs/029-ai-capabilities.md) —— SmartCommand / F4 / F3 / 上下文
- [Plan 030 ash-gui](../designs/030-ash-gui.md) —— GUI 前端（Renderer trait 分层）
- [Plan 031 数据处理](../designs/031-data-processing.md) —— lazy pipeline
- [Plan 033 插件生态](../designs/033-plugin-ecosystem.md) —— 第三方扩展
- [横向一致性检查](../designs/000-cross-cutting-review.md) —— 所有方向的冲突/融合分析
