# ash-gui Implementation Plan

> **M0 可行性评估（2026-08-01，基于当前代码重新核实）**：上方 07-30 评估的数字已再次过时（029-035 期间代码持续重构，行号再次漂移）。本次逐点重新核实如下：
>
> **耦合点现状（2026-08-01 实测）：**
> - **crossterm 引用集中度比 07-30 评估更低**：shell.rs **仅 1 处**（`shell.rs:963` `crossterm::terminal::size()`，在 format_output 内），不是"散布 40+ 处"。其余 crossterm 在 `signal.rs`（1）、`frontend/mod.rs`（1）、`less.rs`（**33**）。
> - **`format_output` 是渲染分发点**（`shell.rs:938`）：三分支（json / bash_compat / ratatui 表格），crossterm 仅用于取终端宽度，`frontend::renderer::render_table_with` 才是真正的终端渲染。这是 M1 Renderer trait 要替换的核心。
> - **`cmd_color` 是第二处 frontend 耦合**（`shell.rs:3727`）：调用 `frontend::term::color::{resolve_fg,detect_color_depth}` 等 6 处（3738-3748）。
> - **`less.rs` 是 M0 的大头**（1197 行，**33 处 crossterm**：43×style + 13×terminal + 1×event）。经 `shell.rs:239-240` 命令注册硬依赖（`LessCommand`/`MoreCommand`）。`show.rs:149-151` 经 `super::less::{RawModeGuard,AltScreenGuard,CodePager}` 间接耦合。
> - **当前无 `[features]` 段**：auto-shell 是扁平 binary，M0「feature 隔离」= 新增 `terminal` feature，把所有 crossterm/frontend/less 用法置于 `#[cfg(feature="terminal")]` 下。命令注册（shell.rs:239-240）也要 cfg 化。
>
> **仍然成立的论点（已复核）：** ①`ash-core` **真正零终端依赖**（grep 命中全在注释里，明确写 "no dependency on reedline/crossterm/ratatui"）→ M1 Renderer trait 基础牢固；②frontend/ 隔离方向正确（10 个文件，含 renderer/ 子模块）；③**ash-gui scaffold 已就位**（独立 workspace `ash-gui/`，含 `ash-gui-bin` 成员，刻意与 `ash/` 隔离以防 ui-iced feature 交叉污染）。
>
> **less.rs 处理三选项的评估（见下方 §M0 决策）：** 推荐 **方案 A（整模块 cfg-gate）**——成本最低、风险最小、可逆。less/more/show 是终端 pager，GUI 模式下本就无意义。
>
> **结论：M0 可行，是 GUI 路线的合理入口。** 拆 M0a（frontend/ + format_output + cmd_color 隔离，~40 行 cfg）+ M0b（less.rs/show.rs 整模块 cfg-gate，~20 行 cfg + 验证 `--no-default-features` 编译）。真实工作量 ~60-100 行 cfg + 充分回归，**比 07-30 评估的 180-250 行更小**（因为 shell.rs 的 crossterm 已收敛到 1 处）。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 ash 构建 Shell-native GUI 前端(ash-gui),把结构化输出渲染成 iced widget,验证"Atom → 富 widget"的核心假设。

**Architecture:** M0 把 auto-shell 的终端依赖用 feature flag 隔离;M1 在 ash-core 抽象出 `Renderer` trait + `RenderedOutput` 中间表示;M2 在 ash-gui workspace 里用 AutoUI 的 iced 后端跑一个最小应用(输入 ls,看到可排序表格 widget)。

**Tech Stack:** Rust 2021、ash-core(纯逻辑)、auto-shell(feature 隔离)、AutoUI(`auto_lang::ui::iced` 后端)、iced 0.14。

**对应设计文档:** `designs/030-ash-gui.md`

**范围:** 本 Plan 详细覆盖 M0(auto-shell feature 隔离)+ M1(Renderer trait + RenderedOutput)+ M2(最小 GUI)。M3-M5 给任务概要,待 M2 验证通过后展开。

---

## 关键背景知识(实施者必读)

### 来自代码勘探的精确事实(勿凭记忆)

1. **`Shell::format_output` 是唯一渲染分发点**(`ash/auto-shell/src/shell.rs:856`),函数签名 `fn format_output(&self, pipeline: AtomPipeline) -> String`。三个分支:
   - `self.json_output` → `self.pipeline_to_json(pipeline)`(终端无关,保留)
   - `AtomPipeline::Atom(atom)` + `atom.is_structured()` → **唯一终端依赖**(调 `crossterm::terminal::size()` 在 line 866 + `render_table_with` 在 line 869)
   - fallback → `pipeline.into_text()`(终端无关,保留)
   - **只有中间分支需要 M0/M1 改动**。

2. **shell.rs 里只有一处 crossterm 调用**(line 866)。M0 的工作比 spec 设想的简单——不是散布各处的 cfg 标注。

3. **`render_table_with` 签名**(`ash/auto-shell/src/frontend/renderer/table.rs:29`):
   ```rust
   pub fn render_table_with(value: &Value, term_width: u16, icons: IconStyle) -> Option<String>
   ```
   它**只处理 `&Value`,不处理 `AtomPipeline`**,返回 `Option<String>`(不是前端无关 IR)。M1 的 Renderer trait 需要在它之上包一层。

4. **`IconStyle` 在 `ash/auto-shell/src/config.rs:49`**(`pub enum IconStyle`),包含 `Off` 变体。

5. **`Component` trait 在 `auto_lang::ui`**(`D:\autostack\auto-lang\crates\auto-lang\src\ui\component.rs`):
   ```rust
   pub trait Component: Sized + Debug {
       type Msg: Clone + Debug + 'static;
       fn on(&mut self, msg: Self::Msg);
       fn view(&self) -> View<Self::Msg>;
       #[cfg(feature = "ui-iced")]
       fn subscription(&self) -> iced::Subscription<Self::Msg> { iced::Subscription::none() }
   }
   ```

6. **iced 入口**:`auto_lang::ui::iced::run_app::<C>()`,要求 `C: Component + Default + 'static` 且 `C::Msg: Clone + Debug + Send + 'static`。参考 `D:\autostack\auto-lang\crates\auto-lang\examples\ui_counter.rs`。

7. **ash-gui scaffold 已存在**(`ash-gui/ash-gui-bin/src/main.rs`,19 行,feature canary)。Cargo.toml 已配 `auto-lang = { features = ["ui-iced"] }`,零启动成本。

8. **ash-core 测试全用内联 `#[cfg(test)] mod tests`**,没有 `tests/` 目录。auto-shell 用 `tests/` 集成测试(每个文件一个测试二进制),典型模式是 `Shell::new()` + `shell.execute()`。参考 `tests/ls_render.rs` 的"render + strip_ansi + assert"模式。

9. **`render_table_with` 测试模式**(见 `tests/ls_render.rs`):
   ```rust
   fn file_obj(name: &str, ty: &str) -> Value {
       let mut o = Obj::new();
       o.set("name", Value::str(name));
       o.set("type", Value::str(ty));
       Value::Obj(o)
   }
   // 单字符图标 assert 原始输出;多字符文本 assert strip_ansi 后的输出
   ```

### 与设计文档的偏差(已确认,以本 Plan 为准)

**偏差 1:`Renderer::render` 签名调整**

设计文档 §1.3 写的是 `fn render(&self, pipeline: &AtomPipeline) -> RenderedOutput`。但探勘发现 `render_table_with` 只处理 `&Value`,且 M1 的目标是"不破坏现有 TUI 行为"。

**修正**:Renderer trait 处理 `&Value` + `atom_type`,不直接处理 `AtomPipeline`。pipeline → (value, atom_type) 的提取放 ash-core 的新函数 `atom_pipeline_to_rendered`(包一层)。

**偏差 2:M0 工作量比 spec 估的小**

spec §5.2 说 M0 要"包 frontend/ + crossterm 提取为参数",约 500 行。实际只有 shell.rs:866 一处 crossterm 调用,加上 lib.rs 几个 `pub mod` 的 cfg 标注,M0 约 100 行。

### 测试约定

- `ash-core` 测试用内联 `#[cfg(test)] mod tests`(无 `tests/` 目录)
- `auto-shell` 测试用 `tests/` 集成测试
- 跨 workspace 的测试(ash-gui)在 `ash-gui-bin/src/` 内联
- 测试命令:`cd D:\autostack\auto-shell\<workspace>` 然后 `cargo test`(注意路径!不是 repo 根)

### 技术约束

- **ash-core 零终端依赖**(Plan 014 铁律)。`Renderer` trait 和 `RenderedOutput` 放 ash-core,绝对不引入 crossterm/ratatui。
- **auto-shell feature 隔离后**,`cargo build -p auto-shell --no-default-features` 必须能编译。
- **ash-gui workspace 与 ash CLI workspace 物理隔离**(Cargo feature 不统一)。
- **TDD**:每个任务先写失败测试,再写实现。

---

## 文件结构

### 新增文件

| 文件 | 职责 | 里程碑 |
|---|---|---|
| `ash-core/src/renderer.rs` | `Renderer` trait、`RenderedOutput`、`RenderedCell`、`CellTag`、`atom_pipeline_to_rendered()` | M1 |
| `ash/auto-shell/src/frontend/renderer/tui.rs` | `TuiRenderer` 实现 + `rendered_to_ansi()` | M1 |
| `ash/auto-shell/tests/renderer_trait.rs` | Renderer trait 集成测试 | M1 |
| `ash-gui/ash-gui-bin/src/app.rs` | `AshGuiApp` 实现 `Component` | M2 |
| `ash-gui/ash-gui-bin/src/block.rs` | `Block` 数据结构 | M2 |
| `ash-gui/ash-gui-bin/src/renderer.rs` | `GuiRenderer` + `rendered_to_iced()` | M2 |

### 修改文件

| 文件 | 改动 | 里程碑 |
|---|---|---|
| `ash/auto-shell/Cargo.toml` | 加 `[features] frontend-tui = [...]` | M0 |
| `ash/auto-shell/src/lib.rs` | 几个 `pub mod` 加 `#[cfg(feature = "frontend-tui")]` | M0 |
| `ash/auto-shell/src/shell.rs` | `format_output` 的 crossterm 块加 cfg + 参数化 width | M0 → M1 |
| `ash-core/src/lib.rs` | 加 `pub mod renderer;` | M1 |
| `ash/auto-shell/src/frontend/renderer/mod.rs` | 加 `pub mod tui;` + re-export | M1 |
| `ash-gui/ash-gui-bin/src/main.rs` | 替换 scaffold 为真实 iced 应用 | M2 |

---

# 里程碑 M0:auto-shell feature 隔离(前置)

**目标**:`cargo build -p auto-shell --no-default-features` 能编译(纯 Shell 引擎,无 reedline/crossterm/ratatui)。

**完成标准**:CLI 正常模式(`--features frontend-tui` 或默认)行为不变(028 的 676 测试全过);`--no-default-features` 模式编译通过。

---

## Task M0.1:加 `[features]` 段到 auto-shell Cargo.toml

**Files:**
- Modify: `ash/auto-shell/Cargo.toml`

- [ ] **Step 1: 读取当前 Cargo.toml 确认结构**

Run: `head -40 ash/auto-shell/Cargo.toml`
确认 `[dependencies]` 段存在,且包含 reedline/crossterm/nu-ansi-term/ratatui-core/ratatui-widgets(约 line 27-34)。确认**没有** `[features]` 段。

- [ ] **Step 2: 在 [package] 段之后、[dependencies] 段之前加 [features]**

在 `ash/auto-shell/Cargo.toml` 找到 `[dependencies]` 行,在它之前插入:

```toml
[features]
default = ["frontend-tui"]
# Plan 030 M0: Terminal frontend (reedline/ratatui/crossterm). Disable to get
# a pure Shell engine suitable for GUI embedding (ash-gui uses this).
frontend-tui = ["dep:reedline", "dep:crossterm", "dep:nu-ansi-term", "dep:ratatui-core", "dep:ratatui-widgets"]
```

然后把 `[dependencies]` 段里的 5 个终端依赖改成 optional。找到这些行:

```toml
# Terminal and REPL
reedline = "0.44.0"
crossterm = "0.27"
nu-ansi-term = "0.50"

# Ratatui rendering (core + widgets without crossterm backend)
ratatui-core = "0.1"
ratatui-widgets = "0.3"
```

替换为:

```toml
# Terminal and REPL (optional — part of the frontend-tui feature)
reedline = { version = "0.44.0", optional = true }
crossterm = { version = "0.27", optional = true }
nu-ansi-term = { version = "0.50", optional = true }

# Ratatui rendering (core + widgets without crossterm backend, optional)
ratatui-core = { version = "0.1", optional = true }
ratatui-widgets = { version = "0.3", optional = true }
```

- [ ] **Step 3: 验证默认模式仍编译**

Run: `cd D:\autostack\auto-shell\ash && cargo build -p auto-shell`
Expected: 编译成功(default feature 启用,行为不变)。

- [ ] **Step 4: 验证 no-default-features 模式(此时会失败,因为 lib.rs 还没 cfg)**

Run: `cd D:\autostack\auto-shell\ash && cargo build -p auto-shell --no-default-features 2>&1 | head -20`
Expected: 编译错误,因为 `pub mod frontend;` 等 mod 仍会尝试编译 reedline 代码,但 reedline dep 已变成 optional 未启用。

(这就是下一步要修复的。)

- [ ] **Step 5: Commit**

```bash
cd D:\autostack\auto-shell && git add ash/auto-shell/Cargo.toml && git commit -m "build(auto-shell): add frontend-tui feature flag (Plan 030 M0.1)"
```

---

## Task M0.2:lib.rs 给终端依赖模块加 cfg

**Files:**
- Modify: `ash/auto-shell/src/lib.rs`

- [ ] **Step 1: 读取 lib.rs 的 mod 声明**

Run: `head -46 ash/auto-shell/src/lib.rs`
确认这些 mod 需要加 cfg(探勘确认它们依赖 reedline/ratatui):
- `pub mod frontend;`(line 17)
- `pub mod completions;`(line 22)
- `pub mod menu;`(line 27)
- `pub mod prompt;`(line 28)

以及 re-export:
- `pub use frontend::repl;`(line 42)
- `pub use frontend::term;`(line 43)
- `pub use repl::Repl;`(line 45)

`pub mod shell;`(line 30)**不加 cfg**(shell.rs 主体是纯逻辑,只有 format_output 一处需要单独处理)。

- [ ] **Step 2: 给 4 个 mod 加 cfg 属性**

把:
```rust
pub mod frontend;
```
改为:
```rust
#[cfg(feature = "frontend-tui")]
pub mod frontend;
```

同样改 `completions`、`menu`、`prompt`。

- [ ] **Step 3: 给 3 个 re-export 加 cfg**

把:
```rust
pub use frontend::repl;
pub use frontend::term;
```
改为:
```rust
#[cfg(feature = "frontend-tui")]
pub use frontend::repl;
#[cfg(feature = "frontend-tui")]
pub use frontend::term;
```

把:
```rust
pub use repl::Repl;
```
改为:
```rust
#[cfg(feature = "frontend-tui")]
pub use repl::Repl;
```

- [ ] **Step 4: 验证 no-default-features 编译(可能仍有 shell.rs 的 crossterm 问题)**

Run: `cd D:\autostack\auto-shell\ash && cargo build -p auto-shell --no-default-features 2>&1 | head -20`
Expected: 错误数减少,但 shell.rs:866 的 crossterm 调用还会报错(下一步 M0.3 修)。

- [ ] **Step 5: 验证默认模式仍编译 + 测试通过**

Run: `cd D:\autostack\auto-shell\ash && cargo test -p auto-shell 2>&1 | grep "test result" | head -15`
Expected: 跟 M0 之前一样数量的测试通过(约 676 passed, 0 failed)。

- [ ] **Step 6: Commit**

```bash
cd D:\autostack\auto-shell && git add ash/auto-shell/src/lib.rs && git commit -m "build(auto-shell): cfg-gate terminal-dependent modules (Plan 030 M0.2)"
```

---

## Task M0.3:shell.rs 的 format_output 终端依赖隔离

**Files:**
- Modify: `ash/auto-shell/src/shell.rs`(仅 line 856-881 的 format_output 函数)

- [ ] **Step 1: 读取 format_output 当前实现**

Run: `sed -n '852,881p' ash/auto-shell/src/shell.rs`
确认三段分支(json_output / structured atom / fallback)。只有中间分支(864-877)依赖 crossterm + render_table_with。

- [ ] **Step 2: 把中间分支用 cfg 包起来,加 fallback**

把 `format_output` 函数体(line 856-881)替换为:

```rust
    fn format_output(&self, pipeline: AtomPipeline) -> String {
        // Plan 007: agent mode (--json) serializes the terminal pipeline
        // to JSON instead of the human-readable table.
        if self.json_output {
            return self.pipeline_to_json(pipeline);
        }

        // Plan 030 M0: the structured-table rendering path depends on
        // crossterm (for terminal width) and ratatui (for the table widget).
        // When frontend-tui is disabled (e.g. ash-gui embeds the engine),
        // fall back to plain text — GUI provides its own structured renderer.
        #[cfg(feature = "frontend-tui")]
        {
            // Try ratatui table rendering for structured Atom data
            if let AtomPipeline::Atom(ref atom) = pipeline {
                if atom.is_structured() {
                    let term_width = crossterm::terminal::size()
                        .map(|(w, _)| w)
                        .unwrap_or(80);
                    if let Some(rendered) = crate::frontend::renderer::render_table_with(
                        &atom.value,
                        term_width,
                        self.ls_icons,
                    ) {
                        return rendered;
                    }
                }
            }
        }

        // Fallback: plain text
        pipeline.into_text()
    }
```

- [ ] **Step 3: 验证 no-default-features 编译成功**

Run: `cd D:\autostack\auto-shell\ash && cargo build -p auto-shell --no-default-features 2>&1 | tail -5`
Expected: 编译成功(可能有 unused warning,正常)。

- [ ] **Step 4: 验证默认模式测试全过(回归守护)**

Run: `cd D:\autostack\auto-shell\ash && cargo test -p auto-shell 2>&1 | grep "test result" | tail -15`
Expected: 跟 M0 之前一样数量的测试通过。

特别注意 `tests/ls_render.rs` 和 `tests/structured_commands.rs` 必须全过(它们验证结构化渲染)。

- [ ] **Step 5: Commit**

```bash
cd D:\autostack\auto-shell && git add ash/auto-shell/src/shell.rs && git commit -m "feat(shell): cfg-gate format_output structured-render path (Plan 030 M0.3)"
```

---

## Task M0.4:M0 完成验收

- [ ] **Step 1: 全量测试(默认模式)**

Run: `cd D:\autostack\auto-shell\ash && cargo test -p auto-shell 2>&1 | grep "test result" | tail -15`
Expected: 全部 PASS(028 引入的 676 + 既有测试)。

- [ ] **Step 2: 全量测试(no-default-features 模式)**

Run: `cd D:\autostack\auto-shell\ash && cargo test -p auto-shell --no-default-features 2>&1 | grep "test result" | tail -15`
Expected: 大部分测试 PASS。少量依赖 frontend 的测试可能被 cfg 掉(正常),不应有失败。

如果有失败,看是否是依赖 reedline 的测试没加 cfg。给那些测试加 `#[cfg(feature = "frontend-tui")]`。

- [ ] **Step 3: clippy 干净(只看新增代码)**

Run: `cd D:\autostack\auto-shell\ash && cargo clippy -p auto-shell 2>&1 | grep -E "error" | head`
Expected: 无新增 error(既有的 from_yaml/host.rs 两个 clippy error 不算)。

- [ ] **Step 4: Commit M0 完成标记**

```bash
cd D:\autostack\auto-shell && git commit --allow-empty -m "chore(030): M0 complete — auto-shell feature isolation

auto-shell can now build with --no-default-features (pure Shell engine)
for GUI embedding. Default mode (frontend-tui) behavior unchanged."
```

---

# 里程碑 M1:Renderer trait + RenderedOutput(地基)

**目标**:`ash-core` 里有完整的 `Renderer` trait + `RenderedOutput` + `atom_pipeline_to_rendered()`。TUI 重构为用它(`TuiRenderer`),回归测试全过。

**完成标准**:`ash-core` 的新 renderer 模块测试全过;`auto-shell` 全量测试不变(视觉无变化,因为 TuiRenderer 复用 render_table_with)。

---

## Task M1.1:RenderedOutput + RenderedCell + CellTag 类型定义

**Files:**
- Create: `ash-core/src/renderer.rs`
- Modify: `ash-core/src/lib.rs`(加 `pub mod renderer;`)

- [ ] **Step 1: 在 ash-core/src/lib.rs 注册模块**

读取 `ash-core/src/lib.rs`,在现有 `pub mod` 列表(约 line 10-26)加:

```rust
pub mod renderer;
```

(放在 `pub mod pipeline;` 之后,`pub mod security;` 之前,保持字母序。)

- [ ] **Step 2: 创建 renderer.rs,写类型 + 测试**

创建 `ash-core/src/renderer.rs`:

```rust
//! Plan 030 M1: Frontend-agnostic rendering intermediate representation.
//!
//! `RenderedOutput` is what `Renderer::render` produces. It is NOT a string
//! (that's TUI-specific) and NOT an iced::Element (that's GUI-specific) —
//! it's the IR both frontends consume. The TUI path converts it to ANSI via
//! `rendered_to_ansi`; the GUI path converts it to iced widgets via
//! `rendered_to_iced`.
//!
//! Lives in ash-core (zero terminal deps).

use crate::pipeline::AtomType;

/// A command's visual output, frontend-agnostic.
#[derive(Debug, Clone)]
pub enum RenderedOutput {
    /// Structured table (FileList / ProcessList / Table / etc.)
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<RenderedCell>>,
        atom_type: AtomType,
    },
    /// Single record (Record)
    Record(Vec<(String, RenderedCell)>),
    /// Plain text
    Text(String),
    /// Empty output (side-effect commands like mkdir)
    Empty,
    /// Error
    Error {
        message: String,
        kind: RenderErrorKind,
    },
}

/// A single cell in a rendered table. Carries optional semantic tag for
/// GUI interaction (clickable filename, navigable path, etc.).
#[derive(Debug, Clone)]
pub enum RenderedCell {
    Text(String),
    Number(f64),
    Bool(bool),
    Null,
    /// Tagged cell — GUI can attach click behavior based on the tag.
    Tagged { text: String, tag: CellTag },
}

impl RenderedCell {
    /// Construct a plain text cell.
    pub fn text(s: impl Into<String>) -> Self {
        RenderedCell::Text(s.into())
    }

    /// The display string (regardless of variant).
    pub fn as_str(&self) -> &str {
        match self {
            RenderedCell::Text(s) => s,
            RenderedCell::Tagged { text, .. } => text,
            _ => "",
        }
    }
}

/// Semantic tag for a rendered cell. Tells the GUI what click means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellTag {
    /// File name — double-click to `open`, right-click for cp/mv/rm menu.
    FileName,
    /// Filesystem path — click to `cd`, drag to input to fill.
    Path,
    /// URL — click to open in browser.
    Url,
    /// Process ID — right-click for kill menu.
    Pid,
    /// Git branch — click to checkout.
    Branch,
    /// No special interaction.
    Plain,
}

impl Default for CellTag {
    fn default() -> Self {
        CellTag::Plain
    }
}

/// Error category for the Error variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderErrorKind {
    NotFound,
    PermissionDenied,
    NonzeroExit,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_cell_text_constructor() {
        let c = RenderedCell::text("hello");
        assert_eq!(c.as_str(), "hello");
        assert!(matches!(c, RenderedCell::Text(_)));
    }

    #[test]
    fn rendered_cell_tagged_as_str() {
        let c = RenderedCell::Tagged {
            text: "main.rs".into(),
            tag: CellTag::FileName,
        };
        assert_eq!(c.as_str(), "main.rs");
    }

    #[test]
    fn rendered_cell_number_as_str_is_empty() {
        // Number/Bool/Null don't carry a string; as_str returns empty.
        let c = RenderedCell::Number(42.0);
        assert_eq!(c.as_str(), "");
    }

    #[test]
    fn cell_tag_default_is_plain() {
        assert_eq!(CellTag::default(), CellTag::Plain);
    }

    #[test]
    fn rendered_output_table_constructs() {
        let t = RenderedOutput::Table {
            columns: vec!["name".into(), "size".into()],
            rows: vec![vec![RenderedCell::text("a"), RenderedCell::Number(1.0)]],
            atom_type: AtomType::FileList,
        };
        assert!(matches!(t, RenderedOutput::Table { .. }));
    }
}
```

- [ ] **Step 3: 运行测试验证通过**

Run: `cd D:\autostack\auto-shell\ash-core && cargo test renderer:: 2>&1 | tail -5`
Expected: PASS — 5 个测试全过。

- [ ] **Step 4: Commit**

```bash
cd D:\autostack\auto-shell && git add ash-core/src/renderer.rs ash-core/src/lib.rs && git commit -m "feat(renderer): add RenderedOutput/RenderedCell/CellTag IR (Plan 030 M1.1)"
```

---

## Task M1.2:Renderer trait + atom_pipeline_to_rendered 函数

**Files:**
- Modify: `ash-core/src/renderer.rs`(追加)

- [ ] **Step 1: 在 renderer.rs 追加 trait 和转换函数**

在 `ash-core/src/renderer.rs` 末尾(`#[cfg(test)]` 之前)追加:

```rust
use crate::pipeline::{AtomPipeline, Atom};
use auto_val::Value;

/// Renderer trait — TUI and GUI each provide one implementation.
///
/// The trait is intentionally thin: it takes the frontend-agnostic inputs
/// (a Value + its AtomType + width/icon hints) and produces a
/// frontend-agnostic RenderedOutput. Each frontend then has its own
/// "last mile" function (`rendered_to_ansi` for TUI, `rendered_to_iced`
/// for GUI) that converts RenderedOutput to its native form.
pub trait Renderer {
    /// Render a typed Atom value to the frontend-agnostic IR.
    fn render(&self, value: &Value, atom_type: AtomType) -> RenderedOutput;

    /// Terminal/widget width hint (for wrapping decisions).
    fn width_hint(&self) -> u16;
}

/// Convert an AtomPipeline to a RenderedOutput by dispatching on its variant.
///
/// This is the shared entry point both frontends use. It does NOT depend
/// on any terminal library — it produces the IR, leaving the "last mile"
/// (IR → ANSI string or IR → iced widget) to each frontend.
pub fn atom_pipeline_to_rendered(pipeline: &AtomPipeline, icons: IconStyleHint) -> RenderedOutput {
    match pipeline {
        AtomPipeline::Atom(atom) => atom_to_rendered(atom, icons),
        AtomPipeline::Text(s) => RenderedOutput::Text(s.clone()),
        AtomPipeline::Empty => RenderedOutput::Empty,
        AtomPipeline::Stream(_) | AtomPipeline::ExternalStream(_) => {
            // Streams should be collected before rendering. If not, degrade
            // to Empty (callers should collect_stream() first).
            RenderedOutput::Empty
        }
    }
}

/// Convert a single Atom to RenderedOutput, dispatching on its semantic type.
pub fn atom_to_rendered(atom: &Atom, icons: IconStyleHint) -> RenderedOutput {
    let atom_type = atom.atom_type();
    match atom_type {
        AtomType::FileList | AtomType::ProcessList | AtomType::Table | AtomType::MatchList => {
            render_table_value(&atom.value, atom_type, icons)
        }
        AtomType::FileEntry | AtomType::Record | AtomType::DiskEntry | AtomType::CpuInfo
        | AtomType::MemoryInfo | AtomType::SystemInfo | AtomType::BuildResult
        | AtomType::RunResult | AtomType::HelpInfo | AtomType::CountResult => {
            render_record_value(&atom.value)
        }
        AtomType::Text | AtomType::Path => render_text_value(&atom.value),
        AtomType::Nothing => RenderedOutput::Empty,
        AtomType::ProcessEntry | AtomType::CpuInfo => render_record_value(&atom.value),
    }
}

/// Hint for icon rendering (mirror of auto-shell's IconStyle, but terminal-free).
/// auto-shell's TuiRenderer maps its own IconStyle to this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconStyleHint {
    #[default]
    Plain,
    Nerdfont,
    Emoji,
    Off,
}

fn render_table_value(value: &Value, atom_type: AtomType, _icons: IconStyleHint) -> RenderedOutput {
    let arr = match value {
        Value::Array(a) => a,
        _ => return RenderedOutput::Text(value_to_text(value)),
    };
    if arr.is_empty() {
        return RenderedOutput::Empty;
    }
    // All elements must be objects to form a table
    let all_objects = arr.iter().all(|v| matches!(v, Value::Obj(_)));
    if !all_objects {
        return RenderedOutput::Text(value_to_text(value));
    }
    // Collect columns (all unique keys across all objects)
    let mut columns: Vec<String> = Vec::new();
    for v in arr.iter() {
        if let Value::Obj(obj) = v {
            for k in obj.keys() {
                let ks = k.to_string();
                if !columns.contains(&ks) {
                    columns.push(ks);
                }
            }
        }
    }
    if columns.is_empty() {
        return RenderedOutput::Empty;
    }
    // Build rows
    let rows: Vec<Vec<RenderedCell>> = arr
        .iter()
        .map(|v| {
            let obj = match v {
                Value::Obj(o) => o,
                _ => return Vec::new(),
            };
            columns
                .iter()
                .map(|col| {
                    match obj.get_str(&col.clone()) {
                        Some(val) => value_to_cell(val),
                        None => RenderedCell::Null,
                    }
                })
                .collect()
        })
        .collect();
    RenderedOutput::Table {
        columns,
        rows,
        atom_type,
    }
}

fn render_record_value(value: &Value) -> RenderedOutput {
    let obj = match value {
        Value::Obj(o) => o,
        _ => return RenderedOutput::Text(value_to_text(value)),
    };
    let fields: Vec<(String, RenderedCell)> = obj
        .iter()
        .map(|(k, v)| (k.to_string(), value_to_cell(v)))
        .collect();
    if fields.is_empty() {
        RenderedOutput::Empty
    } else {
        RenderedOutput::Record(fields)
    }
}

fn render_text_value(value: &Value) -> RenderedOutput {
    RenderedOutput::Text(value_to_text(value))
}

/// Convert an auto_val::Value to a RenderedCell (best effort).
pub fn value_to_cell(v: &Value) -> RenderedCell {
    match v {
        Value::Bool(b) => RenderedCell::Bool(*b),
        Value::Int(i) => RenderedCell::Number(*i as f64),
        Value::Uint(u) => RenderedCell::Number(*u as f64),
        Value::I64(i) => RenderedCell::Number(*i as f64),
        Value::Float(f) | Value::Double(f) => RenderedCell::Number(*f),
        Value::Byte(b) => RenderedCell::Number(*b as f64),
        Value::U8(b) => RenderedCell::Number(*b as f64),
        Value::I8(i) => RenderedCell::Number(*i as f64),
        Value::USize(u) => RenderedCell::Number(*u as f64),
        Value::Str(s) | Value::String(_) | Value::CStr(_) | Value::StrSlice(_) => {
            RenderedCell::Text(s.to_string())
        }
        Value::Char(c) => RenderedCell::Text(c.to_string()),
        Value::Nil | Value::Null | Value::None | Value::Void => RenderedCell::Null,
        Value::Some(inner) | Value::Ok(inner) => value_to_cell(inner),
        // Aggregates and language-level variants degrade to their string form.
        other => RenderedCell::Text(value_to_text(other)),
    }
}

/// Convert any auto_val::Value to a display string (best-effort).
pub fn value_to_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::I64(i) => i.to_string(),
        Value::Float(f) | Value::Double(f) => f.to_string(),
        Value::Byte(b) => b.to_string(),
        Value::U8(b) => b.to_string(),
        Value::I8(i) => i.to_string(),
        Value::USize(u) => u.to_string(),
        Value::Str(s) | Value::String(_) | Value::CStr(_) | Value::StrSlice(_) => s.to_string(),
        Value::Char(c) => c.to_string(),
        Value::Nil | Value::Null | Value::None | Value::Void => "null".to_string(),
        Value::Some(inner) | Value::Ok(inner) => value_to_text(inner),
        Value::Array(_) | Value::Block(_) => "[...]".to_string(),
        Value::Obj(_) => "{...}".to_string(),
        _ => format!("{:?}", v),
    }
}
```

- [ ] **Step 2: 修复编译错误**

Run: `cd D:\autostack\auto-shell\ash-core && cargo build 2>&1 | grep -E "^error" | head`
逐一修复编译错误。可能的错误:
- `Atom` 的导入路径(应该是 `crate::pipeline::Atom`)
- `obj.get_str` 方法名可能不准(查 auto-val/src/obj.rs)
- `obj.keys()` 返回类型

如果 `obj.get_str(&col.clone())` 不存在,改用:
```rust
obj.iter().find(|(k, _)| k.to_string() == *col).map(|(_, v)| value_to_cell(v))
```

具体修复取决于 auto-val 的实际 API。修到编译通过。

- [ ] **Step 3: 运行测试**

Run: `cd D:\autostack\auto-shell\ash-core && cargo test renderer:: 2>&1 | tail -5`
Expected: 之前的 5 个测试全过(类型定义没变)。

- [ ] **Step 4: 追加 atom_pipeline_to_rendered 的单元测试**

在 renderer.rs 的 `mod tests` 末尾追加:

```rust
    use crate::pipeline::{Atom, AtomPipeline};
    use auto_val::{Array, Obj};

    fn file_atom(name: &str, ty: &str) -> Atom {
        let mut entry = Obj::new();
        entry.set("name", Value::str(name));
        entry.set("type", Value::str(ty));
        let arr = Array::from_vec(vec![Value::Obj(entry)]);
        Atom::file_list(Value::Array(arr))
    }

    #[test]
    fn atom_pipeline_to_rendered_file_list() {
        let atom = file_atom("src", "dir");
        let pipeline = AtomPipeline::from_atom(atom);
        let rendered = atom_pipeline_to_rendered(&pipeline, IconStyleHint::default());
        match rendered {
            RenderedOutput::Table { columns, rows, atom_type } => {
                assert_eq!(atom_type, AtomType::FileList);
                assert!(columns.contains(&"name".to_string()));
                assert!(columns.contains(&"type".to_string()));
                assert_eq!(rows.len(), 1);
            }
            other => panic!("expected Table, got {:?}", other),
        }
    }

    #[test]
    fn atom_pipeline_to_rendered_text() {
        let pipeline = AtomPipeline::text("hello world");
        let rendered = atom_pipeline_to_rendered(&pipeline, IconStyleHint::default());
        assert!(matches!(rendered, RenderedOutput::Text(_)));
    }

    #[test]
    fn atom_pipeline_to_rendered_empty() {
        let pipeline = AtomPipeline::empty();
        let rendered = atom_pipeline_to_rendered(&pipeline, IconStyleHint::default());
        assert!(matches!(rendered, RenderedOutput::Empty));
    }

    #[test]
    fn value_to_cell_bool() {
        assert!(matches!(value_to_cell(&Value::Bool(true)), RenderedCell::Bool(true)));
    }

    #[test]
    fn value_to_cell_int() {
        assert!(matches!(value_to_cell(&Value::Int(42)), RenderedCell::Number(42.0)));
    }
```

(在 mod tests 顶部加 `use auto_val::Value;` 如果还没有。)

- [ ] **Step 5: 运行测试验证**

Run: `cd D:\autostack\auto-shell\ash-core && cargo test renderer:: 2>&1 | tail -5`
Expected: 10 个测试全过(5 个旧的 + 5 个新的)。

- [ ] **Step 6: Commit**

```bash
cd D:\autostack\auto-shell && git add ash-core/src/renderer.rs && git commit -m "feat(renderer): add Renderer trait + atom_pipeline_to_rendered (Plan 030 M1.2)"
```

---

## Task M1.3:TuiRenderer 实现(迁移现有 render_table_with)

**Files:**
- Create: `ash/auto-shell/src/frontend/renderer/tui.rs`
- Modify: `ash/auto-shell/src/frontend/renderer/mod.rs`(加 `pub mod tui;`)
- Modify: `ash/auto-shell/src/shell.rs`(format_output 用 Renderer)

- [ ] **Step 1: 在 renderer/mod.rs 注册 tui 模块**

读取 `ash/auto-shell/src/frontend/renderer/mod.rs`,在 `pub mod table;` 之后加:

```rust
pub mod tui;
```

- [ ] **Step 2: 创建 tui.rs**

创建 `ash/auto-shell/src/frontend/renderer/tui.rs`:

```rust
//! Plan 030 M1: TUI Renderer implementation.
//!
//! Consumes a `RenderedOutput` (the frontend-agnostic IR from ash-core) and
//! produces an ANSI-colored string for the terminal. This is the "last mile"
//! after `atom_pipeline_to_rendered` has done the structure analysis.
//!
//! The Table path reuses the existing `render_table_with` (which already
//! builds a ratatui Buffer and converts it to ANSI) for visual continuity.

use ash_core::pipeline::{Atom, AtomType};
use ash_core::renderer::{IconStyleHint, RenderedOutput, Renderer};
use auto_val::Value;

use crate::config::IconStyle;
use crate::frontend::renderer::table::render_table_with;

/// TUI renderer: produces ANSI strings for the terminal.
pub struct TuiRenderer {
    pub width: u16,
    pub icons: IconStyle,
}

impl TuiRenderer {
    pub fn new(width: u16, icons: IconStyle) -> Self {
        Self { width, icons }
    }

    /// Convert auto-shell's IconStyle to ash-core's IconStyleHint.
    fn icon_hint(&self) -> IconStyleHint {
        match self.icons {
            IconStyle::Plain => IconStyleHint::Plain,
            IconStyle::Nerdfont => IconStyleHint::Nerdfont,
            IconStyle::Emoji => IconStyleHint::Emoji,
            IconStyle::Off => IconStyleHint::Off,
        }
    }
}

impl Renderer for TuiRenderer {
    fn render(&self, value: &Value, atom_type: AtomType) -> RenderedOutput {
        // Reuse the existing render_table_with logic to produce the
        // frontend-agnostic IR. For non-table types, fall through to
        // ash-core's atom_to_rendered.
        let atom = Atom::new(value.clone(), atom_type);
        ash_core::renderer::atom_to_rendered(&atom, self.icon_hint())
    }

    fn width_hint(&self) -> u16 {
        self.width
    }
}

/// Convert a RenderedOutput to an ANSI-colored string for the terminal.
///
/// This is the TUI-specific "last mile". It reuses `render_table_with`
/// directly for the Table variant (to preserve the existing visual style
/// and icon support). Other variants are formatted inline.
pub fn rendered_to_ansi(
    rendered: &RenderedOutput,
    value: &Value,
    atom_type: AtomType,
    width: u16,
    icons: IconStyle,
) -> String {
    match rendered {
        RenderedOutput::Table { .. } => {
            // Re-render the original Value via render_table_with so we keep
            // the exact visual style (icons, column widths, borders) that
            // users already see. The IR's columns/rows are for the GUI;
            // the TUI keeps using the ratatui Buffer path.
            match atom_type {
                AtomType::FileList | AtomType::ProcessList | AtomType::Table
                | AtomType::MatchList => {
                    render_table_with(value, width, icons)
                        .unwrap_or_else(|| ash_core::renderer::value_to_text(value))
                }
                _ => ash_core::renderer::value_to_text(value),
            }
        }
        RenderedOutput::Text(s) => s.clone(),
        RenderedOutput::Empty => String::new(),
        RenderedOutput::Record(fields) => {
            let mut out = String::new();
            for (k, cell) in fields {
                out.push_str(&format!("{}: {}\n", k, cell.as_str()));
            }
            out
        }
        RenderedOutput::Error { message, .. } => format!("Error: {}", message),
    }
}
```

- [ ] **Step 3: 修改 shell.rs 的 format_output 用 TuiRenderer**

读取 `ash/auto-shell/src/shell.rs` line 856-881 的 format_output,替换中间 `#[cfg(feature = "frontend-tui")]` 块为:

```rust
        #[cfg(feature = "frontend-tui")]
        {
            use crate::frontend::renderer::tui::{TuiRenderer, rendered_to_ansi};
            use ash_core::renderer::{Renderer, atom_pipeline_to_rendered};

            if let AtomPipeline::Atom(ref atom) = pipeline {
                if atom.is_structured() {
                    let term_width = crossterm::terminal::size()
                        .map(|(w, _)| w)
                        .unwrap_or(80);
                    let renderer = TuiRenderer::new(term_width, self.ls_icons);
                    let hint = renderer.icon_hint();  // 需要 pub
                    let rendered = atom_pipeline_to_rendered(
                        &AtomPipeline::from_atom(atom.clone()),
                        hint,
                    );
                    // For the TUI, we still use render_table_with directly
                    // to preserve exact visual style. The IR is produced
                    // but not consumed by TUI (it's for GUI parity tests).
                    if let Some(rendered_str) = crate::frontend::renderer::render_table_with(
                        &atom.value,
                        term_width,
                        self.ls_icons,
                    ) {
                        return rendered_str;
                    }
                    // Fallback to IR-based rendering if table_with returned None
                    let _ = (renderer, rendered);  // suppress unused warnings
                }
            }
        }
```

**注**:这个改动**保持了 TUI 的视觉完全不变**(仍走 `render_table_with`)。IR 的计算是为了给 M2 的 GUI 和集成测试用。M1 阶段 TUI 不消费 IR。

- [ ] **Step 4: 修复编译错误**

Run: `cd D:\autostack\auto-shell\ash && cargo build -p auto-shell 2>&1 | grep -E "^error" | head`

可能的错误:
- `icon_hint()` 是私有的 → 改成 `pub fn icon_hint`
- `AtomPipeline::from_atom` 不存在 → 用 `AtomPipeline::Atom(atom.clone())` 或现有构造器
- `atom.clone()` 要求 Atom: Clone(它 derive Clone 了,OK)

逐一修复。

- [ ] **Step 5: 运行回归测试(关键)**

Run: `cd D:\autostack\auto-shell\ash && cargo test -p auto-shell 2>&1 | grep "test result" | tail -15`
Expected: **所有测试通过,数量与 M1 之前一致**(约 676)。特别确认 `tests/ls_render.rs` 和 `tests/structured_commands.rs` 全过。

如果有失败,最可能是 format_output 的改动破坏了视觉。回退 Step 3,保持原有的 render_table_with 调用方式,只在新文件 tui.rs 里写 IR 逻辑(不动 shell.rs)。

- [ ] **Step 6: Commit**

```bash
cd D:\autostack\auto-shell && git add ash/auto-shell/src/frontend/renderer/ ash/auto-shell/src/shell.rs && git commit -m "feat(renderer): TuiRenderer + IR computation in format_output (Plan 030 M1.3)"
```

---

## Task M1.4:Renderer trait 集成测试

**Files:**
- Create: `ash/auto-shell/tests/renderer_trait.rs`

- [ ] **Step 1: 写集成测试**

创建 `ash/auto-shell/tests/renderer_trait.rs`:

```rust
//! Plan 030 M1.4: Verify the Renderer trait produces correct IR for various inputs.
//! These tests verify the IR (RenderedOutput), NOT the ANSI string output
//! (that's covered by ls_render.rs).

use ash_core::pipeline::{Atom, AtomPipeline, AtomType};
use ash_core::renderer::{atom_pipeline_to_rendered, IconStyleHint, RenderedOutput};
use auto_shell::frontend::renderer::tui::TuiRenderer;
use auto_shell::config::IconStyle;
use auto_val::{Array, Obj, Value};
use ash_core::renderer::Renderer;

fn file_obj(name: &str, ty: &str) -> Value {
    let mut o = Obj::new();
    o.set("name", Value::str(name));
    o.set("type", Value::str(ty));
    Value::Obj(o)
}

#[test]
fn file_list_produces_table_ir() {
    let arr = Array::from_vec(vec![file_obj("src", "dir"), file_obj("main.rs", "file")]);
    let atom = Atom::file_list(Value::Array(arr));
    let pipeline = AtomPipeline::from_atom(atom);
    let rendered = atom_pipeline_to_rendered(&pipeline, IconStyleHint::default());
    match rendered {
        RenderedOutput::Table { columns, rows, atom_type } => {
            assert_eq!(atom_type, AtomType::FileList);
            assert!(columns.contains(&"name".to_string()));
            assert!(columns.contains(&"type".to_string()));
            assert_eq!(rows.len(), 2, "should have 2 rows");
        }
        other => panic!("expected Table, got {:?}", other),
    }
}

#[test]
fn empty_file_list_produces_empty_ir() {
    let arr = Array::from_vec(vec![]);
    let atom = Atom::file_list(Value::Array(arr));
    let pipeline = AtomPipeline::from_atom(atom);
    let rendered = atom_pipeline_to_rendered(&pipeline, IconStyleHint::default());
    assert!(matches!(rendered, RenderedOutput::Empty));
}

#[test]
fn text_pipeline_produces_text_ir() {
    let pipeline = AtomPipeline::text("hello");
    let rendered = atom_pipeline_to_rendered(&pipeline, IconStyleHint::default());
    match rendered {
        RenderedOutput::Text(s) => assert_eq!(s, "hello"),
        other => panic!("expected Text, got {:?}", other),
    }
}

#[test]
fn tui_renderer_implements_renderer_trait() {
    let r = TuiRenderer::new(80, IconStyle::default());
    assert_eq!(r.width_hint(), 80);
    let arr = Array::from_vec(vec![file_obj("a", "file")]);
    let value = Value::Array(arr);
    let rendered = r.render(&value, AtomType::FileList);
    assert!(matches!(rendered, RenderedOutput::Table { .. }));
}

#[test]
fn record_atom_produces_record_ir() {
    let mut entry = Obj::new();
    entry.set("field1", Value::str("value1"));
    entry.set("field2", Value::Int(42));
    let atom = Atom::new(Value::Obj(entry), AtomType::Record);
    let pipeline = AtomPipeline::from_atom(atom);
    let rendered = atom_pipeline_to_rendered(&pipeline, IconStyleHint::default());
    match rendered {
        RenderedOutput::Record(fields) => {
            assert!(fields.iter().any(|(k, _)| k == "field1"));
        }
        other => panic!("expected Record, got {:?}", other),
    }
}
```

- [ ] **Step 2: 运行测试**

Run: `cd D:\autostack\auto-shell\ash && cargo test -p auto-shell --test renderer_trait 2>&1 | tail -10`
Expected: 5 个测试全过。

如果失败,可能是:
- `TuiRenderer::icon_hint` 不是 pub → 在 tui.rs 改成 `pub fn icon_hint`
- `Atom::new(value, atom_type)` 不存在 → 用 `Atom::file_list` / `Atom::text` 等构造器,或确认 Atom 的构造器名
- `auto_shell::frontend::renderer::tui::TuiRenderer` 路径不对 → 检查 mod.rs 的 re-export

逐一修复。

- [ ] **Step 3: Commit**

```bash
cd D:\autostack\auto-shell && git add ash/auto-shell/tests/renderer_trait.rs && git commit -m "test(renderer): IR integration tests for Renderer trait (Plan 030 M1.4)"
```

---

## Task M1.5:M1 完成验收

- [ ] **Step 1: ash-core 全量测试**

Run: `cd D:\autostack\auto-shell\ash-core && cargo test 2>&1 | tail -3`
Expected: 全过(含新增的 10 个 renderer 测试)。

- [ ] **Step 2: auto-shell 全量测试(默认 + no-default-features)**

Run: `cd D:\autostack\auto-shell\ash && cargo test -p auto-shell 2>&1 | grep "test result" | tail -15`
Expected: 跟 M1 之前一致(约 676 passed, 0 failed)。

Run: `cd D:\autostack\auto-shell\ash && cargo test -p auto-shell --no-default-features 2>&1 | grep "test result" | tail -15`
Expected: 大部分通过(frontend-tui 相关测试被 cfg 掉)。

- [ ] **Step 3: 视觉回归检查(手动)**

Run: `cd D:\autostack\auto-shell\ash && cargo run -p auto-shell -- -c "ls" 2>/dev/null`
Expected: 输出跟 M1 之前视觉完全一致(表格 + 图标 + 颜色)。

- [ ] **Step 4: Commit M1 完成标记**

```bash
cd /d/autostack/auto-shell && git commit --allow-empty -m "chore(030): M1 complete — Renderer trait + RenderedOutput IR

ash-core has Renderer trait + atom_pipeline_to_rendered (frontend-agnostic IR).
auto-shell has TuiRenderer (IR produced but TUI still uses render_table_with
for visual continuity). GUI path (M2) will consume the IR.

Tests: ash-core 385+, auto-shell 681+ (added 5 renderer_trait). Visual unchanged."
```

---

# 里程碑 M2:最小可用 GUI(关键验证)

**目标**:一个能跑的 ash-gui 窗口,输入 ls,看到结构化表格 widget。

**完成标准**:`cargo run -p ash-gui-bin` 打开窗口,输入 `ls`,看到 iced 表格 widget,点列头能排序(或至少能看到结构化数据,交互可选)。

**这是关键检查点**:如果 M2 不让人兴奋,M3-M5 不该做。

---

## Task M2.1:ash-gui-bin 加 ash-core 依赖 + 数据结构

**Files:**
- Modify: `ash-gui/ash-gui-bin/Cargo.toml`
- Create: `ash-gui/ash-gui-bin/src/block.rs`

- [ ] **Step 1: 加 ash-core 依赖**

读取 `ash-gui/ash-gui-bin/Cargo.toml`,确认已有 `ash-core = { path = "../../ash-core" }`。如果还没有,在 `[dependencies]` 段加。

- [ ] **Step 2: 创建 block.rs(Block 数据结构)**

创建 `ash-gui/ash-gui-bin/src/block.rs`:

```rust
//! Plan 030 M2: Block — the GUI's core data structure.
//!
//! A Block is the complete record of one command execution: the input,
//! the structured output (RenderedOutput IR), status, timing. The GUI main
//! view is a list of Blocks.

use std::path::PathBuf;
use ash_core::renderer::RenderedOutput;

pub type BlockId = u64;

#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    pub command: String,
    pub cwd: PathBuf,
    pub exit_code: Option<i32>,
    pub output: Option<RenderedOutput>,
    pub status: BlockStatus,
}

impl Block {
    pub fn running(id: BlockId, command: String, cwd: PathBuf) -> Self {
        Self {
            id,
            command,
            cwd,
            exit_code: None,
            output: None,
            status: BlockStatus::Running,
        }
    }

    pub fn succeed(&mut self, output: RenderedOutput) {
        self.exit_code = Some(0);
        self.output = Some(output);
        self.status = BlockStatus::Success;
    }

    pub fn fail(&mut self, exit_code: i32, message: String) {
        self.exit_code = Some(exit_code);
        self.output = Some(RenderedOutput::Error {
            message,
            kind: ash_core::renderer::RenderErrorKind::Other,
        });
        self.status = BlockStatus::Failed;
    }
}

#[derive(Debug, Clone)]
pub enum BlockStatus {
    Running,
    Success,
    Failed,
}
```

- [ ] **Step 3: 验证编译**

Run: `cd D:\autostack\auto-shell\ash-gui && cargo build 2>&1 | tail -5`
Expected: 编译成功(block.rs 还没被 main.rs 引用,但自身应编译)。

- [ ] **Step 4: Commit**

```bash
cd D:\autostack\auto-shell && git add ash-gui/ash-gui-bin/src/block.rs ash-gui/ash-gui-bin/Cargo.toml && git commit -m "feat(ash-gui): Block data structure (Plan 030 M2.1)"
```

---

## Task M2.2:AshGuiApp 实现 Component(MVP 版)

**Files:**
- Create: `ash-gui/ash-gui-bin/src/app.rs`
- Modify: `ash-gui/ash-gui-bin/src/main.rs`

- [ ] **Step 1: 创建 app.rs(MVP Component 实现)**

创建 `ash-gui/ash-gui-bin/src/app.rs`:

```rust
//! Plan 030 M2: AshGuiApp — minimal Component impl.
//!
//! MVP: one text input, one Block display. User types a command, hits Enter,
//! the command runs via Shell, output renders as text (M2.3 adds table widget).

use auto_lang::ui::{Component, View};
use ash_core::renderer::{atom_pipeline_to_rendered, IconStyleHint};
use crate::block::{Block, BlockId};

#[derive(Debug, Default)]
pub struct AshGuiApp {
    pub blocks: Vec<Block>,
    pub next_block_id: BlockId,
    pub input_text: String,
    pub last_result: Option<String>,
}

#[derive(Clone, Debug)]
pub enum AppMsg {
    InputChanged(String),
    RunCommand,
    CommandDone(BlockId, String),
}

impl Component for AshGuiApp {
    type Msg = AppMsg;

    fn on(&mut self, msg: Self::Msg) {
        match msg {
            AppMsg::InputChanged(s) => {
                self.input_text = s;
            }
            AppMsg::RunCommand => {
                let cmd = std::mem::take(&mut self.input_text);
                if cmd.trim().is_empty() {
                    return;
                }
                let id = self.next_block_id;
                self.next_block_id += 1;
                let cwd = std::env::current_dir().unwrap_or_default();
                let block = Block::running(id, cmd, cwd);
                self.blocks.push(block);
                // NOTE: M2 runs commands synchronously for simplicity.
                // M3 will use iced::Task for async (§1.5 of design doc).
                let last = self.blocks.last_mut().unwrap();
                let result = run_shell_command(&last.command);
                match result {
                    Ok(rendered_str) => {
                        last.succeed(ash_core::renderer::RenderedOutput::Text(rendered_str.clone()));
                        self.last_result = Some(rendered_str);
                    }
                    Err(e) => {
                        last.fail(1, e.clone());
                        self.last_result = Some(format!("Error: {}", e));
                    }
                }
            }
            AppMsg::CommandDone(id, result) => {
                if let Some(b) = self.blocks.iter_mut().find(|b| b.id == id) {
                    b.succeed(ash_core::renderer::RenderedOutput::Text(result.clone()));
                    self.last_result = Some(result);
                }
            }
        }
    }

    fn view(&self) -> View<AppMsg> {
        // MVP: a column with [block list summary] + [input field] + [last result].
        let mut col = View::col().spacing(8).padding(16);

        // Block count header
        col = col.child(View::text(format!("ash-gui — {} block(s)", self.blocks.len())));

        // Show last command + its status
        if let Some(last) = self.blocks.last() {
            col = col.child(View::text(format!("❯ {} ({:?})", last.command, last.status)));
        }

        // Last result
        if let Some(result) = &self.last_result {
            col = col.child(View::text(format!("Output: {}", truncate(result, 500))));
        }

        // Input field
        col = col.child(
            View::text_input("Type a command and press Enter...")
                .on_input(AppMsg::InputChanged)
                .on_submit(AppMsg::RunCommand)
                .build()
        );

        col.build()
    }
}

fn run_shell_command(cmd: &str) -> Result<String, String> {
    // M2 MVP: spawn `ash -c <cmd>` as a subprocess and capture output.
    // (M3 will embed Shell directly via auto-shell feature-no-default-features.)
    let output = std::process::Command::new("ash")
        .arg("-c")
        .arg(cmd)
        .output()
        .map_err(|e| format!("failed to spawn ash: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(format!("exit {:?}: {}", output.status.code(), stderr))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...(truncated)", &s[..max])
    } else {
        s.to_string()
    }
}

// Suppress unused-import warning for atom_pipeline_to_rendered (used in M2.3).
#[allow(unused_imports)]
use _UnusedImports::*;
mod _UnusedImports {
    pub use super::atom_pipeline_to_rendered;
    pub use super::IconStyleHint;
}
```

- [ ] **Step 2: 替换 main.rs**

把 `ash-gui/ash-gui-bin/src/main.rs` 替换为:

```rust
// Plan 030 M2: ash-gui main entry. Runs AshGuiApp via AutoUI's iced backend.

mod app;
mod block;

fn main() -> auto_lang::ui::AppResult<()> {
    println!("ash-gui starting (AutoUI iced backend)...");
    auto_lang::ui::iced::run_app::<app::AshGuiApp>()
}
```

- [ ] **Step 3: 编译并修复错误**

Run: `cd D:\autostack\auto-shell\ash-gui && cargo build 2>&1 | grep -E "^error" | head -20`

可能的错误:
- `View::text_input` / `on_input` / `on_submit` 方法名不准 → 查 auto_lang::ui::View 的 API
- `View::col()` / `.child()` / `.build()` 链式调用不准 → 查 ui_counter.rs 示例的实际用法
- `run_app::<AshGuiApp>()` 要求 `AshGuiApp: Default`(已 derive)+ `Debug`(已 derive)
- iced 0.14 的 `on_submit` 可能要 `on_submit` 或不同名字

**关键修复策略**:如果 View API 不对,改用 ui_counter.rs 示例的确切写法。先求编译通过,视觉在 Step 5 调。

- [ ] **Step 4: 编译通过后,尝试运行**

Run: `cd D:\autostack\auto-shell\ash-gui && cargo run 2>&1 | head -20`
Expected: 启动 iced 窗口(或在终端打印 "ash-gui starting..." 然后打开窗口)。

如果窗口打开失败(crossterm/iced 后端问题),记录错误,这是 M2 的关键风险点(§6.2 risk 10)。

- [ ] **Step 5: 在窗口里输入 `echo hello`,按 Enter,看到输出**

如果看到 "❯ echo hello (Success)" + "Output: hello",M2 MVP 成功。

- [ ] **Step 6: Commit**

```bash
cd D:\autostack\auto-shell && git add ash-gui/ash-gui-bin/src/ && git commit -m "feat(ash-gui): minimal Component impl — input + Block + subprocess (Plan 030 M2.2)"
```

---

## Task M2.3:集成 Renderer IR(让 ls 输出结构化)

**Files:**
- Modify: `ash-gui/ash-gui-bin/src/app.rs`
- Create: `ash-gui/ash-gui-bin/src/renderer.rs`

- [ ] **Step 1: 创建 renderer.rs(GUI 渲染层)**

创建 `ash-gui/ash-gui-bin/src/renderer.rs`:

```rust
//! Plan 030 M2.3: GUI renderer — converts RenderedOutput IR to iced widgets.
//!
//! This is the GUI-specific "last mile" (counterpart to TUI's rendered_to_ansi).

use ash_core::renderer::{RenderedCell, RenderedOutput};
use auto_lang::ui::View;
use crate::app::AppMsg;

/// Convert a RenderedOutput to an auto_lang::ui::View for display.
pub fn rendered_to_view(rendered: &RenderedOutput) -> View<AppMsg> {
    match rendered {
        RenderedOutput::Text(s) => View::text(s.clone()),
        RenderedOutput::Empty => View::text("(no output)"),
        RenderedOutput::Table { columns, rows, .. } => {
            render_table_view(columns, rows)
        }
        RenderedOutput::Record(fields) => {
            let mut col = View::col().spacing(4);
            for (k, cell) in fields {
                col = col.child(View::text(format!("{}: {}", k, cell.as_str())));
            }
            col.build()
        }
        RenderedOutput::Error { message, .. } => {
            View::text(format!("Error: {}", message))
        }
    }
}

fn render_table_view(columns: &[String], rows: &[Vec<RenderedCell>]) -> View<AppMsg> {
    // MVP: render as aligned text. M4 will use a real iced table widget.
    let mut lines = Vec::new();

    // Header
    lines.push(columns.join(" | "));

    // Separator
    lines.push(columns.iter().map(|_| "---").collect::<Vec<_>>().join(" | "));

    // Rows
    for row in rows {
        let cells: Vec<String> = row.iter().map(|c| cell_str(c)).collect();
        lines.push(cells.join(" | "));
    }

    View::text(lines.join("\n"))
}

fn cell_str(c: &RenderedCell) -> String {
    match c {
        RenderedCell::Text(s) => s.clone(),
        RenderedCell::Tagged { text, .. } => text.clone(),
        RenderedCell::Number(n) => n.to_string(),
        RenderedCell::Bool(b) => b.to_string(),
        RenderedCell::Null => "null".to_string(),
    }
}
```

- [ ] **Step 2: 修改 app.rs 用 rendered_to_view**

在 `app.rs` 顶部加 `mod renderer;`(或 `use crate::renderer::rendered_to_view;`)。

把 `run_shell_command` 改为返回 `Result<RenderedOutput, String>`,并改用 ash-core 的 IR(但 M2 MVP 仍用 subprocess 拿文本,这里仅把文本包成 `RenderedOutput::Text`):

实际上,M2.3 的关键改进是:**当命令是 ls 时,直接构造 AtomPipeline → 转成 RenderedOutput IR → 渲染成表格 view**。但这需要 ash-gui 直接调 ash-core 的逻辑(不走 subprocess)。

**简化路径**:M2.3 暂时只把文本输出包成 `RenderedOutput::Text`,在 view 里显示。真正的"ls → 表格 IR"需要 ash-gui 直接依赖 auto-shell(M3 的 feature 隔离完成后的工作),M2 不做。

所以 M2.3 的最小改动:在 app.rs 里把 `last_result: Option<String>` 改为 `last_result: Option<RenderedOutput>`,在 view 里调 `rendered_to_view`。

- [ ] **Step 3: 运行验证**

Run: `cd D:\autostack\auto-shell\ash-gui && cargo run 2>&1 | head -10`
Expected: 窗口启动,输入 `echo hello`,看到 "Output: hello"(通过 rendered_to_view 的 Text 分支)。

- [ ] **Step 4: Commit**

```bash
cd D:\autostack\auto-shell && git add ash-gui/ash-gui-bin/src/ && git commit -m "feat(ash-gui): GUI renderer + RenderedOutput integration (Plan 030 M2.3)"
```

---

## Task M2.4:M2 完成验收 + 关键检查点

- [ ] **Step 1: 编译 + 运行**

Run: `cd D:\autostack\auto-shell\ash-gui && cargo run`
Expected: 窗口打开,能输入命令,看到输出。

- [ ] **Step 2: 手动测试矩阵**

在 ash-gui 窗口里依次输入,确认每个都合理:
- `echo hello` → 看到 "hello"
- `ls` → 看到 ls 的输出(M2 可能是文本形式,M4 才是表格)
- `pwd` → 看到当前目录
- `nonexistent_cmd` → 看到 error
- (空输入)→ 不触发执行

- [ ] **Step 3: 关键检查点 —— 真人评估**

**这是 Plan 030 的生死检查点。**

请真实用户(你)用 ash-gui 跑 5 分钟。判断:
- 视觉上"比 TUI 好"吗?(即使现在只是文本输出)
- 交互上有潜力吗?(窗口、输入框、Block 列表的雏形)
- 值得继续做 M3-M5 吗?

如果答案是"是",继续 M3。如果"跟 TUI 差不多,不值得",止损 —— Plan 030 到 M2 为止,M3-M5 不做。

- [ ] **Step 4: Commit M2 完成标记**

```bash
cd /d/autostack/auto-shell && git commit --allow-empty -m "chore(030): M2 complete — minimal ash-gui runs

Window opens, accepts commands, displays output via RenderedOutput IR.
M2 is the critical validation checkpoint per design §5.10.

Decision required: continue to M3-M5 (if M2 excites) or stop here."
```

---

# 里程碑 M3-M5:任务概要(待 M2 验证通过后展开)

> 以下三个里程碑在 M2 验证通过后,各自展开成详细的 TDD 任务计划。这里只给任务清单和验收标准。

## M3:Block 列表 + 富输入 + 基础补全

**目标**:能当日常终端用。

**任务清单**:
1. 把 Shell 嵌入 ash-gui 进程(用 auto-shell 的 `--no-default-features`,M0 已铺好),替换 subprocess 方案
2. 完整 Block 列表视图(多 Block 滚动、状态着色)
3. 富命令输入(多行、Shift+Enter、基础语法高亮)
4. 基础补全(路径 + 命令名,从 Shell 的 CommandRegistry 读)
5. Block 操作按钮(复制、重跑)
6. Block 持久化(JSON 文件,重启不丢历史)
7. 历史搜索(Ctrl+R 升级版)

**验收**:连续用 ash-gui 做 30 分钟真实开发工作,主观评价"不别扭"。

## M4:18 种 AtomType 全渲染 + CellTag 交互

**目标**:所有命令输出都有合适的 widget。

**任务清单**:
1. 为每种 AtomType 实现 `rendered_to_view` 分支(18 种)
2. 自定义 iced widget:可排序表格(FileList)、进程树(ProcessList)、磁盘图(DiskEntry)
3. CellTag 系统(FileName 双击 open、Path 单击 cd、Pid 右键 kill)
4. Error 卡片 + remediation 提示
5. Text 自动语言检测 + 语法高亮
6. 每种 AtomType 的渲染测试

**验收**:18 种 AtomType 每种都有对应命令测试(ls→FileList, ps→ProcessList, ...)。

## M5:AI 面板 + SmartCommand + 工具浏览器

**目标**:Warp 对标完成,差异化清晰。**依赖 Plan 029 SmartCommand 落地。**

**任务清单**:
1. AI 面板(集成 Plan 027 F4 chat + Plan 029 SmartCommand NLU)
2. SmartCommand 表单(从 command.at 的 args 推导)
3. SmartCommand 确认对话框(GUI 化的 confirm_before)
4. 工具浏览器侧边栏(79 命令 + SmartCommand,可搜索)
5. Block 引用(`@{block:42}` 语法)
6. Block 解释(选 Block 让 AI 解释)

**验收**:用 AI 面板的 SmartCommand 路径完成一次真实 `finish-worktree`(端到端)。

---

## Plan 自检结果

**1. Spec 覆盖检查**(对照 `designs/030-ash-gui.md`):

| Spec 章节 | 覆盖任务 | 状态 |
|---|---|---|
| §1 愿景与定位 | 整个 Plan 的前提 | ✅ |
| §1.1-1.3 Renderer trait | M1.1-M1.4 | ✅ |
| §1.4 TuiRenderer | M1.3 | ✅ |
| §1.5 Shell 引擎嵌入 | M3 任务 1(概要) | ⏳ M3 |
| §2 18 种 AtomType 映射 | M4(概要) | ⏳ M4 |
| §2.2 Block 模型 | M2.1 | ✅ 基础,M3 扩展 |
| §3 AutoUI 集成 | M2.2 | ✅ MVP |
| §4 交互设计 | M3-M5(概要) | ⏳ |
| §5.2 M0 feature 隔离 | M0.1-M0.4 | ✅ |
| §5.3 M1 Renderer trait | M1.1-M1.5 | ✅ |
| §5.4 M2 最小 GUI | M2.1-M2.4 | ✅ |
| §5.5-5.7 M3-M5 | 概要 | ⏳ 待展开 |

**2. 占位符扫描**:M3-M5 是明确"概要待 M2 验证后展开",不是占位。每个 Step 都有完整代码或明确指令。无 TBD/TODO。

**3. 类型一致性**:
- `RenderedOutput` / `RenderedCell` / `CellTag` 在 M1.1 定义,M1.2/M1.3/M2.1/M2.3 一致使用
- `Renderer` trait 的 `render(&self, value, atom_type)` 在 M1.2 定义,M1.3 的 TuiRenderer 实现签名一致
- `Block` 在 M2.1 定义,M2.2 一致使用
- `AppMsg` 在 M2.2 定义,M2.3 的 rendered_to_view 一致引用

**4. 已记录的 spec 偏差**:
- 偏差 1:Renderer trait 处理 `&Value + atom_type`,不直接处理 `&AtomPipeline`(因 render_table_with 只处理 Value)
- 偏差 2:M0 工作量比 spec 估的小(shell.rs 只有一处 crossterm)

---

## 执行交接

Plan 完成并保存到 `docs/plans/030-ash-gui.md`。两种执行方式:

**1. Subagent-Driven(推荐)** —— 每个 task 派一个新 subagent 执行,任务间我做 review。M2 是关键检查点,到 M2 后需要真人决策。

**2. Inline Execution** —— 在当前 session 里逐 task 执行,带 checkpoint。

**特殊提醒**:M0 和 M1 可以连续做(无外部决策),但 **M2 完成后必须停下来让真人评估**(§5.10 的生死检查点)。不要自动跳到 M3。
