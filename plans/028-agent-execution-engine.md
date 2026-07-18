# ASH Agent 执行引擎 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **⚠️ 实现状态(2026-07-18):** M1+M2 已实现完毕(分支 `feat/028-agent-engine`)。实现中发现 3 处与原 plan 不符的偏差(`AtomPipeline` 不 Clone / `auto_val::Value` ≠ `serde_json::Value` / `Command` 无 Send+Sync bound),已就地修正。**完整偏差记录见 `designs/028-agent-execution-engine.md` 附录 B**。M3/M4 展开前必读该附录。下面 M1/M2 的任务文本保留原样作为历史记录;实际代码以分支上的 commit 为准。

**Goal:** 为 ASH 构建统一的 Tool Registry 和外部 Agent CLI 接口,让 AI Agent 能安全、确定、结构化地调用 ash 的 80 个命令。

**Architecture:** 在 `ash-core` 新增 `tool/` 模块,定义 `Tool` trait + `ToolRegistry`;通过 `CommandToolBridge` 让现有 80 个 `Command` 自动满足 `Tool`;在 `auto-shell` 新增 `ash agent ...` CLI 子命令族(describe-tools / describe-policy / check / run / run-batch)。所有改动是加法,旧 `ash -c --json` 接口保持不变。

**Tech Stack:** Rust 2021、serde/serde_json(新增给 ash-core)、miette(错误)、现有 ash-core pipeline/security 模块。

**对应设计文档:** `designs/028-agent-execution-engine.md`

**范围:** 本 Plan 详细覆盖 M1(Tool Registry 骨架 + 桥接)+ M2(CLI Agent 接口 + 结构化输出)。M3(批量 NDJSON)+ M4(跨平台测试矩阵)只给任务概要,留待 M1+M2 完成后各自细化。

---

## 关键背景知识(实施者必读)

### 现有代码的关键事实(来自代码勘探,勿凭记忆)

1. **`AtomType` 有 18 个变体**(不是 21):`FileEntry, FileList, ProcessEntry, ProcessList, DiskEntry, CpuInfo, MemoryInfo, SystemInfo, MatchList, CountResult, Table, Record, Text, Path, BuildResult, RunResult, HelpInfo, Nothing`。定义在 `ash-core/src/pipeline/atom.rs:10-67`,只 derive `Debug, Clone, Copy, PartialEq, Eq`(无 Display、无 Serialize)。

2. **`AtomPipeline` 在 `ash-core/src/pipeline/atom_pipeline.rs`**(不在 `atom.rs`)。5 个变体:`Atom(Atom) | Stream(AtomStream) | ExternalStream(ExternalStream) | Text(String) | Empty`。不 derive 任何 trait。

3. **`SecurityPolicy::check()` 返回 `miette::Result<Decision>`**,**`Decision` 只有 `Allow` 和 `DryRun` 两个变体**——拒绝是 `Err(...)`,不是 `Decision::Deny`。匹配必须用三臂:`Ok(Allow) | Ok(DryRun) | Err(_)`。

4. **`ash-core` 当前零 serde 依赖**(刻意设计,见 `security.rs:281` 注释)。本 Plan 明确为其新增 `serde` + `serde_json`,只用于 `tool/` 模块。

5. **`CommandRegistry`** 在 `ash/auto-shell/src/cmd/registry.rs`,内部是 `HashMap<String, Arc<dyn Command>>`,`register(Box<dyn Command>)` 接收 Box,`get(name) -> Option<Arc<dyn Command>>`。

6. **`Shell` 结构** 在 `ash/auto-shell/src/shell.rs:93-135`。`policy: SecurityPolicy` 是 `pub` 字段;`registry: CommandRegistry` 是私有(需加访问器或传引用)。`json_output: bool` 字段控制 JSON 输出。

7. **`execute_for_agent()`** 在 `shell.rs:891-899`,只有 5 行,就是切换 `json_output` 后调 `execute()`。

8. **CLI 解析是手写的**(无 clap),在 `ash/auto-shell/src/main.rs:33-163`,用 `while i < args.len()` + `match arg.as_str()`。

9. **`ChatSession`** 在 `frontend/ai.rs:83-87`,三个字段全私有:`messages: Vec<Message>, history_path: PathBuf, client: AiClient`。`Message` 来自 `auto_ai_client` crate(非本地)。

10. **`Signature`** 在 `cmd.rs:67-75`,有 `name, description, arguments: Vec<Argument>, extra_help`。`Argument` 有 `name, description, required, is_flag, is_option, short, default`。

### 技术约束

- **所有改动是加法**:旧 `ash -c "..." --json` 行为必须不变(有回归测试守护)。
- **serde 边界**:只给 `tool/` 模块的新类型 derive Serialize/Deserialize。**不要**给 `Atom`/`AtomPipeline`/`AtomType`/`SecurityPolicy` 加 derive(会触动大量已有代码)。需要序列化老类型时,在 `tool/` 里写转换函数。
- **TDD**:每个任务先写失败测试,再写实现。

### 测试约定

- `ash-core` 的单元测试用 `#[cfg(test)] mod tests` 内联,或 `ash-core/tests/` 集成测试。
- `auto-shell` 的 CLI 测试用 `std::process::Command` 调 `cargo run -- agent ...` 做端到端验证。
- 所有 JSON 断言用 `serde_json::json!` 宏构造期望值,用 `pretty_assertions::assert_eq` 比较。

---

## 文件结构

### 新增文件

| 文件 | 职责 | 里程碑 |
|---|---|---|
| `ash-core/src/tool/mod.rs` | `Tool` trait、`ToolContext`、`ToolResult`、`ToolStatus`、`ToolData`、`Capabilities`、`ConfirmationMode`、`OutputLimits` | M1 |
| `ash-core/src/tool/schema.rs` | JSON Schema 类型(`ToolDescriptor`、`CatalogSummary`)、从 `Signature` 推导 schema 的 `derive_schema_from_signature()` | M1 |
| `ash-core/src/tool/catalog.rs` | `ToolRegistry` 结构体 + `catalog()` / `catalog_compact()` | M1 |
| `ash-core/src/tool/error.rs` | `ToolError`、`ErrorKind`、`DeniedReason` | M1 |
| `ash-core/src/tool/bridge.rs` | `CommandToolBridge<T>` —— 但注意:bridge 需要访问 `Command` trait,而 `Command` 在 `auto-shell` 不在 `ash-core`。见下方"架构难题与解法" | M1 |
| `ash-core/src/tool/agent_loop.rs` | `ToolExecuting` trait、`LlmProvider`(仅接口,无实现) | M1 |
| `ash-core/tests/tool_registry.rs` | Tool Registry 集成测试 | M1 |
| `ash/auto-shell/src/agent/mod.rs` | `agent` 子命令分发 | M2 |
| `ash/auto-shell/src/agent/describe.rs` | `describe-tools` + `describe-policy` 实现 | M2 |
| `ash/auto-shell/src/agent/run.rs` | `run` + `check` 实现 | M2 |
| `ash/auto-shell/tests/agent_cli.rs` | CLI 端到端测试 | M2 |

### 修改文件

| 文件 | 改动 | 里程碑 |
|---|---|---|
| `ash-core/Cargo.toml` | 加 `serde` + `serde_json` 依赖 | M1 |
| `ash-core/src/lib.rs` | 加 `pub mod tool;` | M1 |
| `ash/auto-shell/src/lib.rs` | 加 `pub mod agent;`(若需要) | M2 |
| `ash/auto-shell/src/main.rs` | 在 CLI 解析循环里加 `agent` 子命令分支 | M2 |
| `ash/auto-shell/src/shell.rs` | 给 `ToolRegistry` 构建加访问器(或新方法) | M1/M2 |

### 架构难题与解法:bridge 的位置

**难题**:`Tool` trait 我们想放在 `ash-core`(纯逻辑、可复用),但 `Command` trait 在 `auto-shell`(`ash-core` 不能依赖 `auto-shell`,会循环)。

**解法**:**反向依赖**。把 `Tool` trait 放 `ash-core`。`CommandToolBridge` 放 `auto-shell`(它同时能看见 `Command` 和 `ash-core::tool::Tool`)。这样依赖方向正确:

```
auto-shell  ──depends on──>  ash-core
   │                            │
   CommandToolBridge            Tool trait
   (impl Tool for ...Command)   (定义在 ash-core)
```

`ash-core` 不需要知道 `Command` 的存在;它只定义 `Tool` trait 的形状。`auto-shell` 负责 bridge。

> **spec 修正说明**:spec 第 1.5 节原写 `ash-core/src/tool/bridge.rs`,实际 bridge 必须在 `auto-shell`。本 Plan 以此处为准。

---

# 里程碑 M1:Tool Registry 骨架 + 桥接

**目标**:80 个命令自动获得最小 Tool 描述,可被 `ash agent describe-tools` 拉取(虽然 CLI 在 M2 才接上,但 Registry 本身在 M1 就绪并能被单元测试验证)。

**完成标准**:`ToolRegistry::catalog()` 能对全部 80 个命令产出合法 JSON Schema,每个含 name/description/最小 parameters。

---

## Task M1.1:为 ash-core 加 serde 依赖

**Files:**
- Modify: `ash-core/Cargo.toml`

- [ ] **Step 1: 加依赖**

在 `[dependencies]` 段(现有 `miette = "7.0"` 之后)加:

```toml
# Plan 028: Tool Registry 需要 JSON Schema 序列化。
# 只用于 tool/ 模块;现有 Atom/AtomPipeline/SecurityPolicy 不加 derive。
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: 验证依赖能编译**

Run: `cargo build -p ash-core`
Expected: 编译成功(可能有一堆 unused warning,正常)。

- [ ] **Step 3: Commit**

```bash
git add ash-core/Cargo.toml
git commit -m "build(core): add serde + serde_json deps for Plan 028 Tool Registry"
```

---

## Task M1.2:error.rs —— ToolError 与 ErrorKind

**Files:**
- Create: `ash-core/src/tool/error.rs`
- Test: 内联 `#[cfg(test)] mod tests`

- [ ] **Step 1: 写失败测试**

创建 `ash-core/src/tool/error.rs`,先只放测试:

```rust
//! Plan 028: Standardized error type for all Tool invocations.
//!
//! Single source of truth: CLI path, in-process agent loop, and ToolRegistry
//! internals all produce/consume `ToolError`. The `ErrorKind` enum lets an
//! Agent match on recovery strategy without parsing free-text.

use serde::{Deserialize, Serialize};

/// The category of a tool failure. Agents match on this to choose recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Command executed but returned non-zero exit.
    NonzeroExit,
    /// Command name or file path not found.
    NotFound,
    /// OS-level permission denied (NOT sandbox).
    PermissionDenied,
    /// Argument parsing failed.
    InvalidArgs,
    /// Command exceeded its time limit.
    Timeout,
    /// Command hit the SecurityPolicy (sandbox / read-only / no-network).
    SandboxViolation,
    /// Output parsing failed (e.g. from_json on invalid JSON).
    ParseError,
    /// ash internal bug (should not happen; report it).
    Internal,
}

impl ErrorKind {
    /// snake_case string form (matches serde serialization).
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorKind::NonzeroExit => "nonzero_exit",
            ErrorKind::NotFound => "not_found",
            ErrorKind::PermissionDenied => "permission_denied",
            ErrorKind::InvalidArgs => "invalid_args",
            ErrorKind::Timeout => "timeout",
            ErrorKind::SandboxViolation => "sandbox_violation",
            ErrorKind::ParseError => "parse_error",
            ErrorKind::Internal => "internal",
        }
    }
}

/// A standardized tool error. Serialized into the `error` field of the
/// response envelope (see designs/028 §4.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    pub kind: ErrorKind,
    pub message: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_excerpt: Option<String>,
}

impl ToolError {
    pub fn new(kind: ErrorKind, message: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            command: command.into(),
            exit_code: None,
            remediation: None,
            stderr_excerpt: None,
        }
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }

    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        let s: String = stderr.into();
        // Cap at 500 chars to avoid bloating Agent context.
        let excerpt = if s.len() > 500 {
            format!("{}...(truncated)", &s[..500])
        } else {
            s
        };
        self.stderr_excerpt = Some(excerpt);
        self
    }
}

/// Why a command was denied by the security policy. Distinct from `ToolError`
/// because denials are a first-class `ToolStatus::Denied`, not a failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeniedReason {
    /// Stable rule identifier, e.g. "path-outside-sandbox".
    pub rule_id: String,
    /// Human-readable explanation.
    pub message: String,
    /// Machine-readable recovery suggestion (Agent-actionable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

impl DeniedReason {
    pub fn new(rule_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            rule_id: rule_id.into(),
            message: message.into(),
            remediation: None,
        }
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_serializes_to_snake_case() {
        let kind = ErrorKind::NonzeroExit;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"nonzero_exit\"");
    }

    #[test]
    fn error_kind_roundtrips() {
        for kind in [
            ErrorKind::NonzeroExit,
            ErrorKind::NotFound,
            ErrorKind::PermissionDenied,
            ErrorKind::InvalidArgs,
            ErrorKind::Timeout,
            ErrorKind::SandboxViolation,
            ErrorKind::ParseError,
            ErrorKind::Internal,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: ErrorKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind, "roundtrip failed for {:?}", kind);
        }
    }

    #[test]
    fn tool_error_omits_none_fields() {
        let e = ToolError::new(ErrorKind::NotFound, "no such file", "cat missing.txt");
        let json = serde_json::to_string(&e).unwrap();
        // exit_code, remediation, stderr_excerpt should be absent
        assert!(!json.contains("exit_code"));
        assert!(!json.contains("remediation"));
        assert!(!json.contains("stderr_excerpt"));
        assert!(json.contains("\"kind\":\"not_found\""));
        assert!(json.contains("\"command\":\"cat missing.txt\""));
    }

    #[test]
    fn tool_error_includes_fields_when_set() {
        let e = ToolError::new(ErrorKind::NonzeroExit, "exit 2", "grep")
            .with_exit_code(2)
            .with_remediation("try: grep -i")
            .with_stderr("long stderr output");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"exit_code\":2"));
        assert!(json.contains("\"remediation\":\"try: grep -i\""));
        assert!(json.contains("\"stderr_excerpt\""));
    }

    #[test]
    fn stderr_is_truncated_at_500_chars() {
        let long = "x".repeat(1000);
        let e = ToolError::new(ErrorKind::Internal, "x", "x").with_stderr(&long);
        assert!(e.stderr_excerpt.unwrap().len() < 600); // 500 + suffix
        assert!(e.stderr_excerpt.unwrap().contains("truncated"));
    }

    #[test]
    fn denied_reason_omits_remediation_when_none() {
        let d = DeniedReason::new("path-outside-sandbox", "denied");
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("remediation"));
    }
}
```

- [ ] **Step 2: 运行测试(此时会失败,因为 mod 还没挂到 lib.rs)**

Run: `cargo test -p ash-core tool::error`
Expected: FAIL —— 编译错误 `error[E0432]: unresolved module tool`(因为还没在 lib.rs 声明)。

- [ ] **Step 3: 创建 tool 模块骨架并挂到 lib.rs**

创建空文件 `ash-core/src/tool/mod.rs`,内容(暂时):

```rust
//! Plan 028: Tool Registry — the unified description layer for AI Agents.
//!
//! See `designs/028-agent-execution-engine.md` for the full design.
pub mod error;
```

修改 `ash-core/src/lib.rs`,在现有 `pub mod security;` 之后加:

```rust
pub mod tool;
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p ash-core tool::error`
Expected: PASS —— 6 个测试全过。

- [ ] **Step 5: Commit**

```bash
git add ash-core/src/tool/error.rs ash-core/src/tool/mod.rs ash-core/src/lib.rs
git commit -m "feat(tool): add ToolError/ErrorKind/DeniedReason (Plan 028 M1.2)"
```

---

## Task M1.3:mod.rs —— Tool trait 与核心类型

**Files:**
- Modify: `ash-core/src/tool/mod.rs`(扩充)
- Test: 内联

- [ ] **Step 1: 写失败测试 + 类型定义**

把 `ash-core/src/tool/mod.rs` 替换为:

```rust
//! Plan 028: Tool Registry — the unified description layer for AI Agents.
//!
//! A `Tool` is a single invocable unit that an AI Agent (external CLI like
//! Claude Code, or the built-in F4 chat loop) can call. Every one of ash's
//! 80 built-in commands becomes a Tool via `CommandToolBridge` (in auto-shell).
//!
//! See `designs/028-agent-execution-engine.md` for the full design.

pub mod error;
pub mod schema;
pub mod catalog;
pub mod agent_loop;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::pipeline::AtomPipeline;
use crate::security::SecurityPolicy;
pub use error::{DeniedReason, ErrorKind, ToolError};

/// A capability that a Tool may require. Used by the policy layer to decide
/// whether to allow execution without running the full command parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub reads_fs: bool,
    pub writes_fs: bool,
    pub spawns_process: bool,
    pub uses_network: bool,
}

/// How the tool layer should handle a policy denial.
/// External Agents default to `None` (silent deny); the built-in F4 loop
/// defaults to `Interactive` (prompt the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationMode {
    /// Silent: obey policy, return Denied on violation.
    None,
    /// On dangerous ops, surface a y/n prompt to the user (REPL only).
    Interactive,
    /// Ask the user before every tool call (paranoid mode).
    AlwaysConfirm,
}

impl Default for ConfirmationMode {
    fn default() -> Self {
        ConfirmationMode::None
    }
}

/// Hard limits to prevent an Agent from running away (e.g. cat-ing a 10GB log).
#[derive(Debug, Clone)]
pub struct OutputLimits {
    pub max_output_bytes: usize,
    pub max_command_seconds: u64,
    pub max_recursion_depth: u32,
}

impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 1_048_576, // 1 MiB
            max_command_seconds: 60,
            max_recursion_depth: 8,
        }
    }
}

/// Per-invocation context passed to every `Tool::invoke`.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub cwd: PathBuf,
    pub policy: SecurityPolicy,
    pub env: HashMap<String, String>,
    pub output_format: OutputFormat,
    pub timeout: Option<Duration>,
    pub confirmation_mode: ConfirmationMode,
    pub limits: OutputLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Atom,
    Text,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            policy: SecurityPolicy::default(),
            env: std::env::vars().collect(),
            output_format: OutputFormat::Json,
            timeout: None,
            confirmation_mode: ConfirmationMode::None,
            limits: OutputLimits::default(),
        }
    }
}

/// The data payload of a successful ToolResult.
#[derive(Debug, Clone)]
pub enum ToolData {
    /// Already-schema'd structured JSON.
    Json(Value),
    /// The ash Atom pipeline (for in-process consumers like the F4 loop).
    Atom(AtomPipeline),
    /// Plain text (legacy/external command output).
    Text(String),
    /// No data (side-effect-only commands like `mkdir`).
    Empty,
}

impl ToolData {
    /// Convert to a JSON value for the response envelope.
    pub fn into_json(self) -> Value {
        match self {
            ToolData::Json(v) => v,
            ToolData::Text(s) => Value::String(s),
            ToolData::Empty => Value::Null,
            ToolData::Atom(atom) => atom_to_json(atom),
        }
    }
}

/// Best-effort conversion of an AtomPipeline to JSON. Structured Atoms carry
/// a Value already; text/empty degrade gracefully. (Does NOT touch the
/// Atom type itself — no derive added.)
fn atom_to_json(atom: AtomPipeline) -> Value {
    match atom {
        AtomPipeline::Atom(a) => a.value.clone(),
        AtomPipeline::Text(s) => Value::String(s),
        AtomPipeline::Empty => Value::Null,
        AtomPipeline::Stream(_) | AtomPipeline::ExternalStream(_) => {
            // Streams must be collected before serialization; if not, degrade
            // to text. (Callers should collect_stream() first.)
            Value::Null
        }
    }
}

/// The outcome category of a Tool invocation.
#[derive(Debug, Clone)]
pub enum ToolStatus {
    /// Fully succeeded.
    Success,
    /// Policy denied this call. Agent should stop retrying this exact call.
    Denied(DeniedReason),
    /// Execution failed. Agent may retry or try a different approach.
    Failed(ErrorKind, String),
    /// Partially completed (e.g. dry-run, or some items in a batch failed).
    PartialSuccess(String),
}

/// Timing info, included in every ToolResult so Agents can make cost decisions.
#[derive(Debug, Clone, Default)]
pub struct Timing {
    pub wall_ms: u64,
    pub user_ms: u64,
    pub sys_ms: u64,
}

/// A non-fatal warning attached to a successful result.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
    Info,
}

/// The full result of a Tool invocation.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub status: ToolStatus,
    pub data: ToolData,
    pub diagnostics: Vec<Diagnostic>,
    pub timing: Timing,
}

impl ToolResult {
    /// Convenience: a successful result carrying JSON data.
    pub fn success_json(value: Value) -> Self {
        Self {
            status: ToolStatus::Success,
            data: ToolData::Json(value),
            diagnostics: Vec::new(),
            timing: Timing::default(),
        }
    }

    /// Convenience: a successful result carrying text.
    pub fn success_text(text: impl Into<String>) -> Self {
        Self {
            status: ToolStatus::Success,
            data: ToolData::Text(text.into()),
            diagnostics: Vec::new(),
            timing: Timing::default(),
        }
    }

    /// Convenience: a denied result.
    pub fn denied(reason: DeniedReason) -> Self {
        Self {
            status: ToolStatus::Denied(reason),
            data: ToolData::Empty,
            diagnostics: Vec::new(),
            timing: Timing::default(),
        }
    }

    /// Convenience: a failed result.
    pub fn failed(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            status: ToolStatus::Failed(kind, message.into()),
            data: ToolData::Empty,
            diagnostics: Vec::new(),
            timing: Timing::default(),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self.status, ToolStatus::Success)
    }

    pub fn is_denied(&self) -> bool {
        matches!(self.status, ToolStatus::Denied(_))
    }
}

// ──────────────────────────────────────────────────────────────────────────
// The Tool trait
// ──────────────────────────────────────────────────────────────────────────

/// A single invocable unit that an AI Agent can call.
///
/// Implementations live in two places:
/// - `auto-shell::agent::meta` — the three meta-tools (run_command, check,
///   describe_policy).
/// - `auto-shell::tool_bridge` — `CommandToolBridge<T: Command>` wraps each of
///   the 80 existing commands automatically.
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g. "ls", "grep", "run_command").
    fn name(&self) -> &str;

    /// One-line description the LLM reads to decide when to call this tool.
    fn description(&self) -> &str;

    /// Parameters as a JSON Schema object.
    /// Shape: `{"type":"object", "properties":{...}, "required":[...]}`.
    fn parameters_schema(&self) -> Map<String, Value>;

    /// Output schema (optional). None means free-form output.
    fn output_schema(&self) -> Option<Map<String, Value>> {
        None
    }

    /// Execute the tool. `args` is the parsed JSON arguments object.
    fn invoke(&self, args: &Value, ctx: &ToolContext) -> ToolResult;

    /// Capabilities this tool needs (for pre-execution policy decisions).
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_data_json_passes_through() {
        let v = json!({"a": 1});
        let d = ToolData::Json(v.clone());
        assert_eq!(d.into_json(), v);
    }

    #[test]
    fn tool_data_text_becomes_string() {
        let d = ToolData::Text("hello".into());
        assert_eq!(d.into_json(), Value::String("hello".into()));
    }

    #[test]
    fn tool_data_empty_becomes_null() {
        let d = ToolData::Empty;
        assert_eq!(d.into_json(), Value::Null);
    }

    #[test]
    fn tool_result_success_json_helper() {
        let r = ToolResult::success_json(json!([1, 2, 3]));
        assert!(r.is_success());
        assert!(matches!(r.data, ToolData::Json(_)));
    }

    #[test]
    fn tool_result_denied_helper() {
        let r = ToolResult::denied(DeniedReason::new("test-rule", "nope"));
        assert!(r.is_denied());
        assert!(!r.is_success());
    }

    #[test]
    fn output_limits_defaults_are_sane() {
        let l = OutputLimits::default();
        assert_eq!(l.max_output_bytes, 1_048_576);
        assert_eq!(l.max_command_seconds, 60);
        assert_eq!(l.max_recursion_depth, 8);
    }

    #[test]
    fn confirmation_mode_default_is_none() {
        assert_eq!(ConfirmationMode::default(), ConfirmationMode::None);
    }

    /// A minimal Tool impl for testing the trait shape end-to-end.
    struct EchoTool;
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo_test" }
        fn description(&self) -> &str { "test echo" }
        fn parameters_schema(&self) -> Map<String, Value> {
            let mut m = Map::new();
            m.insert("type".into(), "object".into());
            m.insert("properties".into(), json!({}).into());
            m
        }
        fn invoke(&self, args: &Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::success_json(args.clone())
        }
    }

    #[test]
    fn tool_trait_can_be_implemented_and_invoked() {
        let t = EchoTool;
        let ctx = ToolContext::default();
        let args = json!({"msg": "hi"});
        let result = t.invoke(&args, &ctx);
        assert!(result.is_success());
        assert_eq!(result.data.into_json(), args);
    }
}
```

- [ ] **Step 2: 创建 schema.rs 和 catalog.rs 的空骨架(让 mod.rs 编译过)**

创建 `ash-core/src/tool/schema.rs`:

```rust
//! Plan 028: JSON Schema types and signature-to-schema derivation.

use serde_json::{Map, Value};

/// A tool's self-description, as exported by `ToolRegistry::catalog()`.
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters: Map<String, Value>,
    pub output: Option<Map<String, Value>>,
    pub capabilities_json: Value,
}

impl ToolDescriptor {
    /// Serialize to the MCP-compatible `tools/list` item shape.
    pub fn to_json(&self) -> Value {
        let mut obj = Map::new();
        obj.insert("name".into(), Value::String(self.name.clone()));
        obj.insert("description".into(), Value::String(self.description.clone()));
        obj.insert("inputSchema".into(), Value::Object(self.parameters.clone()));
        if let Some(out) = &self.output {
            obj.insert("outputSchema".into(), Value::Object(out.clone()));
        }
        Value::Object(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_to_json_has_mcp_shape() {
        let mut params = Map::new();
        params.insert("type".into(), Value::String("object".into()));
        let d = ToolDescriptor {
            name: "ls".into(),
            description: "list files".into(),
            parameters: params,
            output: None,
            capabilities_json: Value::Null,
        };
        let j = d.to_json();
        let obj = j.as_object().unwrap();
        assert_eq!(obj.get("name").unwrap(), &Value::String("ls".into()));
        assert_eq!(obj.get("description").unwrap(), &Value::String("list files".into()));
        assert!(obj.get("inputSchema").is_some());
        assert!(obj.get("outputSchema").is_none()); // None omitted
    }
}
```

创建 `ash-core/src/tool/catalog.rs`:

```rust
//! Plan 028: ToolRegistry — the in-memory catalog of all Tools.

use std::collections::HashMap;
use std::sync::Arc;

use crate::tool::schema::ToolDescriptor;
use crate::tool::{Capabilities, Tool};

/// Registry of all Tools available to AI Agents.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// bash-compat aliases (e.g. "ll" -> "ls"). Resolved on lookup.
    aliases: HashMap<String, String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    /// Register a tool under its `name()`.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    /// Register an alias that resolves to a registered tool's name.
    pub fn register_alias(&mut self, alias: impl Into<String>, target: impl Into<String>) {
        self.aliases.insert(alias.into(), target.into());
    }

    /// Look up a tool by name, resolving aliases.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if let Some(t) = self.tools.get(name) {
            return Some(Arc::clone(t));
        }
        if let Some(target) = self.aliases.get(name) {
            return self.tools.get(target).cloned();
        }
        None
    }

    /// Export every tool's descriptor (full schemas). Order is unspecified.
    pub fn catalog(&self) -> Vec<ToolDescriptor> {
        self.tools
            .values()
            .map(|t| ToolDescriptor {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters_schema(),
                output: t.output_schema(),
                capabilities_json: capabilities_to_json(&t.capabilities()),
            })
            .collect()
    }

    /// Export only tool names + descriptions (no parameter schemas), for
    /// context-budget-constrained Agents.
    pub fn catalog_compact(&self) -> Vec<ToolDescriptor> {
        self.tools
            .values()
            .map(|t| ToolDescriptor {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: serde_json::Map::new(), // empty = compact
                output: None,
                capabilities_json: serde_json::Value::Null,
            })
            .collect()
    }

    /// Number of registered tools (excluding aliases).
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// All registered tool names (excluding aliases), sorted for stable output.
    pub fn names_sorted(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn capabilities_to_json(caps: &Capabilities) -> serde_json::Value {
    serde_json::json!({
        "reads_fs": caps.reads_fs,
        "writes_fs": caps.writes_fs,
        "spawns_process": caps.spawns_process,
        "uses_network": caps.uses_network,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{ToolContext, ToolResult};
    use serde_json::json;

    struct DummyTool {
        nm: &'static str,
        desc: &'static str,
    }
    impl Tool for DummyTool {
        fn name(&self) -> &str { self.nm }
        fn description(&self) -> &str { self.desc }
        fn parameters_schema(&self) -> serde_json::Map<String, serde_json::Value> {
            let mut m = serde_json::Map::new();
            m.insert("type".into(), json!("object"));
            m
        }
        fn invoke(&self, _a: &serde_json::Value, _c: &ToolContext) -> ToolResult {
            ToolResult::success_json(json!({}))
        }
    }

    #[test]
    fn register_and_get_by_name() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "ls", desc: "list" }));
        assert!(reg.get("ls").is_some());
        assert!(reg.get("missing").is_none());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn alias_resolves_to_target() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "ls", desc: "list" }));
        reg.register_alias("ll", "ls");
        assert!(reg.get("ll").is_some());
        assert_eq!(reg.get("ll").unwrap().name(), "ls");
    }

    #[test]
    fn catalog_has_full_schema() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "ls", desc: "list" }));
        let cat = reg.catalog();
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].name, "ls");
        assert!(!cat[0].parameters.is_empty());
    }

    #[test]
    fn catalog_compact_omits_schema() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "ls", desc: "list" }));
        let cat = reg.catalog_compact();
        assert!(cat[0].parameters.is_empty());
    }

    #[test]
    fn names_sorted_returns_sorted_list() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(DummyTool { nm: "zeta", desc: "" }));
        reg.register(Arc::new(DummyTool { nm: "alpha", desc: "" }));
        assert_eq!(reg.names_sorted(), vec!["alpha", "zeta"]);
    }
}
```

创建 `ash-core/src/tool/agent_loop.rs`:

```rust
//! Plan 028: Interface reservation for the built-in F4 agent loop.
//!
//! This module defines ONLY traits — no implementation. The actual agent
//! loop (LLM polling → tool call → execute → refill) is Plan 029+.
//! By defining the contract here, Plan 029 can consume the ToolRegistry
//! without restructuring.

use serde_json::Value;

use crate::tool::{ToolContext, ToolResult};

/// The LLM provider whose tool-call format we're serializing for.
/// Different providers have subtly different tool-spec shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    /// Anthropic tool_use format.
    Anthropic,
    /// OpenAI function-calling format.
    OpenAI,
    /// MCP tools/list format (also our CLI catalog format).
    Mcp,
    /// Generic JSON Schema (no provider-specific wrapping).
    Generic,
}

/// Contract between a future F4 agent loop and the ToolRegistry.
/// Implemented by `ToolRegistry` (added in a later task).
pub trait ToolExecuting {
    /// Execute one tool call: validate args, check policy, run, return result.
    fn execute_tool_call(
        &self,
        tool_name: &str,
        arguments: &Value,
        ctx: &ToolContext,
    ) -> ToolResult;

    /// Export the full catalog in a provider-specific tool-spec format.
    fn export_for_provider(&self, provider: LlmProvider) -> Vec<Value>;
}
```

- [ ] **Step 3: 更新 mod.rs 的 pub mod 声明(补上 schema/catalog/agent_loop)**

确认 `ash-core/src/tool/mod.rs` 顶部已有:

```rust
pub mod error;
pub mod schema;
pub mod catalog;
pub mod agent_loop;
```

(已在 Step 1 写入。)

- [ ] **Step 4: 运行所有 tool 模块测试**

Run: `cargo test -p ash-core tool::`
Expected: PASS —— mod 的 7 个 + schema 的 1 个 + catalog 的 5 个 = 13 个测试全过。

- [ ] **Step 5: Commit**

```bash
git add ash-core/src/tool/
git commit -m "feat(tool): add Tool trait, ToolRegistry, ToolContext, ToolResult (Plan 028 M1.3)"
```

---

## Task M1.4:Schema 推导 —— 从 Signature 到 JSON Schema

**背景**:本 task 处理"自动从现有 `Command::signature()` 推导出 `parameters_schema()`"的逻辑。但 `Signature` 在 `auto-shell`(`cmd.rs`),不在 `ash-core`。所以推导函数也要放 `auto-shell`。

**Files:**
- Create: `ash/auto-shell/src/tool_bridge.rs`(新模块,放推导 + bridge)
- Modify: `ash/auto-shell/src/lib.rs`(加 `mod tool_bridge;`)

- [ ] **Step 1: 先确认 lib.rs 的模块声明结构**

Run: `head -30 ash/auto-shell/src/lib.rs`
查看现有 `pub mod` / `mod` 声明的风格,确认在哪里插入新模块。

- [ ] **Step 2: 写推导函数的失败测试**

创建 `ash/auto-shell/src/tool_bridge.rs`,先放测试:

```rust
//! Plan 028: Bridge the existing `Command` trait to the new `Tool` trait.
//!
//! `CommandToolBridge<T>` wraps any `T: Command` and implements `Tool` by
//! delegating to `run()` / `run_atom()`. The parameters schema is auto-derived
//! from `Command::signature()`.
//!
//! Lives in `auto-shell` (not `ash-core`) because it needs to see both
//! `Command` (defined here) and `Tool` (defined in ash-core). The dependency
//! direction is correct: auto-shell → ash-core.

use std::sync::Arc;

use ash_core::tool::{
    Capabilities, OutputFormat, Tool, ToolContext, ToolData, ToolResult, ToolStatus,
};
use serde_json::{Map, Value};

use crate::cmd::{Argument, Command, Signature};

/// Derive a minimal JSON Schema from a `Signature`.
///
/// Maps the Signature's argument list to an `{"type":"object", ...}` schema:
/// - required positionals → required string properties
/// - optional positionals → optional string properties (with default if set)
/// - flags → boolean properties (default false)
/// - options (--name VALUE) → string properties
pub fn derive_schema_from_signature(sig: &Signature) -> Map<String, Value> {
    let mut properties = Map::new();
    let mut required: Vec<String> = Vec::new();

    for arg in &sig.arguments {
        let (ty, default) = if arg.is_flag {
            ("boolean".to_string(), Value::Bool(false))
        } else {
            ("string".to_string(), Value::Null)
        };

        let mut prop = Map::new();
        prop.insert("type".into(), Value::String(ty));
        prop.insert(
            "description".into(),
            Value::String(arg.description.clone()),
        );
        if let Some(d) = &arg.default {
            prop.insert("default".into(), Value::String(d.clone()));
        }
        properties.insert(arg.name.clone(), Value::Object(prop));

        if arg.required && !arg.is_flag {
            required.push(arg.name.clone());
        }
        let _ = default; // (default unused in minimal schema beyond above)
    }

    let mut schema = Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert(
            "required".into(),
            Value::Array(required.into_iter().map(Value::String).collect()),
        );
    }
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::Signature;

    #[test]
    fn empty_signature_yields_empty_object_schema() {
        let sig = Signature::new("noop", "does nothing");
        let schema = derive_schema_from_signature(&sig);
        assert_eq!(schema.get("type").unwrap(), &Value::String("object".into()));
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.is_empty());
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn required_positional_becomes_required_string() {
        let sig = Signature::new("cat", "concatenate")
            .required("file", "the file to read");
        let schema = derive_schema_from_signature(&sig);
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert_eq!(props.get("file").unwrap().get("type").unwrap(), &Value::String("string".into()));
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], Value::String("file".into()));
    }

    #[test]
    fn flag_becomes_boolean_not_required() {
        let sig = Signature::new("ls", "list")
            .flag("all", "show hidden");
        let schema = derive_schema_from_signature(&sig);
        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert_eq!(props.get("all").unwrap().get("type").unwrap(), &Value::String("boolean".into()));
        assert!(schema.get("required").is_none()); // flags never required
    }

    #[test]
    fn optional_with_default_includes_default() {
        let sig = Signature::new("x", "x")
            .optional_default("count", "10", "number of items");
        let schema = derive_schema_from_signature(&sig);
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let count_prop = props.get("count").unwrap().as_object().unwrap();
        assert_eq!(count_prop.get("default").unwrap(), &Value::String("10".into()));
        assert!(schema.get("required").is_none());
    }
}
```

- [ ] **Step 3: 在 lib.rs 注册模块**

修改 `ash/auto-shell/src/lib.rs`,在合适位置(其他 `mod` 声明附近)加:

```rust
pub mod tool_bridge;
```

(若该文件用 `mod xxx;` 而非 `pub mod`,跟随现有风格。)

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p auto-shell tool_bridge::`
Expected: PASS —— 4 个测试全过。

- [ ] **Step 5: Commit**

```bash
git add ash/auto-shell/src/tool_bridge.rs ash/auto-shell/src/lib.rs
git commit -m "feat(tool): add derive_schema_from_signature (Plan 028 M1.4)"
```

---

## Task M1.5:CommandToolBridge —— 让 Command 成为 Tool

**Files:**
- Modify: `ash/auto-shell/src/tool_bridge.rs`(追加 bridge)
- Test: 内联

- [ ] **Step 1: 追加 bridge 实现和测试**

在 `tool_bridge.rs` 末尾(`mod tests` 之前)追加:

```rust
// ──────────────────────────────────────────────────────────────────────────
// CommandToolBridge
// ──────────────────────────────────────────────────────────────────────────

/// Wrap any `Command` so it satisfies the `Tool` trait.
///
/// The bridge derives:
/// - `name()` / `description()` from `signature()`
/// - `parameters_schema()` via `derive_schema_from_signature()`
/// - `invoke()` — but this needs a `Shell` to run against, which the Tool
///   interface doesn't carry. So the bridge's `invoke()` returns an
///   `Internal` error with a hint; the real execution path is
///   `invoke_with_shell()` (used by the agent run command).
pub struct CommandToolBridge<T: Command> {
    pub inner: T,
}

impl<T: Command> CommandToolBridge<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Command + 'static> Tool for CommandToolBridge<T> {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.signature().description.as_str()
    }

    fn parameters_schema(&self) -> Map<String, Value> {
        derive_schema_from_signature(&self.inner.signature())
    }

    fn invoke(&self, _args: &Value, _ctx: &ToolContext) -> ToolResult {
        // The plain Tool interface is shell-less. Real execution requires a
        // Shell reference (to pass to Command::run). The agent CLI path uses
        // `invoke_with_shell()` instead. This stub exists so the bridge can be
        // registered in a ToolRegistry and introspected (catalog/describe).
        ToolResult::failed(
            ash_core::tool::ErrorKind::Internal,
            "CommandToolBridge.invoke() called without a Shell; \
             use invoke_with_shell() via the agent run path",
        )
    }
}
```

在 `mod tests` 里追加 bridge 测试:

```rust
    // ── bridge tests ──
    use ash_core::tool::Tool;
    use crate::cmd::parser::ParsedArgs;
    use crate::cmd::PipelineData;
    use crate::shell::Shell;
    use miette::Result;

    /// Minimal Command for testing the bridge.
    struct PingCommand;
    impl Command for PingCommand {
        fn name(&self) -> &str { "ping_test" }
        fn signature(&self) -> Signature {
            Signature::new("ping_test", "test ping command")
                .required("target", "host to ping")
                .flag("verbose", "verbose output")
        }
        fn run(
            &self,
            _args: &ParsedArgs,
            _input: PipelineData,
            _shell: &mut Shell,
        ) -> Result<PipelineData> {
            Ok(PipelineData::Text("pong".into()))
        }
    }

    #[test]
    fn bridge_satisfies_tool_trait() {
        let bridge = CommandToolBridge::new(PingCommand);
        let tool: &dyn Tool = &bridge;
        assert_eq!(tool.name(), "ping_test");
        assert_eq!(tool.description(), "test ping command");
    }

    #[test]
    fn bridge_derives_schema_from_command_signature() {
        let bridge = CommandToolBridge::new(PingCommand);
        let tool: &dyn Tool = &bridge;
        let schema = tool.parameters_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        // target (required string) + verbose (flag → boolean)
        assert!(props.contains_key("target"));
        assert!(props.contains_key("verbose"));
        assert_eq!(
            props.get("verbose").unwrap().get("type").unwrap(),
            &Value::String("boolean".into())
        );
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert_eq!(required[0], Value::String("target".into()));
    }

    #[test]
    fn bridge_invoke_without_shell_returns_internal_error() {
        let bridge = CommandToolBridge::new(PingCommand);
        let tool: &dyn Tool = &bridge;
        let ctx = ToolContext::default();
        let result = tool.invoke(&Value::Null, &ctx);
        match result.status {
            ToolStatus::Failed(ash_core::tool::ErrorKind::Internal, _) => {}
            other => panic!("expected Failed(Internal), got {:?}", other),
        }
    }
```

并确认 imports 区已包含 `ToolStatus`、`ToolContext`(M1.4 的 use 块里已有 `Tool`、`ToolContext`、`ToolResult`,这里加 `ToolStatus`)。

更新 `tool_bridge.rs` 顶部的 use:

```rust
use ash_core::tool::{
    Capabilities, OutputFormat, Tool, ToolContext, ToolData, ToolResult, ToolStatus,
};
```

(`Capabilities`、`OutputFormat`、`ToolData` 暂时 unused,会有 warning,可加 `#[allow(unused_imports)]` 或在 M2 用到时移除。简单起见保留。)

- [ ] **Step 2: 运行测试验证**

Run: `cargo test -p auto-shell tool_bridge::`
Expected: PASS —— 4(已有)+ 3(新增)= 7 个测试全过。

- [ ] **Step 3: Commit**

```bash
git add ash/auto-shell/src/tool_bridge.rs
git commit -m "feat(tool): add CommandToolBridge<T: Command> (Plan 028 M1.5)"
```

---

## Task M1.6:从 Shell 构建 ToolRegistry(桥接全部 80 命令)

**Files:**
- Modify: `ash/auto-shell/src/shell.rs`(加 `build_tool_registry()` 方法)
- Modify: `ash/auto-shell/src/tool_bridge.rs`(加桥接辅助)
- Test: 集成测试 `ash/auto-shell/tests/tool_registry_build.rs`

- [ ] **Step 1: 写集成测试(失败)**

创建 `ash/auto-shell/tests/tool_registry_build.rs`:

```rust
//! Plan 028 M1.6: Verify all ~80 commands can be bridged into a ToolRegistry.

use auto_shell::Shell;
use ash_core::tool::catalog::ToolRegistry;

#[test]
fn shell_builds_tool_registry_with_all_commands() {
    let shell = Shell::new();
    let registry: ToolRegistry = shell.build_tool_registry();
    // We register ~80 commands; assert a healthy lower bound. If commands
    // are removed in future, update this number deliberately.
    assert!(
        registry.len() >= 70,
        "expected >=70 bridged tools, got {}",
        registry.len()
    );
}

#[test]
fn registry_contains_core_commands() {
    let shell = Shell::new();
    let registry = shell.build_tool_registry();
    for name in ["ls", "cat", "grep", "find", "rm", "cp", "mv", "mkdir", "echo", "pwd"] {
        assert!(
            registry.get(name).is_some(),
            "expected tool '{}' in registry",
            name
        );
    }
}

#[test]
fn every_bridged_tool_has_valid_schema() {
    use ash_core::tool::Tool;
    let shell = Shell::new();
    let registry = shell.build_tool_registry();
    let catalog = registry.catalog();
    assert!(!catalog.is_empty());
    for desc in &catalog {
        // Every descriptor must have a name, description, and an object schema.
        assert!(!desc.name.is_empty(), "tool with empty name");
        assert!(!desc.description.is_empty(), "tool {} has empty description", desc.name);
        assert_eq!(
            desc.parameters.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "tool {} parameters schema is not type=object",
            desc.name
        );
    }
}

#[test]
fn catalog_compact_omits_schemas() {
    let shell = Shell::new();
    let registry = shell.build_tool_registry();
    let compact = registry.catalog_compact();
    assert!(compact.len() >= 70);
    for desc in &compact {
        assert!(desc.parameters.is_empty(), "compact catalog entry {} has schema", desc.name);
    }
}
```

- [ ] **Step 2: 运行测试(失败,因为 build_tool_registry 不存在)**

Run: `cargo test -p auto-shell --test tool_registry_build`
Expected: FAIL —— `no method named build_tool_registry found`.

- [ ] **Step 3: 在 tool_bridge.rs 加批量桥接函数**

在 `tool_bridge.rs` 末尾(`mod tests` 之前)追加:

```rust
use ash_core::tool::catalog::ToolRegistry;

/// Build a ToolRegistry by bridging every command in a CommandRegistry.
///
/// Each `Arc<dyn Command>` is wrapped in `CommandToolBridge` and registered.
/// The result is a ToolRegistry whose catalog() can be exported for Agents.
pub fn build_tool_registry_from_commands(
    commands: &crate::cmd::CommandRegistry,
) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    for sig in commands.params() {
        let name = sig.name.clone();
        if let Some(cmd) = commands.get(&name) {
            // Bridge needs a concrete type, but we only have a trait object.
            // Workaround: register a dynamic adapter that holds the Arc.
            let bridged = DynamicCommandTool::new(name.clone(), cmd);
            tools.register(std::sync::Arc::from(bridged));
        }
    }
    tools
}

// ──────────────────────────────────────────────────────────────────────────
// DynamicCommandTool — a Tool backed by an Arc<dyn Command> (not a generic T)
// ──────────────────────────────────────────────────────────────────────────

/// Tool wrapper around a trait-object `Command`, so we can bridge all 80
/// commands into one ToolRegistry without monomorphizing 80 types.
///
/// Like `CommandToolBridge`, its `invoke()` is shell-less and returns Internal;
/// the real execution path is the agent CLI's `run` command (M2).
pub struct DynamicCommandTool {
    name: String,
    description: String,
    parameters: Map<String, Value>,
    cmd: Arc<dyn Command>,
}

impl DynamicCommandTool {
    pub fn new(name: String, cmd: Arc<dyn Command>) -> Box<Self> {
        let sig = cmd.signature();
        let description = sig.description.clone();
        let parameters = derive_schema_from_signature(&sig);
        Box::new(Self {
            name,
            description,
            parameters,
            cmd,
        })
    }
}

impl Tool for DynamicCommandTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters_schema(&self) -> Map<String, Value> {
        self.parameters.clone()
    }
    fn invoke(&self, _args: &Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult::failed(
            ash_core::tool::ErrorKind::Internal,
            "DynamicCommandTool.invoke() called without a Shell; use the agent run path",
        )
    }
}
```

- [ ] **Step 4: 给 Shell 加 registry 访问器 + build_tool_registry 方法**

在 `ash/auto-shell/src/shell.rs`,先找到 `impl Shell {` 块里 `set_policy` 附近(line ~411),追加两个方法。

先读取确认 registry 的可见性:

Run: `grep -n "registry" ash/auto-shell/src/shell.rs | head -10`

确认 `registry` 是私有字段。需要加一个借用访问器。

在 `set_policy` 方法之后追加:

```rust
    /// Plan 028: Borrow the command registry (for building the ToolRegistry).
    pub fn registry(&self) -> &CommandRegistry {
        &self.registry
    }

    /// Plan 028: Build a ToolRegistry bridging all registered commands.
    /// Each command becomes a Tool via DynamicCommandTool. The resulting
    /// registry's catalog() can be exported for AI Agents.
    pub fn build_tool_registry(&self) -> ash_core::tool::catalog::ToolRegistry {
        crate::tool_bridge::build_tool_registry_from_commands(&self.registry)
    }
```

- [ ] **Step 5: 运行测试验证**

Run: `cargo test -p auto-shell --test tool_registry_build`
Expected: PASS —— 4 个测试全过。如果 `len() >= 70` 断言失败,把 70 改成实际数字的合理下限(当前注册约 80 个,留余量)。

- [ ] **Step 6: 全量编译检查**

Run: `cargo build -p auto-shell`
Expected: 编译成功(warning 可接受,error 不行)。

- [ ] **Step 7: Commit**

```bash
git add ash/auto-shell/src/tool_bridge.rs ash/auto-shell/src/shell.rs ash/auto-shell/tests/tool_registry_build.rs
git commit -m "feat(tool): bridge all 80 commands into ToolRegistry (Plan 028 M1.6)"
```

---

## Task M1.7:AtomType → kind 字符串映射

**背景**:结构化输出信封需要把 `AtomType` 暴露成 `kind` 字符串。不能给 `AtomType` 加 derive(约束),所以写一个显式映射函数。

**Files:**
- Create: `ash-core/src/tool/atom_kind.rs`
- Modify: `ash-core/src/tool/mod.rs`(加 `pub mod atom_kind;`)

- [ ] **Step 1: 写映射函数 + 测试**

创建 `ash-core/src/tool/atom_kind.rs`:

```rust
//! Plan 028: Map AtomType → snake_case kind string for the response envelope.
//!
//! We deliberately do NOT add Serialize to AtomType itself (to avoid touching
//! existing pipeline code). Instead this module is the single source of truth
//! for the kind label of each AtomType.

use crate::pipeline::AtomType;

/// Return the stable snake_case kind label for an AtomType.
///
/// These strings appear in the response envelope's `data.kind` field and are
/// part of the Agent-facing contract — do NOT rename without bumping
/// schema_version.
pub fn atom_type_to_kind(t: AtomType) -> &'static str {
    match t {
        AtomType::FileEntry => "file_entry",
        AtomType::FileList => "file_list",
        AtomType::ProcessEntry => "process_entry",
        AtomType::ProcessList => "process_list",
        AtomType::DiskEntry => "disk_entry",
        AtomType::CpuInfo => "cpu_info",
        AtomType::MemoryInfo => "memory_info",
        AtomType::SystemInfo => "system_info",
        AtomType::MatchList => "match_list",
        AtomType::CountResult => "count_result",
        AtomType::Table => "table",
        AtomType::Record => "record",
        AtomType::Text => "text",
        AtomType::Path => "path",
        AtomType::BuildResult => "build_result",
        AtomType::RunResult => "run_result",
        AtomType::HelpInfo => "help_info",
        AtomType::Nothing => "empty",
    }
}

/// The AtomType name as it appears in `data.atom_type` (PascalCase, matches
/// the Rust enum variant). Kept distinct from `kind` (snake_case) so Agents
/// can reference either.
pub fn atom_type_name(t: AtomType) -> &'static str {
    match t {
        AtomType::FileEntry => "FileEntry",
        AtomType::FileList => "FileList",
        AtomType::ProcessEntry => "ProcessEntry",
        AtomType::ProcessList => "ProcessList",
        AtomType::DiskEntry => "DiskEntry",
        AtomType::CpuInfo => "CpuInfo",
        AtomType::MemoryInfo => "MemoryInfo",
        AtomType::SystemInfo => "SystemInfo",
        AtomType::MatchList => "MatchList",
        AtomType::CountResult => "CountResult",
        AtomType::Table => "Table",
        AtomType::Record => "Record",
        AtomType::Text => "Text",
        AtomType::Path => "Path",
        AtomType::BuildResult => "BuildResult",
        AtomType::RunResult => "RunResult",
        AtomType::HelpInfo => "HelpInfo",
        AtomType::Nothing => "Nothing",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_atom_types_have_kind_mappings() {
        // Every variant must map to a non-empty snake_case string.
        let all = [
            AtomType::FileEntry,
            AtomType::FileList,
            AtomType::ProcessEntry,
            AtomType::ProcessList,
            AtomType::DiskEntry,
            AtomType::CpuInfo,
            AtomType::MemoryInfo,
            AtomType::SystemInfo,
            AtomType::MatchList,
            AtomType::CountResult,
            AtomType::Table,
            AtomType::Record,
            AtomType::Text,
            AtomType::Path,
            AtomType::BuildResult,
            AtomType::RunResult,
            AtomType::HelpInfo,
            AtomType::Nothing,
        ];
        for t in all {
            let k = atom_type_to_kind(t);
            assert!(!k.is_empty(), "empty kind for {:?}", t);
            assert!(
                !k.chars().any(|c| c.is_uppercase()),
                "kind {:?} has uppercase (must be snake_case): {}",
                t,
                k
            );
        }
    }

    #[test]
    fn kind_is_stable_string() {
        assert_eq!(atom_type_to_kind(AtomType::FileList), "file_list");
        assert_eq!(atom_type_to_kind(AtomType::ProcessList), "process_list");
        assert_eq!(atom_type_to_kind(AtomType::Table), "table");
        assert_eq!(atom_type_to_kind(AtomType::Nothing), "empty");
    }

    #[test]
    fn atom_type_name_is_pascal() {
        assert_eq!(atom_type_name(AtomType::FileList), "FileList");
        assert_eq!(atom_type_name(AtomType::Nothing), "Nothing");
    }
}
```

- [ ] **Step 2: 在 mod.rs 注册模块**

在 `ash-core/src/tool/mod.rs` 的 `pub mod` 列表加:

```rust
pub mod atom_kind;
```

(放在 `pub mod agent_loop;` 之后。)

- [ ] **Step 3: 运行测试**

Run: `cargo test -p ash-core atom_kind`
Expected: PASS —— 3 个测试全过。

- [ ] **Step 4: Commit**

```bash
git add ash-core/src/tool/atom_kind.rs ash-core/src/tool/mod.rs
git commit -m "feat(tool): add AtomType -> kind string mapping (Plan 028 M1.7)"
```

---

## Task M1.8:M1 完成验收

- [ ] **Step 1: 跑 ash-core 全量测试**

Run: `cargo test -p ash-core`
Expected: 全部 PASS(含原有测试 + 新增 tool 模块测试)。

- [ ] **Step 2: 跑 auto-shell 全量测试**

Run: `cargo test -p auto-shell`
Expected: 全部 PASS。

- [ ] **Step 3: 确认 clippy 干净**

Run: `cargo clippy -p ash-core -p auto-shell -- -D warnings`
Expected: 无 warning(若有,修复或加 `#[allow]` 并说明理由)。

- [ ] **Step 4: Commit M1 完成标记**

```bash
git commit --allow-empty -m "chore(028): M1 complete — Tool Registry skeleton + 80-command bridge"
```

---

# 里程碑 M2:CLI Agent 接口 + 结构化输出

**目标**:外部 Agent 能通过 `ash agent ...` 子命令完整调用 ash,拿到稳定的信封输出。

**完成标准**:`ash agent describe-tools` / `describe-policy` / `check` / `run` 四个子命令可用,端到端测试通过。

---

## Task M2.1:响应信封序列化

**Files:**
- Create: `ash-core/src/tool/envelope.rs`
- Modify: `ash-core/src/tool/mod.rs`(加 `pub mod envelope;`)

- [ ] **Step 1: 写信封类型 + 测试**

创建 `ash-core/src/tool/envelope.rs`:

```rust
//! Plan 028: The response envelope returned by `ash agent run`.
//!
//! Shape (see designs/028 §4.2):
//! ```json
//! {
//!   "schema_version": "1",
//!   "status": "success" | "failed" | "denied" | "partial",
//!   "data": { "kind": "...", "atom_type": "...", "value": ...,
//!             "pipeline_hint": "...", "truncation": {...} },
//!   "diagnostics": [...],
//!   "timing": { "wall_ms": ..., "user_ms": ..., "sys_ms": ... },
//!   "command_echo": "..."
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pipeline::AtomType;
use crate::tool::atom_kind::{atom_type_name, atom_type_to_kind};
use crate::tool::{DeniedReason, Diagnostic, ErrorKind, Timing, ToolError, ToolStatus};

pub const ENVELOPE_SCHEMA_VERSION: &str = "1";

/// Truncation info attached when output exceeded `max_output_bytes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Truncation {
    pub truncated: bool,
    pub original_bytes: usize,
    pub returned_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_hint: Option<String>,
}

/// The `data` block of a success response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeData {
    /// snake_case semantic kind (e.g. "file_list").
    pub kind: String,
    /// PascalCase AtomType name (e.g. "FileList").
    pub atom_type: String,
    /// The actual payload.
    pub value: Value,
    /// Hint about what DSL ops this output can be piped into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,
}

impl EnvelopeData {
    /// Build the data block from an AtomType and its JSON value.
    pub fn from_atom(atom_type: AtomType, value: Value) -> Self {
        Self {
            kind: atom_type_to_kind(atom_type).to_string(),
            atom_type: atom_type_name(atom_type).to_string(),
            value,
            pipeline_hint: pipeline_hint_for(atom_type),
            truncation: None,
        }
    }

    /// Plain-text data block (kind="text").
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            kind: "text".to_string(),
            atom_type: "Text".to_string(),
            value: Value::String(content.into()),
            pipeline_hint: Some("pipeable to grep/head/tail/wc".to_string()),
            truncation: None,
        }
    }
}

fn pipeline_hint_for(t: AtomType) -> Option<String> {
    match t {
        AtomType::FileList | AtomType::FileEntry => {
            Some("pipeable to filter/sort/select (e.g. filter .size > 1k)".into())
        }
        AtomType::ProcessList | AtomType::ProcessEntry => {
            Some("pipeable to filter/sort/select (e.g. filter .cpu > 1.0)".into())
        }
        AtomType::Table => Some("pipeable to select <cols> or to_csv".into()),
        AtomType::Record => Some("pipeable to get <field>".into()),
        AtomType::Text => Some("pipeable to grep/head/tail/wc".into()),
        _ => None,
    }
}

/// A diagnostic in the envelope (warning/info attached to a result).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeDiagnostic {
    pub level: String, // "warning" | "info"
    pub message: String,
}

impl From<&Diagnostic> for EnvelopeDiagnostic {
    fn from(d: &Diagnostic) -> Self {
        Self {
            level: match d.level {
                crate::tool::DiagnosticLevel::Warning => "warning",
                crate::tool::DiagnosticLevel::Info => "info",
            }
            .to_string(),
            message: d.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeTiming {
    pub wall_ms: u64,
    pub user_ms: u64,
    pub sys_ms: u64,
}

impl From<&Timing> for EnvelopeTiming {
    fn from(t: &Timing) -> Self {
        Self {
            wall_ms: t.wall_ms,
            user_ms: t.user_ms,
            sys_ms: t.sys_ms,
        }
    }
}

/// The top-level status string in the envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeStatus {
    Success,
    Failed,
    Denied,
    Partial,
}

impl EnvelopeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            EnvelopeStatus::Success => "success",
            EnvelopeStatus::Failed => "failed",
            EnvelopeStatus::Denied => "denied",
            EnvelopeStatus::Partial => "partial",
        }
    }
}

/// Build the final envelope JSON from a ToolResult + command echo.
///
/// This is the single function that turns an in-process ToolResult into the
/// wire format Agents consume.
pub fn build_envelope(result: &crate::tool::ToolResult, command_echo: &str) -> Value {
    use serde_json::json;

    let (status, data_block, error_block) = match &result.status {
        ToolStatus::Success => (
            EnvelopeStatus::Success,
            result.data.clone().into_json(),
            None,
        ),
        ToolStatus::PartialSuccess(msg) => (
            EnvelopeStatus::Partial,
            result.data.clone().into_json(),
            Some(json!({ "partial_message": msg })),
        ),
        ToolStatus::Denied(reason) => (
            EnvelopeStatus::Denied,
            Value::Null,
            Some(serde_json::to_value(reason).unwrap_or(Value::Null)),
        ),
        ToolStatus::Failed(kind, msg) => {
            let err = ToolError::new(*kind, msg, command_echo);
            (
                EnvelopeStatus::Failed,
                Value::Null,
                Some(serde_json::to_value(&err).unwrap_or(Value::Null)),
            )
        }
    };

    // Wrap the data block in the {kind, atom_type, value} structure when present.
    let data_field = if status == EnvelopeStatus::Success || status == EnvelopeStatus::Partial {
        // Heuristic: if data_block is already an object with "kind", pass through;
        // otherwise wrap as text. (The run command will usually pre-build EnvelopeData.)
        match &result.data {
            crate::tool::ToolData::Json(v) => {
                if v.get("kind").and_then(|k| k.as_str()).is_some() {
                    v.clone()
                } else {
                    // Wrap raw JSON value as a generic record.
                    serde_json::to_value(EnvelopeData::from_atom(
                        crate::pipeline::AtomType::Record,
                        v.clone(),
                    ))
                    .unwrap_or(Value::Null)
                }
            }
            crate::tool::ToolData::Text(s) => {
                serde_json::to_value(EnvelopeData::text(s)).unwrap_or(Value::Null)
            }
            _ => Value::Null,
        }
    } else {
        Value::Null
    };

    json!({
        "schema_version": ENVELOPE_SCHEMA_VERSION,
        "status": status.as_str(),
        "data": data_field,
        "error": error_block,
        "diagnostics": result.diagnostics.iter().map(EnvelopeDiagnostic::from).collect::<Vec<_>>(),
        "timing": serde_json::to_value(EnvelopeTiming::from(&result.timing)).unwrap_or(Value::Null),
        "command_echo": command_echo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{DeniedReason, ToolData, ToolResult, ToolStatus};
    use serde_json::json;

    #[test]
    fn success_envelope_has_schema_version_and_status() {
        let r = ToolResult::success_text("hello");
        let env = build_envelope(&r, "echo hello");
        assert_eq!(env["schema_version"], "1");
        assert_eq!(env["status"], "success");
        assert_eq!(env["data"]["kind"], "text");
        assert_eq!(env["data"]["value"], "hello");
        assert_eq!(env["command_echo"], "echo hello");
        assert!(env["error"].is_null());
    }

    #[test]
    fn denied_envelope_has_denied_reason() {
        let r = ToolResult::denied(
            DeniedReason::new("path-outside-sandbox", "denied")
                .with_remediation("use /sandbox/x"),
        );
        let env = build_envelope(&r, "rm /etc/foo");
        assert_eq!(env["status"], "denied");
        assert_eq!(env["error"]["rule_id"], "path-outside-sandbox");
        assert_eq!(env["error"]["remediation"], "use /sandbox/x");
        assert!(env["data"].is_null());
    }

    #[test]
    fn failed_envelope_has_error_kind() {
        let r = ToolResult::failed(ErrorKind::NotFound, "no such file");
        let env = build_envelope(&r, "cat missing.txt");
        assert_eq!(env["status"], "failed");
        assert_eq!(env["error"]["kind"], "not_found");
        assert_eq!(env["error"]["message"], "no such file");
        assert_eq!(env["error"]["command"], "cat missing.txt");
    }

    #[test]
    fn envelope_data_from_atom_sets_kind_and_atom_type() {
        let d = EnvelopeData::from_atom(AtomType::FileList, json!([{"name": "a"}]));
        assert_eq!(d.kind, "file_list");
        assert_eq!(d.atom_type, "FileList");
        assert!(d.pipeline_hint.is_some());
    }

    #[test]
    fn envelope_data_text_has_hint() {
        let d = EnvelopeData::text("body");
        assert_eq!(d.kind, "text");
        assert!(d.pipeline_hint.unwrap().contains("grep"));
    }
}
```

- [ ] **Step 2: 在 mod.rs 注册模块**

在 `ash-core/src/tool/mod.rs` 的 `pub mod` 列表加:

```rust
pub mod envelope;
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p ash-core envelope`
Expected: PASS —— 5 个测试全过。

- [ ] **Step 4: Commit**

```bash
git add ash-core/src/tool/envelope.rs ash-core/src/tool/mod.rs
git commit -m "feat(tool): add response envelope serialization (Plan 028 M2.1)"
```

---

## Task M2.2:SecurityPolicy 摘要方法

**Files:**
- Modify: `ash-core/src/security.rs`(加 `summarize()` 方法)

- [ ] **Step 1: 写摘要方法 + 测试**

在 `ash-core/src/security.rs` 的 `impl SecurityPolicy` 块内(在 `audit()` 方法之后)追加:

```rust
    /// Plan 028: Produce a capability-only summary for Agents.
    ///
    /// Deliberately does NOT include specific paths (sandbox_dir is shown
    /// only as a boolean "is sandboxed"). This is safe to surface in system
    /// prompts / logs.
    pub fn summarize(&self) -> PolicySummary {
        PolicySummary {
            has_allow_list: !self.allow.is_empty(),
            deny_count: self.deny.len(),
            no_exec: self.no_exec,
            no_network: self.no_network,
            read_only: self.read_only,
            dry_run: self.dry_run,
            sandboxed: self.sandbox_dir.is_some(),
            audit_enabled: self.audit_file.is_some(),
        }
    }
```

在文件末尾(`AuditRecord` 之后,或在合适位置)追加 `PolicySummary` 结构:

```rust
/// Plan 028: A capability-only summary of a SecurityPolicy, safe to expose
/// to Agents (no specific paths or deny-list contents).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PolicySummary {
    pub has_allow_list: bool,
    pub deny_count: usize,
    pub no_exec: bool,
    pub no_network: bool,
    pub read_only: bool,
    pub dry_run: bool,
    pub sandboxed: bool,
    pub audit_enabled: bool,
}
```

在 `#[cfg(test)] mod tests`(若 security.rs 已有则追加,否则新建)加测试:

```rust
    #[test]
    fn plan028_summarize_default_policy() {
        let p = crate::security::SecurityPolicy::default();
        let s = p.summarize();
        assert!(!s.has_allow_list);
        assert_eq!(s.deny_count, 0);
        assert!(!s.no_exec);
        assert!(!s.no_network);
        assert!(!s.read_only);
        assert!(!s.dry_run);
        assert!(!s.sandboxed);
        assert!(!s.audit_enabled);
    }

    #[test]
    fn plan028_summarize_locked_down_policy() {
        let mut p = crate::security::SecurityPolicy::default();
        p.no_exec = true;
        p.no_network = true;
        p.read_only = true;
        p.sandbox_dir = Some(std::path::PathBuf::from("/sandbox"));
        p.deny.push("rm".into());
        let s = p.summarize();
        assert!(s.no_exec);
        assert!(s.no_network);
        assert!(s.read_only);
        assert!(s.sandboxed);
        assert_eq!(s.deny_count, 1);
        // sandbox_dir value must NOT appear in summary
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("/sandbox"));
    }
```

> 注意:`PolicySummary` derive `serde::Serialize`,需要 `ash-core` 已加 serde 依赖(M1.1 已完成)。security.rs 顶部若没有 `use serde`,加 `use serde::Serialize;` 或用全路径 `#[derive(serde::Serialize)]`。用全路径更简单(无需改 use)。

- [ ] **Step 2: 运行测试**

Run: `cargo test -p ash-core security::tests::plan028`
Expected: PASS —— 2 个测试全过。

- [ ] **Step 3: Commit**

```bash
git add ash-core/src/security.rs
git commit -m "feat(security): add PolicySummary::summarize() (Plan 028 M2.2)"
```

---

## Task M2.3:agent 子命令骨架 + describe-tools

**Files:**
- Create: `ash/auto-shell/src/agent/mod.rs`
- Create: `ash/auto-shell/src/agent/describe.rs`
- Modify: `ash/auto-shell/src/main.rs`

- [ ] **Step 1: 创建 agent 模块骨架**

创建 `ash/auto-shell/src/agent/mod.rs`:

```rust
//! Plan 028 M2: `ash agent ...` CLI subcommand family.
//!
//! Subcommands:
//!   ash agent describe-tools [--format json|compact] [--filter <csv>]
//!   ash agent describe-policy
//!   ash agent check "<command>"
//!   ash agent run "<command>" [--timeout N] [--format json|text]
//!   ash agent run-batch --input <ndjson>     (M3)
//!   ash agent compat-check                    (M4)

pub mod describe;

use std::process::ExitCode;

/// Dispatch the `ash agent <sub>` subcommand. Called from main.rs.
///
/// `args` is everything after `agent` on the command line.
pub fn dispatch(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!(
            "usage: ash agent <subcommand>\n\
             subcommands:\n\
             \  describe-tools [--format json|compact] [--filter file,git,...]\n\
             \  describe-policy\n\
             \  check \"<command>\"\n\
             \  run \"<command>\" [--timeout N] [--format json|text]"
        );
        return ExitCode::from(2);
    }
    let sub = args[0].as_str();
    let rest = &args[1..];
    match sub {
        "describe-tools" | "describe" => describe::describe_tools(rest),
        "describe-policy" => describe::describe_policy(),
        "check" => crate::agent::run::check_command(rest),
        "run" => crate::agent::run::run_command(rest),
        other => {
            eprintln!("ash agent: unknown subcommand '{}'", other);
            ExitCode::from(2)
        }
    }
}
```

创建 `ash/auto-shell/src/agent/describe.rs`:

```rust
//! Plan 028 M2.3: `ash agent describe-tools` and `describe-policy`.

use std::process::ExitCode;

/// `ash agent describe-tools [--format json|compact] [--filter <csv>]`
pub fn describe_tools(args: &[String]) -> ExitCode {
    let mut format = "json";
    let mut filter: Option<Vec<String>> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                if let Some(v) = args.get(i + 1) {
                    format = match v.as_str() {
                        "json" | "compact" => v.as_str(),
                        _ => {
                            eprintln!("ash agent describe-tools: --format must be json|compact");
                            return ExitCode::from(2);
                        }
                    };
                    i += 2;
                    continue;
                }
            }
            "--filter" => {
                if let Some(v) = args.get(i + 1) {
                    filter = Some(v.split(',').map(|s| s.trim().to_string()).collect());
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let shell = auto_shell::Shell::new();
    let registry = shell.build_tool_registry();

    let catalog: Vec<_> = if format == "compact" {
        registry.catalog_compact()
    } else {
        registry.catalog()
    };

    let filtered: Vec<_> = match &filter {
        None => catalog,
        Some(prefixes) => catalog
            .into_iter()
            .filter(|d| {
                prefixes
                    .iter()
                    .any(|p| d.name.starts_with(p) || d.name == *p)
            })
            .collect(),
    };

    let tools_json: Vec<serde_json::Value> = filtered.iter().map(|d| d.to_json()).collect();
    let envelope = serde_json::json!({
        "schema_version": "1",
        "tool_count": tools_json.len(),
        "tools": tools_json,
    });
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
    ExitCode::SUCCESS
}

/// `ash agent describe-policy`
pub fn describe_policy() -> ExitCode {
    let shell = auto_shell::Shell::new();
    let summary = shell.policy.summarize();
    let env = serde_json::json!({
        "schema_version": "1",
        "policy": summary,
        "note": "Policy is capability-only; specific sandbox paths and deny-list contents are NOT exposed.",
    });
    println!("{}", serde_json::to_string_pretty(&env).unwrap());
    ExitCode::SUCCESS
}
```

- [ ] **Step 2: 创建 run.rs 占位(M2.4 实现)**

创建 `ash/auto-shell/src/agent/run.rs`:

```rust
//! Plan 028 M2.4: `ash agent run` and `ash agent check`.

use std::process::ExitCode;

pub fn run_command(_args: &[String]) -> ExitCode {
    eprintln!("ash agent run: not yet implemented (Plan 028 M2.4)");
    ExitCode::from(1)
}

pub fn check_command(_args: &[String]) -> ExitCode {
    eprintln!("ash agent check: not yet implemented (Plan 028 M2.4)");
    ExitCode::from(1)
}
```

- [ ] **Step 3: 在 lib.rs 注册 agent 模块**

修改 `ash/auto-shell/src/lib.rs`,加:

```rust
pub mod agent;
```

并把 `dispatch` 里对 `run` 模块的引用对齐(mod.rs 用了 `crate::agent::run::...`,确认路径正确)。

注意:`agent` 模块从 main.rs 调用,需要 `pub`。`agent::run` 内部用 `auto_shell::Shell::new()`,确认 crate 名是 `auto-shell`(use 里用 `auto_shell::`)。

- [ ] **Step 4: 在 main.rs 加 agent 分支**

修改 `ash/auto-shell/src/main.rs`。找到 `while i < args.len()` 循环里的 match(约 line 50),在 `-c => { ... }` 分支**之前**加一个 `agent` 分支:

```rust
            "agent" => {
                // Plan 028: `ash agent <sub>` dispatches to the agent CLI family.
                let sub_args: Vec<String> = args[(i + 1)..].to_vec();
                return Ok::<(), miette::Report>(auto_shell::agent::dispatch(&sub_args).into());
            }
```

注意:`dispatch` 返回 `ExitCode`,而 main 返回 `Result<()>`。需要适配。最简单的方式是让 dispatch 直接 `std::process::exit(code)`:

修改 `agent/mod.rs` 的 `dispatch` 签名,让它返回 `()` 并自己 exit:

```rust
pub fn dispatch(args: &[String]) -> ! {
    // ... 同前,但每个返回点改为 std::process::exit(code)
    let code = /* 匹配逻辑 */;
    std::process::exit(code);
}
```

**更简洁的做法**:让各子命令返回 `ExitCode`,dispatch 也返回 `ExitCode`,在 main.rs 里 `std::process::exit(...)` 转换:

```rust
            "agent" => {
                let sub_args: Vec<String> = args[(i + 1)..].to_vec();
                let code = auto_shell::agent::dispatch(&sub_args);
                std::process::exit(code);
            }
```

(`ExitCode` 需要从 `process::Exit` 转 `i32`。更简单:让 dispatch 和子命令直接返回 `i32`。统一改成 `i32`。)

**最终决定**:`dispatch` 和所有子命令返回 `i32`(0 = 成功,非零 = 失败)。改 mod.rs / describe.rs / run.rs 的返回类型从 `ExitCode` 改成 `i32`,成功返回 `0`,失败返回 `2` 等。

- [ ] **Step 5: 手动冒烟测试**

Run: `cargo run -p auto-shell -- agent describe-tools --format compact | head -20`
Expected: 输出 JSON,包含 `tool_count`(>=70)和 `tools` 数组,每个 tool 有 name/description/inputSchema。

Run: `cargo run -p auto-shell -- agent describe-policy`
Expected: 输出 policy 摘要 JSON。

Run: `cargo run -p auto-shell -- agent describe-tools --filter ls,grep`
Expected: 只含 `ls` 和 `grep`(及任何以 ls/grep 开头的命令)。

- [ ] **Step 6: Commit**

```bash
git add ash/auto-shell/src/agent/ ash/auto-shell/src/lib.rs ash/auto-shell/src/main.rs
git commit -m "feat(agent): add 'ash agent' subcommand + describe-tools/policy (Plan 028 M2.3)"
```

---

## Task M2.4:agent run + check

**Files:**
- Modify: `ash/auto-shell/src/agent/run.rs`(实现 run + check)

- [ ] **Step 1: 实现 run + check**

把 `ash/auto-shell/src/agent/run.rs` 替换为:

```rust
//! Plan 028 M2.4: `ash agent run` and `ash agent check`.

use std::time::Instant;

use ash_core::tool::envelope::build_envelope;
use ash_core::tool::{ErrorKind, ToolData, ToolResult, ToolStatus};

/// `ash agent run "<command>" [--timeout N] [--format json|text]`
///
/// Executes a single command via Shell::execute_for_agent and wraps the
/// output in the Plan 028 response envelope.
pub fn run_command(args: &[String]) -> i32 {
    let (command, _timeout, format) = match parse_run_args(args) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let mut shell = auto_shell::Shell::new();
    shell.load_env_persistence();

    let start = Instant::now();
    let exec_result = shell.execute_for_agent(&command, false); // we build envelope ourselves
    let elapsed = start.elapsed();

    let result = match exec_result {
        Ok(Some(output)) => {
            let mut r = if format == "text" {
                ToolResult::success_text(output)
            } else {
                // Wrap text output as a generic Text data block.
                ToolResult::success_json(serde_json::json!({
                    "kind": "text",
                    "atom_type": "Text",
                    "value": output,
                    "pipeline_hint": "pipeable to grep/head/tail/wc",
                }))
            };
            r.timing.wall_ms = elapsed.as_millis() as u64;
            r
        }
        Ok(None) => {
            // No output (side-effect command like mkdir).
            let mut r = ToolResult::success_json(serde_json::json!({
                "kind": "empty",
                "atom_type": "Nothing",
                "value": null,
            }));
            r.timing.wall_ms = elapsed.as_millis() as u64;
            r
        }
        Err(e) => {
            // Determine kind heuristically from the error string.
            let msg = format!("{}", e);
            let kind = classify_error(&msg);
            let mut r = ToolResult::failed(kind, msg);
            r.timing.wall_ms = elapsed.as_millis() as u64;
            r
        }
    };

    // If the shell recorded a non-zero exit code, override status to Failed.
    let exit_code = shell.last_exit_code();
    let result = if exit_code != 0 && result.is_success() {
        let mut r = ToolResult::failed(
            ErrorKind::NonzeroExit,
            format!("command exited with code {}", exit_code),
        );
        r.timing.wall_ms = elapsed.as_millis() as u64;
        r
    } else {
        result
    };

    let envelope = build_envelope(&result, &command);
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());

    if result.is_success() {
        0
    } else {
        1
    }
}

/// `ash agent check "<command>"`
///
/// Dry-run: evaluate the command against the security policy WITHOUT
/// executing. Returns whether it would be allowed.
pub fn check_command(args: &[String]) -> i32 {
    let command = match args.get(0) {
        Some(c) => c.clone(),
        None => {
            eprintln!("ash agent check: missing command argument");
            return 2;
        }
    };

    let shell = auto_shell::Shell::new();
    // Parse the command into (name, args) using the existing helper.
    let parts = ash_core::cmd::external::parse_command(&command);
    if parts.is_empty() {
        let env = serde_json::json!({
            "command": command,
            "allowed": false,
            "denied_reasons": [{"rule_id": "empty-command", "message": "empty command"}],
        });
        println!("{}", serde_json::to_string_pretty(&env).unwrap());
        return 0;
    }
    let cmd_name = &parts[0];
    let cmd_args = &parts[1..];

    // We need to classify external-ness like Shell does, but that method is
    // private. Workaround: check the registry + legacy builtins via Shell.
    // For check, we re-build a shell and inspect.
    let mut shell = shell;
    let is_external = shell.classify_is_external_pub(cmd_name);

    let result = shell.policy.check(cmd_name, cmd_args, is_external);
    let env = match result {
        Ok(ash_core::security::Decision::Allow) => serde_json::json!({
            "command": command,
            "allowed": true,
            "decision": "allow",
        }),
        Ok(ash_core::security::Decision::DryRun) => serde_json::json!({
            "command": command,
            "allowed": true,
            "decision": "dry_run",
            "note": "would be short-circuited under --dry-run",
        }),
        Err(e) => {
            let msg = format!("{}", e);
            serde_json::json!({
                "command": command,
                "allowed": false,
                "decision": "deny",
                "denied_reasons": [{
                    "rule_id": "security-policy",
                    "message": msg,
                }],
            })
        }
    };
    println!("{}", serde_json::to_string_pretty(&env).unwrap());
    0
}

// ── helpers ──

fn parse_run_args(args: &[String]) -> Result<(String, Option<u64>, String), i32> {
    let mut command: Option<String> = None;
    let mut timeout: Option<u64> = None;
    let mut format = "json";
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--timeout" => {
                if let Some(v) = args.get(i + 1) {
                    timeout = v.parse().map_err(|_| {
                        eprintln!("ash agent run: --timeout must be an integer");
                        2
                    })?;
                    i += 2;
                    continue;
                }
            }
            "--format" => {
                if let Some(v) = args.get(i + 1) {
                    format = match v.as_str() {
                        "json" | "text" => v.as_str(),
                        _ => {
                            eprintln!("ash agent run: --format must be json|text");
                            return Err(2);
                        }
                    };
                    i += 2;
                    continue;
                }
            }
            _ => {
                if command.is_none() {
                    command = Some(args[i].clone());
                }
            }
        }
        i += 1;
    }
    match command {
        Some(c) => Ok((c, timeout, format.to_string())),
        None => {
            eprintln!("ash agent run: missing command argument");
            Err(2)
        }
    }
}

fn classify_error(msg: &str) -> ErrorKind {
    let lower = msg.to_lowercase();
    if lower.contains("no such file") || lower.contains("not found") {
        ErrorKind::NotFound
    } else if lower.contains("permission denied") {
        ErrorKind::PermissionDenied
    } else if lower.contains("security:") || lower.contains("denied") {
        ErrorKind::SandboxViolation
    } else if lower.contains("timed out") || lower.contains("timeout") {
        ErrorKind::Timeout
    } else if lower.contains("invalid") || lower.contains("parse") {
        ErrorKind::InvalidArgs
    } else {
        ErrorKind::Internal
    }
}
```

- [ ] **Step 2: 给 Shell 加 classify_is_external_pub 访问器**

在 `ash/auto-shell/src/shell.rs` 的 `impl Shell` 块(`classify_is_external` 方法附近,line ~434)加一个 public 包装:

```rust
    /// Plan 028: public wrapper around classify_is_external for the
    /// `ash agent check` dry-run path.
    pub fn classify_is_external_pub(&self, cmd_name: &str) -> bool {
        self.classify_is_external(cmd_name)
    }
```

- [ ] **Step 3: 更新 agent/mod.rs 的返回类型(若 M2.3 还没改成 i32)**

确认 `dispatch` / `describe_tools` / `describe_policy` 都返回 `i32`。若 M2.3 用了 `ExitCode`,统一改成 `i32`。

- [ ] **Step 4: 手动冒烟测试**

Run: `cargo run -p auto-shell -- agent run "echo hello"`
Expected: 输出 JSON 信封,status=success,data.kind=text,data.value="hello"。

Run: `cargo run -p auto-shell -- agent run "ls /nonexistent"`
Expected: status=failed,error.kind=not_found(或 internal,取决于 ls 的错误信息)。

Run: `cargo run -p auto-shell -- agent check "rm -rf /"`
Expected: allowed=false(命中 dangerous-pattern)。

Run: `cargo run -p auto-shell -- agent check "ls"`
Expected: allowed=true。

Run: `cargo run -p auto-shell -- agent run "ls" --format text`
Expected: 纯文本输出(不包信封)。

- [ ] **Step 5: Commit**

```bash
git add ash/auto-shell/src/agent/run.rs ash/auto-shell/src/shell.rs ash/auto-shell/src/agent/mod.rs
git commit -m "feat(agent): implement 'ash agent run' + 'check' (Plan 028 M2.4)"
```

---

## Task M2.5:CLI 端到端测试

**Files:**
- Create: `ash/auto-shell/tests/agent_cli.rs`

- [ ] **Step 1: 写端到端测试**

创建 `ash/auto-shell/tests/agent_cli.rs`:

```rust
//! Plan 028 M2.5: End-to-end tests for `ash agent ...` CLI.

use std::process::Command;

/// Helper: run `ash agent <args>` and return (exit_code, stdout, stderr).
fn run_agent(args: &[&str]) -> (i32, String, String) {
    // Use `cargo run` so the test works without a pre-built binary.
    // The bin name is "ash" (see auto-shell/Cargo.toml [[bin]]).
    let mut cmd_args = vec!["run", "--quiet", "--", "agent"];
    cmd_args.extend_from_slice(args);
    let output = Command::new("cargo")
        .args(&cmd_args)
        .output()
        .expect("failed to run cargo");
    let code = output.status.code().unwrap_or(-1);
    // Note: with `cargo run --`, the exit code of cargo matches the child.
    // But `--` separates cargo args from bin args. We read stdout.
    (
        code,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn describe_tools_returns_valid_json_envelope() {
    let (code, stdout, _stderr) = run_agent(&["describe-tools", "--format", "compact"]);
    assert_eq!(code, 0, "agent describe-tools failed");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout not valid JSON");
    assert_eq!(v["schema_version"], "1");
    assert!(v["tool_count"].as_u64().unwrap() >= 70, "too few tools: {}", v["tool_count"]);
    assert!(v["tools"].is_array());
}

#[test]
fn describe_tools_filter_returns_subset() {
    let (_code, stdout, _stderr) = run_agent(&["describe-tools", "--filter", "ls", "--format", "compact"]);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("not JSON");
    let names: Vec<&str> = v["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.iter().any(|n| *n == "ls"));
    assert!(names.iter().all(|n| n.starts_with("ls"))); // filter honored
}

#[test]
fn describe_policy_returns_capability_summary() {
    let (code, stdout, _stderr) = run_agent(&["describe-policy"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("not JSON");
    assert!(v["policy"].is_object());
    // Must NOT contain specific paths even when sandboxed
    let json_str = stdout.clone();
    assert!(!json_str.contains("/sandbox") || !v["policy"]["sandboxed"].as_bool().unwrap_or(false));
}

#[test]
fn run_echo_returns_success_envelope() {
    let (code, stdout, _stderr) = run_agent(&["run", "echo hello"]);
    assert_eq!(code, 0, "agent run echo failed: {}", _stderr);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout not JSON");
    assert_eq!(v["schema_version"], "1");
    assert_eq!(v["status"], "success");
    assert_eq!(v["data"]["kind"], "text");
    assert!(v["data"]["value"].as_str().unwrap().contains("hello"));
    assert_eq!(v["command_echo"], "echo hello");
}

#[test]
fn run_nonexistent_command_returns_failed_envelope() {
    let (code, stdout, _stderr) = run_agent(&["run", "this_command_does_not_exist_xyz"]);
    // Non-zero exit on failure
    assert_ne!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout not JSON");
    assert_eq!(v["status"], "failed");
    assert!(v["error"]["kind"].is_string());
}

#[test]
fn check_dangerous_command_is_denied() {
    let (code, stdout, _stderr) = run_agent(&["check", "rm -rf /"]);
    assert_eq!(code, 0); // check itself succeeds; it reports the decision
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("not JSON");
    assert_eq!(v["allowed"], false);
    assert_eq!(v["decision"], "deny");
}

#[test]
fn check_safe_command_is_allowed() {
    let (code, stdout, _stderr) = run_agent(&["check", "ls"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("not JSON");
    assert_eq!(v["allowed"], true);
    assert_eq!(v["decision"], "allow");
}

#[test]
fn run_format_text_returns_plain_text() {
    let (code, stdout, _stderr) = run_agent(&["run", "echo plain", "--format", "text"]);
    assert_eq!(code, 0);
    // In text mode, output is NOT JSON-wrapped.
    assert!(!stdout.contains("\"schema_version\""));
    assert!(stdout.contains("plain"));
}

#[test]
fn no_subcommand_prints_usage() {
    let (code, _stdout, stderr) = run_agent(&[]);
    assert_ne!(code, 0);
    assert!(stderr.contains("usage:") || stderr.contains("subcommand"));
}
```

- [ ] **Step 2: 运行端到端测试**

Run: `cargo test -p auto-shell --test agent_cli`
Expected: PASS —— 9 个测试全过。若 `cargo run` 在测试环境太慢,考虑直接构建二进制:`cargo build -p auto-shell` 然后用 `target/debug/ash agent ...`。可把 `run_agent` 改成调 `env!("CARGO_BIN_EXE_ash")`(但集成测试里需用 `auto_shell` crate 的 bin 路径,具体看 workspace 布局)。

> **性能提示**:如果 `cargo run` 让测试很慢(每次重新链接),改成预构建 + 直接调用二进制:
> ```rust
> let bin = std::env::var("ASH_TEST_BIN").unwrap_or_else(|_| "target/debug/ash".into());
> Command::new(bin).args(args)
> ```
> 并在跑测试前 `cargo build`。

- [ ] **Step 3: Commit**

```bash
git add ash/auto-shell/tests/agent_cli.rs
git commit -m "test(agent): end-to-end CLI tests for ash agent (Plan 028 M2.5)"
```

---

## Task M2.6:回归测试 —— 旧 -c --json 不变

**Files:**
- Create: `ash/auto-shell/tests/legacy_json_compat.rs`

- [ ] **Step 1: 写回归测试**

创建 `ash/auto-shell/tests/legacy_json_compat.rs`:

```rust
//! Plan 028 M2.6: Regression — the existing `ash -c "..." --json` interface
//! must keep working unchanged. (Plan 007 contract preserved.)

use std::process::Command;

fn run_ash(args: &[&str]) -> (i32, String, String) {
    let mut cmd_args = vec!["run", "--quiet", "--"];
    cmd_args.extend_from_slice(args);
    let output = Command::new("cargo")
        .args(&cmd_args)
        .output()
        .expect("failed to run cargo");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn legacy_c_flag_executes_command() {
    // `ash -c "echo hi"` should print "hi" (Plan 007 behavior, no --json).
    let (code, stdout, _stderr) = run_ash(&["-c", "echo hi"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("hi"));
}

#[test]
fn legacy_c_json_flag_still_works() {
    // `ash -c "echo hi" --json` should still produce output (Plan 007 JSON mode).
    // We don't assert exact JSON shape (that's Plan 007's contract), only that
    // it doesn't error and produces something.
    let (code, stdout, stderr) = run_ash(&["-c", "echo hi", "--json"]);
    assert_eq!(code, 0, "stderr: {}", stderr);
    assert!(!stdout.is_empty());
}
```

- [ ] **Step 2: 运行回归测试**

Run: `cargo test -p auto-shell --test legacy_json_compat`
Expected: PASS —— 2 个测试全过。

- [ ] **Step 3: Commit**

```bash
git add ash/auto-shell/tests/legacy_json_compat.rs
git commit -m "test(agent): regression guard for legacy -c --json (Plan 028 M2.6)"
```

---

## Task M2.7:M2 完成验收

- [ ] **Step 1: 全量测试**

Run: `cargo test -p ash-core && cargo test -p auto-shell`
Expected: 全部 PASS。

- [ ] **Step 2: clippy 干净**

Run: `cargo clippy --workspace -- -D warnings`
Expected: 无 warning。

- [ ] **Step 3: 手动端到端走查(伪 Agent 流程)**

依次执行(记录每步输出是否合理):

```bash
cargo run -- agent describe-tools --format compact > /tmp/tools.json
cargo run -- agent describe-policy > /tmp/policy.json
cargo run -- agent check "ls /tmp"
cargo run -- agent run "ls /tmp"
cargo run -- agent run "echo final check"
```

确认:catalog >= 70 tools、policy 是 capability 摘要、check/run 返回合法 JSON 信封。

- [ ] **Step 4: Commit M2 完成标记**

```bash
git commit --allow-empty -m "chore(028): M2 complete — external Agent CLI + structured envelope"
```

---

# 里程碑 M3 + M4:任务概要(待 M1+M2 完成后细化)

> 以下两个里程碑在 M1+M2 完成并验证后,各自展开成详细的 TDD 任务计划。这里只给任务清单和验收标准,作为后续 Plan 028-续 或 Plan 029 的输入。

## M3:批量调用 + NDJSON

**目标**:`ash agent run-batch` 从 stdin 读 NDJSON,流式输出结果。

**任务清单**:
1. 定义 NDJSON 输入/输出格式(`BatchRequest` / `BatchResponse` 结构,含 `seq` 字段)。
2. 实现 `ash agent run-batch --input <file>`(或从 stdin)。
3. 流式处理:每读完一行输入,立即执行并输出一行结果(不缓冲全部)。
4. `seq` 关联:保证响应 `seq` 与请求对应,即便乱序完成。
5. 失败隔离:单条失败不中断后续。
6. 测试:100 条混合(成功/失败/denied)的批量请求,验证顺序、隔离、seq 对齐。

**验收标准**:
- `echo '{"seq":1,"command":"echo a"}\n{"seq":2,"command":"echo b"}' | ash agent run-batch` 产出两行 NDJSON 响应。
- 单条 denied 不中断后续。
- 性能:100 条命令批量执行比 100 次 `ash agent run` 快至少 3 倍(少 fork)。

## M4:跨平台 + bash 兼容测试

**目标**:把"跨平台一致"变成 CI 守护。

**任务清单**:
1. 建 `tests/bash_compat/` 目录,30+ 命令的行为测试(用 `.at` 脚本或 Rust 集成测试)。
2. 建 `tests/cross_platform/`,三平台一致性测试(路径、行结尾、权限位)。
3. 建 `tests/agent_contract/`,信封契约测试(envelope schema、error kinds、truncation)。
4. 写 `docs/compat.md`(命令清单 + flag 矩阵 + 已知差异表 + 等价命令映射)。
5. 实现 `ash agent compat-check` 自检命令。
6. 配 GitHub Actions 三平台 matrix(ubuntu/macos/windows)。

**验收标准**:
- 三平台 CI 全绿。
- `docs/compat.md` 覆盖 30+ 核心命令。
- `ash agent compat-check` 输出当前平台的兼容性状态。
- 任何破坏 bash 兼容的改动被 CI 拦住。

---

## Plan 自检结果

**1. Spec 覆盖检查**(对照 `designs/028-agent-execution-engine.md`):

| Spec 章节 | 覆盖任务 | 状态 |
|---|---|---|
| §1.1-1.5 Tool Registry | M1.1-M1.6 | ✅ 完整 |
| §1.6 渐进迁移(bridge) | M1.4-M1.6 | ✅ |
| §2.2 CLI 子命令 | M2.3-M2.4 | ✅(run-batch 留 M3) |
| §2.3 NDJSON | M3 概要 | ⏳ 待细化 |
| §3 内置 loop 接口 | M1.3(agent_loop.rs trait) | ✅ 接口就位,实现留 Plan 029 |
| §4.2 信封 | M2.1 | ✅ |
| §4.3 Atom→kind | M1.7 | ✅ |
| §4.4 错误模型 | M1.2 | ✅ |
| §4.6 truncation | M2.1(类型就位,策略在 run 实现里扩展) | ✅ 类型,⚠️ 截断逻辑待 M3 完善 |
| §4.7 限额 | M1.3(OutputLimits 类型) | ✅ 类型,⚠️ 强制执行待 M3 |
| §5 跨平台测试 | M4 概要 | ⏳ 待细化 |

**2. 占位符扫描**:无 TBD/TODO,所有代码块完整。M3/M4 是明确的"概要待细化",不是占位。

**3. 类型一致性**:
- `Tool` trait 的 5 个方法在 M1.3 定义,在 M1.4(bridge)、M1.5(bridge)、M1.6(DynamicCommandTool)中一致使用。
- `ToolResult` 的辅助构造器(success_json/denied/failed)在 M1.3 定义,M2.1/M2.4 使用。
- `ErrorKind` 8 个变体在 M1.2 定义,M2.4 的 `classify_error` 用到其中 6 个。
- `AtomType` 18 变体在 M1.7 的两个映射函数里全覆盖。
- `dispatch` 返回类型:统一用 `i32`(M2.3 Step 4 已说明从 ExitCode 改 i32)。

**4. 发现并修正的 spec 偏差**:
- spec §1.5 说 bridge 在 `ash-core/src/tool/bridge.rs`,实际必须在 `auto-shell`(依赖方向)。Plan 已在"架构难题与解法"说明,以 Plan 为准。
- spec 多处说 "21 种 AtomType",实际 18 种。spec 已在写 plan 前修正。

---

## 执行交接

Plan 完成并保存到 `plans/028-agent-execution-engine.md`。两种执行方式:

**1. Subagent-Driven(推荐)** —— 每个 task 派一个新 subagent 执行,任务间我做 review,迭代快、上下文干净。

**2. Inline Execution** —— 在当前 session 里按 executing-plans skill 逐 task 执行,带 checkpoint review。

选哪种?
