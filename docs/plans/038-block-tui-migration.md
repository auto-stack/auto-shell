# Plan 038: Block TUI 迁移 — ratatui inline viewport 取代 reedline 终端控制

> **日期**: 2026-08-02
> **分支**: 待建（实验分支，如 `experiment/038-block-tui`）
> **状态**: 调研完成，待实施（**实验性质** — 做出后再决策是否替代当前 reedline 路线）
> **来源**: Plan 037 M3 的后续调研（reedline 无法实现 sticky block → 探索替代架构）
> **预估**: 4 个里程碑，~2500 行，4-6 周

---

## 0. 背景与定位

### 为什么有这个计划

Plan 037 M3 交付了"降级方案"——在每条命令输出前打印一行带色的命令头（`❯ 命令 ... 耗时 ✓/✗`）。这是 reedline 0.44.0 能力的天花板：reedline 是 immediate-mode 行编辑器，无法实现 Warp 式的 sticky block（命令头随输出滚动时钉住）。

本计划探索**激进路线**：用 ratatui 的 inline viewport 取代 reedline 对终端的控制，实现真正的"固定底部编辑器 + 完成的 block 推入 scrollback"体验。这是普通终端（非 Warp 那种自定义模拟器）里能达到的最佳 shell TUI。

### ⚠️ 实验性质 — 重要

**本计划在独立的实验分支上实施，不阻塞主线。** 做出可用的原型后，再决策：
- 若体验显著优于 M3 降级方案 → 合并，替代 reedline 路线
- 若体验提升不明显或代价过大 → 保留 M3 降级方案，归档本计划

**无论结果如何，M3 降级方案（已合并 `feat/037-cli-architecture-cleanup`）都是安全的兜底。**

### 与 Plan 037 的关系

| | Plan 037 M3（已交付） | Plan 038（本计划，实验） |
|---|---|---|
| 终端所有者 | reedline | ratatui (`Viewport::Inline`) |
| 命令头 | 单行 ANSI，随 scrollback 滚走 | block 推入 scrollback，编辑器固定底部 |
| 编辑器 | reedline `read_line`（完整 vi/emacs/history/completion/hints） | ratatui-textarea + 复用 reedline 编辑原语 |
| sticky | ❌ 不可能 | ⚠️ 部分（编辑器固定，但 block body 仍进宿主 scrollback） |
| 风险 | 低（已完成） | 高（重建 completion/hints，子进程交接） |

---

## 1. 调研结论（决策依据）

### 1.1 reedline 不可"半复用"

reedline 0.44.0 的编辑大脑与渲染身体在公共 API 层**不可分离**：

| 组件 | 可见性 | 能否跨 crate 用 |
|---|---|---|
| `LineBuffer`（缓冲区原语，grapheme/word 感知） | **`pub`** | ✅ 可独立实例化操作 |
| `EditMode`/`Emacs`/`Vi` + 默认 keybindings | **`pub`** | ✅ 可把 crossterm Event 转成 ReedlineEvent |
| `EditCommand` enum（编辑词汇） | **`pub`**（`#[non_exhaustive]`） | ✅ |
| `ReedlineRawEvent` + `TryFrom<crossterm::Event>` | **`pub`** | ✅ |
| `FileBackedHistory` / `Completer` / `Highlighter` / `Hinter` / `Validator` | **`pub`**（纯数据 trait） | ✅ |
| `Reedline::handle_event`（事件分发核心） | `pub(crate)` | ❌ 不可用 |
| `Painter::new`（渲染器构造） | `pub(crate)` | ❌ 不可用 |
| `Menu::update_working_details(&Painter)` | 需 `&Painter`（不可构造） | ❌ 卡住 |

**结论**：不能"保留 reedline 的 `read_line`，只替换渲染"。必须**绕过 `read_line`**，自建事件循环 + ratatui 渲染，但可复用 `LineBuffer`/`EditMode`/history/completer/highlighter/hinter 这些纯数据层。

### 1.2 ratatui 的能力与边界

ratatui 是**纯渲染/布局层**，为 shell TUI 提供了关键能力，也有明确缺口：

| 能力 | ratatui 支持 | 说明 |
|---|---|---|
| 固定底部 + 可滚动顶部布局 | ✅ 原生 | `Layout::vertical` + `Constraint::Min`/`Length` |
| **`Viewport::Inline` + `insert_before`** | ✅ **专为 shell 设计** | 底部固定 N 行，完成的 block 推入宿主 scrollback |
| **`scrolling-regions` feature（无闪烁）** | ✅ ratatui-core 0.1.2 已有 | `insert_before` 在此 feature 下用滚动区域，无闪 |
| 行偏移滚动 | ⚠️ 手动 offset | `Paragraph::scroll((y,x))`；无 `Scrollable` widget（RFC #1924 未决） |
| **可编辑文本输入 widget** | ❌ **无** | 必须用 `ratatui-textarea`（第三方）或手写 |
| 语法高亮输入 | ❌ 无 | 渲染 styled `Line`，高亮自己接 syntect |
| **子进程 TTY 交接（vim/less/top）** | ❌ **无** | 必须手写 crossterm 拆除/重建（最难的部分） |
| 虚拟化大 scrollback | ⚠️ 部分 | diff 只输出变化 cell（好）；但 wrapping 是 O(n)（差） |

### 1.3 reedline 与 ratatui 终端控制根本冲突

两者都想拥有 raw mode / 光标 / 转义码。**无法并存。** 现有 ash-tui 之所以能工作，正是因为它**从不让 ratatui 拥有 `Terminal`**——只用 ratatui 渲染到离屏 `Buffer` 再转 ANSI 字符串（`renderer/buffer_to_ansi.rs`），交给 reedline 打印。

> 引证（[users.rust-lang.org](https://users.rust-lang.org/t/line-editor-reedline-rustyline-in-async-ratatui-app/116662)）："Readline libraries and TUI frameworks both want to control the screen... they naturally conflict."

**本计划的本质**：把终端所有权从 reedline 移交给 ratatui。

### 1.4 Warp 的 block 是模拟器/GUI 特性，不是 shell 特性

Warp 用 GPU（Metal/wgpu）直接渲染，block 之所以能 sticky 是因为 **Warp 本身就是终端模拟器**。在宿主终端里运行的 shell **做不到**：
- ❌ pinned header 钉在独立滚动的 body 之上
- ❌ 点击选择任意历史 block
- ❌ per-block 叠加菜单（在 scrollback 上）

**Warp 对全屏程序（vim/less）的处理**：另起一个经典 alternate-screen 终端模拟器视图，**不试图把它们做成 block**。这是被验证的设计，本计划沿用。

**能做到的（Option B 天花板）**：
- ✅ 固定底部编辑器（始终可见）
- ✅ 完成的命令以 block 形式推入宿主 scrollback
- ✅ 样式化命令头 + 输出 body
- ✅ 编辑器内 completion 菜单/hints（ratatui-textarea 之上重建）

### 1.5 三条路线对比（为何选 Option B）

| | Option A（现状/M3） | **Option B（本计划）** | Option C（模拟器/GUI） |
|---|---|---|---|
| 终端所有者 | reedline | **ratatui inline viewport** | 自定义 GPU/Web 渲染 |
| 编辑器 | reedline 完整 | ratatui-textarea + reedline 原语 | 自定义编辑器（如 ash-gui） |
| sticky | ❌ | ⚠️ 部分（编辑器固定） | ✅ 完整 |
| 工作量 | 已完成 | 中（~2500 行） | 大（→ ash-gui 路线） |
| 风险 | 低 | 中高 | 高 |
| 定位 | CLI shell | **CLI shell（普通终端内最佳）** | 终端模拟器/GUI 应用 |

**选 Option B 的理由**：普通终端里能达到的最佳 shell TUI；ratatui `Inline` viewport 正为此设计；`ratatui-textarea` + reedline `LineBuffer` 覆盖编辑；工作量可控；失败有 M3 兜底。Option C 是另一个方向的投资（对接 ash-gui），不在本计划范围。

### 1.6 技术核实结论（依赖兼容性）

- ✅ `scrolling-regions` + `Terminal` + `insert_before` + `Viewport::Inline` **全在 split crate `ratatui-core 0.1.2`**，无需迁移到 umbrella `ratatui`
- ✅ `ratatui-textarea 0.9.x` 依赖 split crate（`ratatui-core ^0.1.1` + `ratatui-widgets ^0.3.1`），与现有栈兼容
- ✅ reedline 的 `LineBuffer`/`EditMode`/`Emacs`/`Vi`/`ReedlineRawEvent`/`FileBackedHistory` 全是 public，可复用
- ⚠️ crossterm 双版本（ash-tui pin 0.27 / reedline 要 0.29）需统一到 **0.29**

---

## 2. 目标架构

### 2.1 目标布局

```
┌─────────────────────────────────┐
│ （宿主终端原生 scrollback）       │  ← 完成的 block 经 insert_before 推入此处
│ ❯ ls -la                  ✓ 12ms │     （ratatui 不再追踪，用宿主 PgUp 翻）
│ file1  file2  file3             │
│ ❯ echo hi                 ✓ 1ms  │
│ hi                              │
│ ...                             │
├─────────────────────────────────┤  ← ratatui Viewport::Inline 的边界
│ ❯ ca|t file.txt                 │  ← 固定底部编辑器（ratatui-textarea）
│   [completion 菜单 / hints]      │  ← 编辑器上方的浮动层
└─────────────────────────────────┘
```

### 2.2 组件分层

```
ash-tui/src/
├── repl.rs              ← 重写：ratatui 事件循环（取代 reedline read_line）
├── editor/              ← 新建：编辑器层
│   ├── mod.rs           ← Editor 组件（持有 LineBuffer + EditMode + history）
│   ├── dispatch.rs      ← EditCommand → LineBuffer 变异的 dispatch（从 reedline 移植）
│   └── history.rs       ← 历史导航（用 FileBackedHistory）
├── block_view.rs        ← 新建：block 渲染（命令头 + body → ratatui Text）
├── completion_menu.rs   ← 新建：ratatui completion 浮动菜单（取代 reedline menu）
├── hints.rs             ← 新建：inline hints 渲染（用 reedline Hinter trait）
├── subprocess.rs        ← 新建：全屏子进程 TTY 交接（crossterm 拆除/重建 ratatui）
└── (保留) renderer/, term/, commands.rs, block_header.rs
```

### 2.3 数据流

```
crossterm Event
    │
    ▼
ReedlineRawEvent::try_from(event)       ← 复用 reedline 的 Event 包装
    │
    ▼
EditMode::parse_event(raw_event)        ← 复用 reedline 的 Emacs/Vi keybinding
    │
    ▼ ReedlineEvent
self.dispatch(event)                    ← 自建：分发到 editor / history / completion / submit
    │
    ▼
ratatui Terminal::draw(frame)           ← 自建：渲染编辑器 + 菜单 + hints
    │
    ▼ (命令提交时)
Terminal::insert_before(block_height, |buf| block_view::render(...))
                                        ← 把完成的 block 推入 scrollback
```

---

## 3. 实施里程碑

### M0：骨架 + 依赖统一（~300 行）

**目标**：实验分支上搭起 ratatui inline viewport 骨架，能显示固定底部框 + 空 block 推入。

**任务**：
1. 从 `feat/037-cli-architecture-cleanup` 建实验分支 `experiment/038-block-tui`
2. 统一 crossterm 到 0.29（改 ash-tui `Cargo.toml`，移除 reedline 0.27 pin）
3. ash-tui `Cargo.toml` 加 `ratatui-core` 的 `scrolling-regions` feature；加 `ratatui-textarea`、`ratatui-crossterm`（后端）
4. 新建 `repl.rs` 的实验骨架：`Viewport::Inline(3)` + 事件循环读 crossterm Event（暂不编辑，仅显示按下的键）+ `insert_before` 推一行测试文本
5. `ash` binary 加一个 `--block-tui` feature flag 切换到新 REPL（保留旧 reedline REPL 作 fallback）

**验收**：`ash --block-tui` 启动，底部有固定 3 行框，按键时把字符推入上方 scrollback，退出恢复正常。无闪烁。

**风险**：crossterm 0.27→0.29 统一可能触发其他 crate 版本波动。

### M1：编辑器（用 reedline 原语 + ratatui-textarea）（~700 行）

**目标**：底部编辑器可输入，有光标、基础编辑、vi/emacs 模式。

**任务**：
1. `editor/mod.rs`：`Editor` 组件持有 `reedline::LineBuffer` + `Box<dyn reedline::EditMode>`（Emacs 或 Vi）+ undo 栈
2. `editor/dispatch.rs`：从 reedline `core_editor/editor.rs:54` 的 `run_edit_command`（`pub(crate)`，~170 行）**移植** `EditCommand → LineBuffer` 变异的 match 分支（逐 arm 搬，不 fork）
3. 事件循环：`crossterm::event::read()` → `ReedlineRawEvent::try_from` → `EditMode::parse_event` → 匹配 `ReedlineEvent::Edit([EditCommand])` → 调 dispatch → 更新 LineBuffer
4. 渲染：把 LineBuffer 内容喂给 `ratatui-textarea`（或直接 `Paragraph` + `set_cursor_position`），`Highlighter` trait 做语法高亮
5. vi 模式切换（normal/insert）、Emacs 基础键绑定（C-a/C-e/C-k/C-w/M-b/M-f）

**验收**：`ash --block-tui` 编辑器可输入文本，C-a/C-e 跳首尾，M-b/M-f 词移动，vi 的 `ESC`/`i`/`w`/`b` 工作。

**风险**：`EditCommand` 是 `#[non_exhaustive]`，新 variant 会在 wildcard arm 暴露（需持续跟进 reedline 版本）。Undo 栈需自建（reedline 的 `EditStack` 是 `pub(crate)`）。

### M2：history + completion + hints（~800 行）

**目标**：编辑器有历史导航（↑↓）、completion 菜单、inline hints——对齐当前 reedline 体验。

**任务**：
1. `editor/history.rs`：用 `reedline::FileBackedHistory`（public）+ 自建 `HistoryCursor`（reedline 的是 `pub(crate)`）；↑↓ 导航、`C-r` 反向搜索
2. `completion_menu.rs`：自建 ratatui 浮动菜单。复用现有 `ShellCompleter`（`completions_reedline.rs`，实现 `reedline::Completer`）的 `complete()` 逻辑；渲染用 ratatui（`Clear` widget 做叠加 + `List`/自定义 widget）
3. `hints.rs`：复用现有 `AshHinter`（`term/hinter.rs`，实现 `reedline::Hinter`）的 `handle()` 返回的 hint 文本；在编辑器右侧/下方灰色渲染
4. Tab 触发 completion、Right/End 接受 hint

**验收**：↑↓ 翻历史；`git c`+Tab 弹 completion 菜单并可选；输入时显示灰色 hint。

**风险**：reedline 的 `Menu::update_working_details` 需 `&Painter`（不可构造），completion 菜单的列布局必须**自建**（`columnar_menu.rs:566-680` 的布局数学可参考移植）。

### M3：block 渲染 + 子进程交接 + 集成（~700 行）

**目标**：完整可用的 block TUI——命令执行后以 block 形式推入 scrollback，vim/less 等全屏程序正确交接。

**任务**：
1. `block_view.rs`：把命令头（复用 `block_header.rs` 的 `render_block_header`）+ 输出 body 组装成 ratatui `Text`，`insert_before` 推入 scrollback
2. `subprocess.rs`：全屏子进程交接——检测命令是否是全屏程序（vim/less/top/man）或经 `show --pager`/`less`；若是，拆除 ratatui（`Terminal::draw` 最终帧 + 离开 inline viewport + `disable_raw_mode`）、`spawn` 子进程继承 stdio、`wait`、重建 ratatui（重锚 inline viewport + 重置双缓冲 + `enable_raw_mode`）
3. 集成 `Shell::execute`（复用 `execute_with_header` 的计时 + exit code 逻辑，但输出走 `insert_before` 而非 `print_command_output`）
4. 非 block-tui 路径（`-c`/`-s`/script）不受影响（仍走旧路径）

**验收**：`ash --block-tui` 跑 `ls`（block 推入）、`echo hi`（block）、`vim file`（进 vim，退出后恢复 block TUI）、`show file.rs --pager`（pager 正常）。

**风险**：子进程交接是最难的部分——ratatui 不检测屏幕变化，子进程返回后必须强制 `Terminal::resize` 或 clear。Windows 上的 raw mode / alt screen 行为需重点测。

---

## 4. 依赖与顺序

```
M0（骨架 + 依赖统一）→ M1（编辑器）→ M2（history/completion/hints）→ M3（block + 子进程）
```

每个里程碑独立可验证。**M0 后即可决策**骨架是否值得继续（ratatui inline 在目标平台是否无闪烁可用）。

## 5. 关键复用清单（不重新发明）

从 reedline 0.44.0 复用（全是 public，无 fork）：
- `LineBuffer`（grapheme/word 感知缓冲区）—— 编辑原语
- `EditMode`/`Emacs`/`Vi` + `default_*_keybindings` —— keybinding 解析
- `ReedlineRawEvent` + `TryFrom<crossterm::Event>` —— 事件包装
- `FileBackedHistory` —— 历史存储
- `Completer`/`Highlighter`/`Hinter`/`Validator` trait —— 现有实现（`ShellCompleter`/`AshHighlighter`/`AshHinter`）直接用

从 reedline **移植**（`pub(crate)`，逐行搬）：
- `Editor::run_edit_command`（`core_editor/editor.rs:54`，~170 行）—— `EditCommand → LineBuffer` dispatch
- `columnar_menu.rs:566-680` 的布局数学（completion 菜单列宽计算）

从现有 ash-tui 复用：
- `block_header.rs`（`render_block_header`）—— block 命令头渲染
- `renderer/`（ratatui Buffer → ANSI）—— 结构化输出渲染
- `completions_reedline.rs` 的 `ShellCompleter.complete()` —— completion 数据源
- `term/hinter.rs` 的 `AshHinter.handle()` —— hint 数据源
- `commands_less.rs` 的 crossterm 交接模式（RAII guard）—— 子进程交接参考

## 6. 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 子进程交接在 Windows 不稳定 | 高 | 高 | M3 重点测；参考 `commands_less.rs` 的 RAII guard；最坏降级为"全屏程序退出后清屏重画" |
| completion 菜单列布局自建工作量大 | 中 | 中 | 移植 reedline `columnar_menu.rs` 的布局数学（自包含） |
| `EditCommand` `#[non_exhaustive]` 持续维护 | 中 | 低 | wildcard arm 兜底；升级 reedline 时编译期暴露新 variant |
| ratatui-textarea 高亮需 DIY | 中 | 低 | 接 syntect（已是依赖）；或暂不做输入高亮 |
| 实验失败浪费投入 | 中 | 中 | M0 后设决策点；每个里程碑可独立终止；M3 降级方案始终兜底 |
| crossterm 0.27→0.29 统一波动 | 低 | 中 | M0 第一步做，及早暴露 |

## 7. 决策点（里程碑评审）

- **M0 后**：ratatui inline viewport 在 Windows/Linux 无闪烁可用？→ 继续/终止
- **M1 后**：编辑器体验是否接近 reedline？→ 继续/补齐/终止
- **M3 后**：整体体验是否显著优于 M3 降级方案？→ **合并替代 / 保留 M3 归档本计划**

## 8. 非目标（明确不做）

- ❌ 成为终端模拟器（Option C，是 ash-gui 的方向）
- ❌ pinned header 钉在独立滚动的 body 之上（宿主终端做不到，需模拟器）
- ❌ 点击选择历史 block（同上）
- ❌ 改 `auto-shell` 逻辑层（Shell/Command 零改动，纯 ash-tui 层）
- ❌ 给 `-c`/`-s`/script 加 block TUI（非交互路径不变）

## 9. 成功指标

1. M0：`ash --block-tui` 显示固定底部 + 无闪烁 block 推入
2. M1：编辑器支持 vi/emacs 基础编辑
3. M2：history/completion/hints 对齐当前 reedline 体验
4. M3：完整 block TUI，含子进程交接；`--block-tui` 与默认 reedline REPL 可切换
5. **最终决策**：体验评审后决定是否设为默认 / 保留实验

---

## 附录 A：调研证据索引

### reedline 0.44.0 源码（`~/.cargo/registry/src/*/reedline-0.44.0/`）
- `src/engine.rs:102-173` — `Reedline` struct（editor/painter/edit_mode/completer 等）
- `src/engine.rs:655` — `read_line`（唯一入口，拥有 raw mode + 事件循环）
- `src/engine.rs:834` — `handle_event`（`pub(crate)`，事件分发核心，不可用）
- `src/engine.rs:951` — `handle_editor_event`（`pub(crate)`，history/menu/submit）
- `src/engine.rs:1483` — `run_edit_commands`（`pub`，但只覆盖 Edit arm）
- `src/engine.rs:1773` — `buffer_paint`（渲染，私有）
- `src/painting/painter.rs:51,90,104` — `Painter`（`BufWriter<Stderr>`，`new` 是 `pub(crate)`）
- `src/core_editor/line_buffer.rs:9` — `LineBuffer`（`pub`，可独立用）
- `src/core_editor/editor.rs:54` — `run_edit_command`（`pub(crate)`，~170 行，需移植）
- `src/edit_mode/base.rs:10` — `EditMode` trait（`pub`）
- `src/enums.rs:62,713,895` — `EditCommand`/`ReedlineEvent`/`ReedlineRawEvent`（`pub`）
- `src/menu/mod.rs:126` — `update_working_details(&Painter)`（卡点，需自建菜单）

### ratatui-core 0.1.2 源码（`~/.cargo/registry/src/*/ratatui-core-0.1.2/`）
- `Cargo.toml:63-86` — `[features]` 含 `scrolling-regions = []`
- `src/terminal/terminal.rs:398,465` — `Terminal`/`TerminalOptions`
- `src/terminal/viewport.rs:62` — `Viewport::{Inline,Fullscreen,Fixed}`
- `src/terminal/inline.rs:109` — `insert_before`（feature-gated 分支）
- `src/buffer/diff.rs` — `BufferDiff`（零分配 diff，性能关键）

### 外部引证
- [How Warp Works](https://www.warp.dev/blog/how-warp-works) — GPU 渲染，block 是模拟器特性
- [Warp Block Model](https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment) — 全屏程序走独立 alt-screen
- [OSC 133 shell integration](https://contour-terminal.org/vt-extensions/osc-133-shell-integration/) — block 边界标记协议
- [ratatui inline example](https://ratatui.rs/examples/apps/inline/) — `Viewport::Inline` 用法
- [ratatui v0.29 scrolling-regions](https://ratatui.rs/highlights/v029/) — 无闪烁修复
- [ratatui-textarea](https://crates.io/crates/ratatui-textarea) — 编辑 widget（依赖 split crate）
- [reedline/rustyline in ratatui 冲突](https://users.rust-lang.org/t/line-editor-reedline-rustyline-in-async-ratatui-app/116662) — 终端控制互斥
