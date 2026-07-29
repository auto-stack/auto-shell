# Plan 030: ash-gui — Shell-native 应用 UI 设计

> **日期**: 2026-07-21
> **状态**: 设计中(待评审)
> **战略驱动**: 把 ash 的结构化输出(Atom/信封/SmartCommand)渲染成 iced widget,提供 Warp 式的 Block 交互、AI 面板、富补全
> **范围**: ash-gui 独立 iced 应用 + Renderer trait 抽象 + 18 种 AtomType 的 widget 映射
> **跨 workspace**: ash-core + auto-shell + ash-gui
> **预估**: M0-M5 共约 3-5 个月(详见 §6.9)
> **形态**: Shell-native 应用 UI(路径 B),非通用终端模拟器

---

## 愿景

> **ash-gui 是 ash 的 Shell-native GUI 前端**——把 ash 的结构化输出(Atom/信封/SmartCommand)渲染成 iced widget,提供 Warp 式的 Block 交互、AI 面板、富补全。**ash CLI 和 ash-gui 共用同一个 Shell 引擎**,只是渲染出口不同。

### 战略定位:为什么是 Shell-native 而非通用终端

| 路径 | 竞品 | 与 ash 的关系 |
|---|---|---|
| **通用终端模拟器**(PTY + 网格) | Alacritty/WezTerm/WindTerm | 红海;不知道 shell 状态;ash 的结构化优势用不上 |
| **Shell-native 应用 UI**(本 Plan) | Warp/Fig | 蓝海;**ash 的结构化输出天然适配**;Warp 要解析文本反推结构,ash 直接渲染 |

**核心差异化**:Warp 是"bash + UI 外壳",它要反向解析 bash 文本输出才能渲染 Block;**ash 是结构化原生的**,Atom 类型系统 / Plan 028 信封 / Plan 029 SmartCommand 的输出本来就是结构化对象,ash-gui 直接渲染。这是 Warp 永远做不到的根本优势。

### 已有基础(探勘确认,超出预期)

```
✅ Atom 类型系统(18 种)+ Plan 024 结构化 DSL
   ↓ 每个命令输出都带语义标签(FileList/ProcessList/Table/...)
✅ 生产和渲染干净分离(Shell::format_output 唯一分发点)
   ↓ 加 GUI 渲染 = 给现有 seam 加第二个消费者,生产端零改动
✅ Plan 028 信封(EnvelopeData: kind/atom_type/value/pipeline_hint)
   ↓ GUI 知道怎么按 kind 选 widget
✅ Plan 029 SmartCommand(结构化结果 + skill 文档)
   ↓ GUI 能渲染 SmartCommand 表单和执行进度
✅ ash-gui feature-isolation workspace scaffold(ui-iced 能编译)
   ↓ Cargo.toml 配好,启动成本零
✅ AutoUI(iced 后端 + GPUI 后端 + 解释器 + hot reload)
   ↓ 成熟的 widget DSL + 多后端,直接用
✅ Plan 014 分层架构(GUI 前端位置已预留)
   ↓ 设计层面早就在等这一步
```

**七个基础全部就位**。差的只是"把 Atom/信封映射成 iced widget"和"做 GUI event loop"。

### 三条路径(最终选 B)

| 路径 | 描述 | 工作量 | 风险 |
|---|---|---|---|
| **A. 轻路径**:ash 内嵌 TUI | 在现有 ratatui 栈里把 ls/ps 渲染成结构化 widget。不引入 AutoUI | 2-4 周 | 低,但只是 CLI 内美化 |
| **B. 中路径**:**ash-gui 独立应用**(本 Plan) | 用 AutoUI(iced)做 Warp 式应用 | **2-3 个月(渐进 M0-M5 共 3-5 个月)** | 中,但地基已铺好 |
| **C. 重路径**:完整终端 OS | B + PTY 兼容 bash + Tab/SSH/分屏,对标 Warp 全功能 | 6-12 个月 | 高,红海竞争 |

**本 Plan 选 B**,理由:
- A 太轻(只是 CLI 美化,体现不出 Shell-native 的革命性)
- C 太重(红海,且 ash 的护城河不在"通用兼容"而在"结构化原生")
- B 是**差异化最尖锐的切入**

### 范围内 / 范围外

| 范畴 | 本 Plan 包含 | 不包含 |
|---|---|---|
| **前端** | ash-gui 独立 iced 应用(Block 渲染 + 命令输入 + AI 面板) | Tab/分屏/SSH(C 路径) |
| **后端** | 复用 ash-core + auto-shell 的 Shell 引擎(不复制) | 引擎重构 |
| **渲染映射** | Atom/信封 → iced widget(18 种 AtomType) | 新 AtomType |
| **交互** | Block 导航 / 富补全面板 / F4 AI 面板 / SmartCommand 表单 | 自定义主题/配色 |
| **PTY 兼容** | ❌ 不做(纯 Shell-native) | 跑 bash/pwsh(C 路径) |
| **后端选择** | iced(成熟、ash-gui 已 scaffold) | GPUI(后续可选) |
| **集成** | ash CLI 和 ash-gui 完全对等(同一引擎,同一脚本,同一 SmartCommand) | 独立的 GUI-only 功能 |

### 三条核心架构决策

1. **CLI / GUI 引擎共享** —— `ash-core` + `auto-shell` 的 Shell 引擎被两个前端消费。GUI 不复制 shell 逻辑。
2. **渲染器抽象** —— 把 `Shell::format_output` 提升为 trait(`Renderer`),TUI 实现(ANSI string)和 GUI 实现(iced widget)是两个具体实现。生产端零改动。
3. **Block 模型** —— GUI 的核心数据结构是 `Block`(一次命令执行的完整记录),不是字符网格。

---

## 第 1 节:架构与分层(CLI/GUI 引擎共享)

### 1.1 Plan 014 预留的分层架构

```
┌─────────────────────────────────────────────────────────┐
│                    Frontend Layer                        │
│  ┌──────────────────┐    ┌───────────────────────────┐  │
│  │   TUI Frontend    │    │   GUI Frontend (本 Plan)  │  │
│  │   reedline        │    │   AutoUI (iced)           │  │
│  │   ratatui widgets │    │   iced widget tree        │  │
│  │   crossterm       │    │   GPU 加速                │  │
│  └────────┬──────────┘    └────────────┬──────────────┘  │
│           └─────────────┬──────────────┘                 │
│                ┌────────▼─────────┐                      │
│                │  Ash Renderer    │  ← 本 Plan 抽象出    │
│                │  (trait)         │                      │
│                └────────┬─────────┘                      │
└─────────────────────────┼────────────────────────────────┘
                          │
┌─────────────────────────┼────────────────────────────────┐
│                   Backend Layer(零终端依赖)              │
│                ┌────────▼─────────┐                      │
│                │   Ash Engine     │  ← 已存在             │
│                │   ash-core +     │                      │
│                │   auto-shell     │                      │
│                │   (Shell 引擎)   │                      │
│                └──────────────────┘                      │
└──────────────────────────────────────────────────────────┘
```

**Plan 014 早就等这一步**。本 Plan 的工作是:把"`Shell::format_output` 这个硬编码的 ANSI 渲染"抽象成 `Renderer` trait,然后 TUI 和 GUI 各做一个实现。

### 1.2 当前的渲染 seam(探勘确认)

```
命令 run_atom() 产生 AtomPipeline(结构化 Value + AtomType 标签)
         ↓
Shell::format_output(pipeline)  ← 唯一分发点(shell.rs:856)
         ↓ 分支
    ┌────┴────┐
    ↓         ↓
json_output  is_structured?
    │         │
    ↓         ↓
pipeline_   render_table_with()  ← 唯一渲染入口(renderer/table.rs:29)
to_json()     ↓
              ratatui Buffer → buffer_to_ansi() → ANSI String
```

**关键事实**:
- 生产端(命令的 `run_atom`)完全不知道渲染怎么进行——它只返回 `AtomPipeline`
- 渲染端只有一个函数 `render_table_with(&Value, width, icons)`
- `Shell::format_output` 是唯一的"生产 → 渲染"桥接点

### 1.3 Renderer trait(本 Plan 新增,放 ash-core)

```rust
// ash-core/src/renderer.rs(新增,纯逻辑,零终端依赖)

use crate::pipeline::{AtomPipeline, AtomType};

/// 一个命令输出的视觉呈现。前端无关。
///
/// TUI 的 Renderer 实现会把它转成 ANSI String;GUI 的会转成 widget tree。
pub enum RenderedOutput {
    /// 结构化表格(FileList / ProcessList / Table 等)
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<RenderedCell>>,
        atom_type: AtomType,
    },
    /// 单条记录(Record)
    Record(Vec<(String, RenderedCell)>),
    /// 纯文本
    Text(String),
    /// 空输出
    Empty,
    /// 错误
    Error { message: String, kind: RenderErrorKind },
}

#[derive(Clone)]
pub enum RenderedCell {
    Text(String),
    Number(f64),
    Bool(bool),
    Null,
    /// 带语义提示的单元格(文件名可点击、路径可导航)
    Tagged { text: String, tag: CellTag },
}

#[derive(Clone, Copy)]
pub enum CellTag {
    FileName, Path, Url, Pid, Branch, Plain,
}

pub enum RenderErrorKind {
    NotFound, PermissionDenied, NonzeroExit, Other,
}

/// 渲染器 trait —— TUI 和 GUI 各一个实现。
pub trait Renderer: Send {
    fn render(&self, pipeline: &AtomPipeline) -> RenderedOutput;
    fn width_hint(&self) -> u16;
}
```

**关键设计**:`RenderedOutput` 是**前端无关的中间表示**。TUI 实现把它转 ANSI,GUI 实现把它转 iced widget。这让"哪些 AtomType 用什么 widget"的逻辑只写一次。

### 1.4 两个 Renderer 实现

**TuiRenderer**(迁移现有 render_table_with,放 auto-shell):

```rust
// ash/auto-shell/src/frontend/renderer/tui.rs(迁移 + 重构)
pub struct TuiRenderer {
    pub width: u16,
    pub icons: IconStyle,
}

impl Renderer for TuiRenderer {
    fn render(&self, pipeline: &AtomPipeline) -> RenderedOutput {
        render_pipeline_to_structured(pipeline, self.icons)  // 复用 ash-core 纯逻辑
    }
    fn width_hint(&self) -> u16 { self.width }
}

/// 把 RenderedOutput 转成 ANSI String(给 reedline 打印)。TUI 特有最后一公里。
pub fn rendered_to_ansi(rendered: &RenderedOutput, width: u16, icons: IconStyle) -> String { ... }
```

**GuiRenderer**(新增,放 ash-gui-bin):

```rust
// ash-gui-bin/src/renderer.rs
pub struct GuiRenderer { pub width: u16 }

impl Renderer for GuiRenderer {
    fn render(&self, pipeline: &AtomPipeline) -> RenderedOutput {
        render_pipeline_to_structured(pipeline, IconStyle::default())  // 复用同一个纯逻辑函数
    }
    fn width_hint(&self) -> u16 { self.width }
}

/// 把 RenderedOutput 转成 iced widget tree。GUI 特有最后一公里。
pub fn rendered_to_iced<'a>(rendered: &'a RenderedOutput) -> iced::Element<'a, AppMsg> { ... }
```

**复用关键**:`render_pipeline_to_structured` 是 ash-core 里的纯逻辑函数,TUI 和 GUI **调用同一个**。只是最后一步(`rendered_to_ansi` vs `rendered_to_iced`)不同。

### 1.5 Shell 引擎怎么被 GUI 消费(同进程方案)

**决策(v1)**:Shell 引擎嵌入 GUI 进程(同进程,方案 A)。

```rust
// ash-gui-bin/src/main.rs
fn main() -> iced::Result {
    let mut shell = auto_shell::Shell::new();  // ← Shell 在 GUI 进程里
    shell.load_env_persistence();
    iced::run("ash-gui", AshGuiApp::update, AshGuiApp::view)?;
    Ok(())
}
```

**优点**:简单、零 IPC、共享内存;**缺点**:GUI 卡顿会阻塞 Shell、Shell 的 panic 会带崩 GUI。

**v2 视情况考虑方案 B**(Shell 作为子进程,GUI 通过 Plan 028 NDJSON 通信)。v1 不做隔离,目标是证明"结构化渲染的价值",不是"高可用"。

**关键约束**:Shell 不能在 GUI 主线程跑(命令可能很慢)。GUI 里把 Shell 调用丢到 iced 的 `Task`(异步):

```rust
fn update(msg: AppMsg) -> Task<AppMsg> {
    match msg {
        AppMsg::RunCommand(cmd) => Task::perform(
            async move { shell_clone().execute(&cmd) },
            |result| AppMsg::CommandDone(result)
        ),
        // ...
    }
}
```

### 1.6 三块改动的总览

| 改动 | 位置 | 工作量 | 风险 |
|---|---|---|---|
| **Renderer trait + RenderedOutput** | `ash-core/src/renderer.rs`(新增) | 中(纯逻辑,~600 行) | 低 |
| **TuiRenderer**(迁移 render_table_with) | `ash/auto-shell/src/frontend/renderer/tui.rs`(重构) | 中(搬现有代码,~400 行) | 低(回归测试守护) |
| **GuiRenderer + ash-gui 应用** | `ash-gui/ash-gui-bin/src/`(新增) | 大(event loop + widget,~2000 行) | 中 |

**关键**:Renderer trait 的设计是地基,必须先稳。TuiRenderer 是"重构已有代码"(可回归测试),GuiRenderer 是"全新代码"。

### 1.7 与 Plan 028/029 的协同

- **Plan 028 信封** —— GUI 直接消费 `EnvelopeData`,渲染时用 `kind`/`atom_type` 选 widget
- **Plan 028 Tool Registry** —— GUI 可以有"工具浏览器"侧边栏(展示 79 个命令的 schema)
- **Plan 029 SmartCommand** —— GUI 渲染 SmartCommand 的表单、执行进度、确认对话框

ash-gui 是 028/029 的**视觉出口**,让它们的结构化数据第一次以富 UI 形式呈现给用户。

---

## 第 2 节:Atom/信封 → widget 渲染映射

### 2.1 18 种 AtomType 的渲染策略

这是 ash-gui 最实质的设计——**每种语义类型对应一种最佳 widget**。这是 Warp 做不到的(Warp 看到的是文本,要猜结构)。

| AtomType | TUI(现有/改进) | GUI widget(新增) | 交互能力(GUI 独有) |
|---|---|---|---|
| **FileList** | 表格(已有) | 可排序表格 + 图标列 + 路径面包屑 | 点列头排序、点文件名 `open`、右键菜单 |
| **FileEntry** | 键值对列表 | 详情卡片 + 预览缩略图 | 点"打开"调 `open` |
| **ProcessList** | 表格 | 可排序表格 + 进程树视图切换 | 点 PID 发 `kill`、CPU/内存条形图 |
| **ProcessEntry** | 键值对 | 详情卡片 + 父子进程关系图 | 启停进程按钮 |
| **DiskEntry** | 键值对 + 用量条 | 磁盘用量环形图 + 详情 | 挂载/卸载 |
| **CpuInfo** | 键值对 | CPU 拓扑图 + 实时频率 | 切换调速器 |
| **MemoryInfo** | 数字 | 内存用量条 + 进程占用 Top 5 | (只读) |
| **SystemInfo** | 键值对 | 系统概览卡片 | (只读) |
| **MatchList** | grep 结果文本 | grep 结果卡片 + 高亮 + 折叠上下文 | 点文件名打开到匹配行 |
| **CountResult** | 数字 | 大号数字 + 标签 | (只读) |
| **Table** | 表格(已有) | 可排序/筛选表格(SQL 浏览器式) | 列筛选、导出 CSV |
| **Record** | 键值对 | JSON 树视图 + 字段类型标签 | 折叠/展开、复制路径 |
| **Text** | 纯文本 | 富文本 + 语法高亮(检测语言) | 搜索、复制、保存 |
| **Path** | 字符串 | 可点击路径(显示是否存在、文件类型) | 点击导航、拖到其他命令 |
| **BuildResult** | 文本 | 构建状态卡片(成功/失败 + 错误日志折叠) | 点错误跳到源码行 |
| **RunResult** | 文本 | 运行结果卡片(退出码 + 输出 tab) | 重新运行按钮 |
| **HelpInfo** | help 文本 | 帮助文档视图(分节 + 索引) | 搜索、跳转 |
| **Nothing** | (空) | 短暂"✓ done" toast | (无) |

**这张表是 ash-gui 的产品核心**。它把"结构化数据"翻译成"视觉呈现 + 交互能力"。

### 2.2 Block 模型:GUI 的基本数据结构

```rust
// ash-gui-bin/src/block.rs(新增)

/// 一次命令执行的完整记录。GUI 主视图 = 一个 Block 列表。
pub struct Block {
    pub id: BlockId,
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub command: String,
    pub cwd: PathBuf,
    pub exit_code: i32,
    pub output: RenderedOutput,
    pub envelope: Option<serde_json::Value>,
    pub timing: Timing,
    pub sub_blocks: Vec<Block>,
    pub status: BlockStatus,
    pub ai_context: Option<AiContext>,
}

pub enum BlockStatus {
    Success,
    Failed(String),
    Denied(DeniedReason),
    Running { started_at: chrono::DateTime<chrono::Local> },
    Cancelled,
    AwaitingConfirmation { prompt: String },
}

pub struct AiContext {
    pub user_request: Option<String>,
    pub resolved_command: Option<String>,
    pub resolved_args: Option<serde_json::Value>,
    pub conversation_turns: Vec<ConversationTurn>,
}
```

**Block 比字符网格强在哪**:
- 每个 Block 是**可寻址的对象**(能搜索、引用 `@{block:42}` 给 AI)
- Block 有**结构化状态**(成功/失败/被拒/正在跑)
- Block 有**子步骤**(SmartCommand 的 4 步 git 操作是 4 个 sub_block)
- Block 有**AI 上下文**(知道这个命令怎么来的)

这是 Warp Block 概念的**结构化升级**。

### 2.3 主视图:Block 列表

```
┌─ ash-gui ──────────────────────────────────────────────────┐
│  ┌─ Block #42 (成功, 2.3s) ──────────────────────────────┐ │
│  │ ❯ ls -la /sandbox                              [⤴ 复制]│ │
│  │ 📁  ~/sandbox   exit: 0                                │ │
│  │ ┌──────────────────────────────────────────────────┐   │ │
│  │ │ name        size      modified          type     │   │ │ ← FileList
│  │ │ README.md   2.3 KB    2026-07-20 15:30  file     │   │ │   表格 widget
│  │ │ src/        -         2026-07-19 10:00  dir      │   │ │   (可点排序)
│  │ └──────────────────────────────────────────────────┘   │ │
│  └───────────────────────────────────────────────────────┘ │
│  ┌─ Block #43 (失败, 0.1s) ──────────────────────────────┐ │
│  │ ❯ rm /etc/passwd                                       │ │
│  │ 🚫 denied: path-outside-sandbox                        │ │ ← Denied
│  │ ┌─ remediation ─────────────────────────────────────┐  │ │   红色卡片
│  │ │ use /sandbox/etc/passwd or run with --allow-write │  │ │
│  │ └────────────────────────────────────────────────────┘ │ │
│  └───────────────────────────────────────────────────────┘ │
│  ┌─ Block #44 (SmartCommand, 4.2s) ──────────────────────┐ │
│  │ 🤖 smart "finish this worktree"             [展开 AI] │ │ ← SmartCommand
│  │    └ 解析为: git.finish-worktree {push: true}         │ │   带 AI 上下文
│  │    ✓ git commit -m "feat: add widget"    [子步骤 1]   │ │   + 子步骤
│  │    ✓ git checkout main                   [子步骤 2]   │ │
│  │    ✓ git merge feat/030                  [子步骤 3]   │ │
│  │    ✓ git push origin main                [子步骤 4]   │ │
│  └───────────────────────────────────────────────────────┘ │
│  ┌─ 输入 ─────────────────────────────────────────────────┐ │
│  │ ❯ _                                                    │ │ ← 富输入
│  │   [补全建议: ls | grep | find | smart ...]             │ │   + 补全面板
│  └────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────┘
```

### 2.4 渲染分层

```
AtomPipeline(原始)
    ↓ Renderer trait.render()
RenderedOutput(前端无关中间表示)
    ↓ TuiRenderer 终端路径 / GuiRenderer GUI 路径
    ├── TUI: rendered_to_ansi() → String → reedline 打印
    └── GUI: rendered_to_iced() → iced::Element → 嵌入 Block widget
                ↓
            Block(含 RenderedOutput + 元数据 + AI 上下文)
                ↓
            Block 列表 widget(主视图)
```

**关键不变量**:`RenderedOutput` 是前端无关的。TUI 和 GUI 对同一个 `ls` 输出产生**同一个 `RenderedOutput`**,只是最后一公里(ANSI vs iced widget)不同。

### 2.5 交互能力(从 RenderedCell 到事件)

`RenderedCell::Tagged { tag: CellTag }` 让 GUI 能给单元格附加语义,从而支持点击交互:

| CellTag | GUI 行为 |
|---|---|
| `FileName` | 双击 → `open <file>`;右键 → cp/mv/rm 菜单 |
| `Path` | 单击 → `cd <path>`;拖到输入框 → 自动填入 |
| `Url` | 单击 → 系统浏览器打开 |
| `Pid` | 右键 → `kill <pid>` 菜单 |
| `Branch` | 单击 → `git checkout <branch>` |
| `Plain` | 无特殊交互 |

**这是 GUI 独有的能力**——TUI 只能渲染,不能交互。CellTag 让每个单元格都是 actionable。

### 2.6 渲染失败 / 非结构化输出的 fallback

不是所有输出都是结构化的。对这些,GUI 渲染成:
- **Text** → 富文本 widget(自动检测语言做高亮,如检测到 JSON 就格式化+折叠)
- **Error** → 红色错误卡片 + remediation 提示 + "重试"按钮
- **Empty** → 短暂的 "✓ done" toast(3 秒后消失)

这保证 GUI 对**任何**命令都有合理的视觉呈现。

---

## 第 3 节:AutoUI 集成 + iced 后端

### 3.1 关键问题:用 AutoUI 还是直接写 iced?

三种用法:

**方案 A**:直接写 iced widget tree(不碰 AutoUI DSL)
- 优点:简单直接,Rust 原生类型安全
- 缺点:不用声明式 DSL;UI 改动要改 Rust + 重编译

**方案 B**:用 AutoUI widget DSL(.at 声明式)
- 优点:声明式、可热重载、跟 25 个 UI 示例同构
- 缺点:学习曲线;ash-gui 的 UI 主要是"渲染结构化数据",声明式原语可能不够

**方案 C(推荐):混合 —— 骨架用 AutoUI,渲染层直接 iced**

把 UI 分两层:
- **应用骨架**(主窗口、侧边栏、AI 面板)—— 用 AutoUI widget DSL,可热重载
- **渲染层**(RenderedOutput → 具体 widget)—— 直接写 iced,因为这是跟 ash 数据结构紧耦合的代码

```rust
// AutoUI 描述主窗口骨架(可热重载)
widget AshGuiMain {
    model { var blocks Vec<Block>, var input_text String }
    view {
        column {
            for_each(.blocks) { |b| BlockWidget { block: b } }
            CommandInput { text: .input_text, on_submit: .RunCommand }
        }
    }
}

// BlockWidget 嵌入"渲染层"(直接 iced)
widget BlockWidget {
    model { var block Block }
    view { column { text `❯ ${.block.command}`; render_output(.block.output) } }
}

// render_output 是 AutoUI 的 native 桥接(类似 Flutter 的 platform view)
fn render_output(output: RenderedOutput) -> iced::Element {
    rendered_to_iced(&output)  // 复用 §2.4 的渲染层
}
```

**为什么 C 最佳**:这是 Flutter / SwiftUI 的成熟模式——声明式骨架 + 命令式 escape hatch。

### 3.2 AutoUI 的 Component trait 对接

探勘确认 AutoUI 的核心是 `Component` trait(Elm/TEA):

```rust
pub trait Component: Sized {
    type Msg;
    fn on(&mut self, msg: Self::Msg);
    fn view(&self) -> View<Self::Msg>;
}
```

ash-gui 主应用实现这个 trait:

```rust
// ash-gui-bin/src/app.rs
pub enum AppMsg {
    RunCommand(String),
    CommandDone(BlockId, Result<RenderedOutput, RenderedOutput>),
    CancelBlock(BlockId),
    ToggleAiPanel,
    SmartCommandConfirm(BlockId, bool),
    ClickCell(BlockId, usize, usize, CellTag),
}

pub struct AshGuiApp {
    blocks: Vec<Block>,
    next_block_id: u64,
    input_text: String,
    ai_panel_open: bool,
    shell: Shell,
}

impl Component for AshGuiApp {
    type Msg = AppMsg;
    fn on(&mut self, msg: Self::Msg) { /* 状态更新 */ }
    fn view(&self) -> View<AppMsg> { /* 声明式 view */ }
}
```

### 3.3 iced 后端的具体接入

AutoUI 的 iced 后端已存在(`crates/auto-lang/src/ui/iced/`,有 `run_app` / `ComponentIced`)。ash-gui 用它启动:

```rust
// ash-gui-bin/src/main.rs
use auto_lang::ui::iced::run_app;

fn main() -> iced::Result {
    let app = app::AshGuiApp::new();
    run_app(app, "ash-gui")
}
```

### 3.4 启动顺序与 feature 隔离

```
D:\autostack\auto-shell\
├── ash\                  ← CLI workspace(auto-lang 不带 ui-iced)
│   └── auto-shell\
├── ash-gui\              ← GUI workspace(auto-lang 带 ui-iced)← 已存在 scaffold
│   └── ash-gui-bin\
└── ash-core\             ← 两个 workspace 共享(纯逻辑,无 UI 依赖)
```

**关键约束**:
- `ash-core` 绝不带 UI 依赖(Plan 014 已定)
- `Renderer` trait + `RenderedOutput` 放 `ash-core`(纯逻辑)
- `TuiRenderer` 放 `auto-shell`(依赖 reedline/ratatui,无 iced)
- `GuiRenderer` 放 `ash-gui-bin`(依赖 iced,无 reedline)

### 3.5 Shell 引擎怎么被两个 workspace 用?

**问题**:`auto-shell` 依赖 reedline/crossterm/ratatui。GUI 不想要它们。

**三种解法**:

- **A.ash-gui 依赖 auto-shell 全量**(简单,但拖累)
- **B.把 Shell 引擎拆出 `ash-engine` crate**(Plan 014 终态,工作量大)
- **C(v1 推荐).feature flag 隔离**:auto-shell 加 `frontend-tui` feature(默认开),ash-gui 关掉它

**决策**:v1 用 C。把 reedline/crossterm/ratatui 的代码用 `#[cfg(feature = "frontend-tui")]` 包起来。这是 Plan 030 的 M0 前置工作。

### 3.6 渲染层的共享逻辑

`Renderer` trait 放 `ash-core`(零 UI 依赖)。但 `rendered_to_iced()` 是 iced 特有的,只能放 `ash-gui-bin`。两个 workspace 怎么共享"AtomType → widget 选择"的逻辑?

**关键**:把**选择逻辑**(哪种 AtomType 用哪种 RenderedOutput 变体)放 ash-core,把**具体渲染**(RenderedOutput → iced/ANSI)放各自前端。

```rust
// ash-core/src/renderer.rs(共享)
pub fn atom_pipeline_to_rendered(pipeline: &AtomPipeline, icons: IconStyle) -> RenderedOutput {
    match pipeline {
        AtomPipeline::Atom(atom) => match atom.atom_type() {
            AtomType::FileList => render_file_list(&atom.value, icons),
            // ... 18 种
        },
        // ...
    }
}

// auto-shell/frontend/renderer/tui.rs(TUI 专用)
pub fn rendered_to_ansi(r: &RenderedOutput, width: u16) -> String { ... }

// ash-gui-bin/src/renderer.rs(GUI 专用)
pub fn rendered_to_iced(r: &RenderedOutput) -> iced::Element { ... }
```

`atom_pipeline_to_rendered` 写一次,两边复用。

---

## 第 4 节:交互设计(Blocks / 补全 / AI)

### 4.1 富命令输入(区别于 readline)

CLI 用 reedline(单行 + 历史 + 基础补全)。GUI 的输入区是**多行富文本编辑器**:

| 能力 | CLI(现有) | GUI(本 Plan) |
|---|---|---|
| 单行输入 | ✅ | ✅ |
| 多行编辑 | ❌ | ✅(Shift+Enter 换行) |
| 语法高亮 | ✅ 有限 | ✅ 完整(管道/重定向/AutoLang) |
| 补全面板 | reedline menu | ✅ 富 widget(图标+文档+类型) |
| 命令历史搜索 | Ctrl+R | ✅ fuzzy + 时间筛选 |
| 拖放 | ❌ | ✅(从 Block 拖路径) |

**补全面板示例**:

```
┌─ 输入 ──────────────────────────────────────────────┐
│ ❯ ls -la /s                                        │
├─────────────────────────────────────────────────────┤
│  📁 /sandbox/   📁 /src/   📁 /system/             │ ← 路径补全
├─────────────────────────────────────────────────────┤
│  🛠 ls      List directory contents   [file_list]   │ ← 命令补全
│  🤖 smart   Run a SmartCommand        [smart_nl]    │   含 AtomType 标签
│  🔧 sort    Sort lines               [table]       │
└─────────────────────────────────────────────────────┘
```

补全每项带图标 + 描述 + 输出类型标签。这是 Warp 式补全的**结构化升级**——Warp 只能补命令名,ash-gui 告诉你"这个命令会产出什么类型的数据"。

### 4.2 Block 导航与操作

**全局快捷键**:

| 快捷键 | 行为 |
|---|---|
| `Ctrl+↑/↓` | 在 Block 间跳转 |
| `Ctrl+Shift+P` | 命令面板(搜索历史 Block) |
| `Ctrl+F` | 在所有 Block 输出里搜索 |
| `Ctrl+L` | 清屏(archive,不真删) |
| `Ctrl+S` | 保存当前 Block 输出 |

**单 Block 操作**(右上角按钮):

- **⤴ 复制**:命令 / 输出 JSON / 输出 Markdown 表格
- **🔄 重跑**:同命令+cwd 重新执行
- **⭐ 收藏**:加入侧边栏(跨 session 持久化)
- **⋯ 更多**:导出、分享为链接、设为 alias

**Block 引用**(GUI 独有):

```bash
# 引用 Block 42 的输出作为新命令输入
❯ grep "TODO" @{block:42}

# 引用 Block 42 的命令本身
❯ @{block:42.command} | sort

# 在 AI 面板里引用 Block 42 给 LLM 解释
? 这个输出是什么意思? @{block:42}
```

Warp 的 Block 只能视觉滚动到,不能数据引用。

### 4.3 AI 面板(集成 Plan 027/029)

GUI 右侧或底部是 AI 面板,集成:
- **Plan 027 F4 chat**(对话式 AI)
- **Plan 029 SmartCommand NLU**(自然语言 → 命令)
- **Block 解释**(选 Block 让 AI 解释)

```
┌─ AI 面板 ──────────────────────────────┐
│ 🤖 选择模式:                            │
│   ○ 对话(F4 chat)                     │
│   ● SmartCommand(NLU)                  │
│   ○ 解释选中 Block                      │
├─────────────────────────────────────────┤
│ 用户: finish this worktree and push    │
│                                         │
│ 🤖 解析为:                              │
│    git.finish-worktree {                │
│      target: "main", push: true,        │
│      message_source: "diff"             │
│    }                                    │
│    [✓ 执行] [✏ 编辑参数] [✗ 取消]      │
└─────────────────────────────────────────┘
```

AI 面板和 Block 列表**双向联动**——AI 建议的命令执行后成为 Block,Block 可以被拖进 AI 面板让 AI 解释。

### 4.4 SmartCommand 表单渲染

SmartCommand 的 `command.at` 有结构化 args schema。GUI 把它渲染成**表单**:

```
┌─ SmartCommand 表单:git.finish-worktree ───────────────┐
│  target:     [auto           ▼]  (auto/main/master)   │
│  push:       [☑]                                     │
│  message_source: [diff     ▼]  (diff/plan/manual)     │
│  plan_file:  [(可选) 留空     ]                       │
│  [✓ 执行]  [✏ 转为命令行]  [⭐ 保存为快捷方式]        │
└────────────────────────────────────────────────────────┘
```

- "转为命令行"翻译成 `ash smart git.finish-worktree --target main --push`
- "保存为快捷方式"把这套参数存起来,下次一键执行

### 4.5 危险操作确认(GUI 化)

Plan 029 的 `confirm_before` 在 CLI 是 `confirm("继续? (y/n)")`。GUI 是**模态对话框**:

```
┌─ ⚠️ SmartCommand 确认 ────────────────────────────┐
│  git.finish-worktree 即将执行:                     │
│   commit message: "feat: add widget renderer"      │
│   merge 回:       main                             │
│   push:           true                             │
│   删除分支:       feat/030                         │
│  [✓ 确认执行]  [✗ 取消]  [✏ 编辑参数]            │
└────────────────────────────────────────────────────┘
```

比 CLI 的 y/n 更清晰(展示所有将发生的副作用)。

### 4.6 工具浏览器侧边栏(Plan 028 协同)

Plan 028 的 Tool Registry 有 79 个命令 + N 个 SmartCommand 的完整 schema。GUI 把它渲染成**可浏览工具库**:

```
┌─ 工具库 ────────────────────┐
│  🔍 搜索工具...              │
├──────────────────────────────┤
│  📁 文件操作                  │
│    ls   cat   cp   mv   rm   │
│  📊 数据处理                  │
│    sort   uniq   wc   cut    │
│  🤖 SmartCommand             │
│    git.finish-worktree       │
│  🌐 网络                     │
│    http_get   http_post ...  │
└──────────────────────────────┘
```

点任意工具 → 展开它的 schema → "使用"按钮填入输入框。这是 ash-gui 的**可发现性**——用户不用记 79 个命令。

### 4.7 主题与配色(范围外,但预留)

v1 用 iced 默认主题 + 一个深色主题。自定义主题/配色/字体不在 v1。但 Renderer trait 要把"样式"作为参数(不硬编码),未来切主题不用改渲染逻辑。

---

## 第 5 节:里程碑与三路径落地

### 5.1 整体策略:渐进式,每个 M 验证一个假设

```
M0(前置):auto-shell feature 隔离
    ↓
M1:Renderer trait + RenderedOutput(纯逻辑)
    ↓
M2:最小可用 GUI(单 Block + 单 AtomType)  ← 关键验证点
    ↓
M3:Block 列表 + 富输入 + 基础补全
    ↓
M4:18 种 AtomType 全渲染 + CellTag 交互
    ↓
M5:AI 面板 + SmartCommand 表单 + 工具浏览器
```

每个 M 都能独立验证。M2 后发现"结构化渲染没比文本好多少"可以止损。

### 5.2 M0:auto-shell feature 隔离(前置)

**目标**:让 ash-gui 能依赖 auto-shell 但不拉 reedline/crossterm。

**交付物**:
- `auto-shell/Cargo.toml` 加 `[features]`,默认 `default = ["frontend-tui"]`
- 把 `frontend/` 用 `#[cfg(feature = "frontend-tui")]` 包起来
- `shell.rs` 里 `crossterm::terminal::size()` 提取为参数
- 验证:`cargo build -p auto-shell --no-default-features` 能编译

**风险**:`Shell::format_output` 现在硬编码调 crossterm。需要重构为接受 width 参数。

**规模**:中等(~500 行)。

### 5.3 M1:Renderer trait + RenderedOutput(地基)

**目标**:`ash-core` 里有完整 `Renderer` trait + `RenderedOutput` + `atom_pipeline_to_rendered()`。TUI 重构为用它,回归测试全过。

**交付物**:
- `ash-core/src/renderer.rs` —— `Renderer` trait + `RenderedOutput` + `RenderedCell` + `CellTag` + `atom_pipeline_to_rendered()`
- 18 种 AtomType 的渲染逻辑
- `auto-shell/src/frontend/renderer/tui.rs` —— `TuiRenderer` + `rendered_to_ansi()`
- 迁移 `Shell::format_output` 用 `Renderer` trait
- 全量回归测试

**验证**:
- `cargo test` 全绿(028 的 676+375 测试不受影响)
- 视觉无变化(ls/ps 输出跟重构前一模一样)

**规模**:大(~1200 行新增 + 重构)。

### 5.4 M2:最小可用 GUI(关键验证)

**目标**:一个能跑的 ash-gui 窗口,输入 ls,看到结构化表格 widget。

**交付物**:
- `ash-gui-bin/src/main.rs` —— iced 应用入口
- `ash-gui-bin/src/app.rs` —— `AshGuiApp` 实现 `Component`
- `ash-gui-bin/src/block.rs` —— `Block` 数据结构
- `ash-gui-bin/src/renderer.rs` —— `GuiRenderer` + `rendered_to_iced()`
- 最小 Block 视图 + 最小输入框
- **只渲染 1 种 AtomType**:FileList(`ls` 的输出)

**验证场景**:
```
# 在 ash-gui 里输入:
ls
# → 看到结构化表格 widget
# → 点列头能排序(TUI 做不到)
# → 点文件名能 open(TUI 做不到)
```

**这个 M 的意义**:证明"结构化 Atom → 富 widget"的核心假设。M2 是最关键检查点。

**规模**:中等(~1500 行,但只覆盖一种 AtomType)。

### 5.5 M3:Block 列表 + 富输入 + 基础补全

**目标**:能当日常终端用。

**交付物**:
- 完整 Block 列表视图(多 Block、滚动、状态着色)
- 富命令输入(多行、语法高亮、Shift+Enter)
- 基础补全(路径 + 命令名)
- Block 操作(复制、重跑、收藏)
- 历史搜索(Ctrl+R 升级版)
- Block 持久化

**验证**:连续用 ash-gui 做 30 分钟真实工作(代替 CLI)。如果"舒服",M3 达标。

**规模**:大(~2000 行)。

### 5.6 M4:18 种 AtomType 全渲染 + CellTag 交互

**目标**:所有命令输出都有合适的 widget,且可交互。

**交付物**:
- 完整 `rendered_to_iced()` 覆盖 18 种 AtomType
- CellTag 系统(FileName/Path/Url/Pid/Branch 的点击行为)
- 进程树视图(ps)、磁盘图(du)、构建卡片(build)等专门 widget
- Error 卡片 + remediation 提示
- Text 的自动语言检测 + 高亮

**验证**:每种 AtomType 都有对应命令测试,全部渲染正确。

**规模**:大(~2500 行,主要是 18 种 widget)。

### 5.7 M5:AI 面板 + SmartCommand + 工具浏览器

**目标**:Warp 对标完成,差异化清晰。

**交付物**:
- AI 面板(集成 Plan 027 F4 chat + Plan 029 NLU)
- SmartCommand 表单(从 command.at 推导)
- SmartCommand 确认对话框
- 工具浏览器侧边栏
- Block 引用(`@{block:42}` 语法)
- Block 解释(选 Block 让 AI 解释)

**验证**:
- 用 AI 面板的 SmartCommand 路径完成一次真实的 `finish-worktree`(端到端)
- 工具浏览器能找到并使用所有 79 命令
- Block 引用能在新命令里拿到正确数据

**规模**:大(~2000 行)。**依赖 Plan 029 落地**。

### 5.8 里程碑依赖图

```
Plan 028 M1+M2(已完成)─── Plan 029(设计完成,待实施)
                              │
                              │ (M5 需要 029 的 SmartCommand)
                              ↓
M0(feature 隔离)─→ M1(Renderer trait)─→ M2(最小 GUI)
                                              ↓
                                           M3(日常可用)
                                              ↓
                                           M4(全 AtomType)
                                              ↓
                                           M5(AI + SmartCommand)
```

Plan 030 跟 Plan 029 **可以并行**——M0-M4 不依赖 SmartCommand,M5 才需要。

### 5.9 工作量与时间估算

| 里程碑 | 代码行 | 估算时间 | 可并行 029? |
|---|---|---|---|
| M0 feature 隔离 | ~500 | 1 周 | ✅ |
| M1 Renderer trait | ~1200 | 2-3 周 | ✅ |
| M2 最小 GUI | ~1500 | 2-3 周 | ✅ |
| M3 日常可用 | ~2000 | 3-4 周 | ✅ |
| M4 全 AtomType | ~2500 | 3-4 周 | ✅ |
| M5 AI + SmartCommand | ~2000 | 2-3 周 | ❌(需 029) |
| **总计** | **~9700 行** | **13-20 周(3-5 个月)** | |

估算偏保守——AutoUI/iced 成熟度让很多 widget 不用从零写。

### 5.10 验证假设的检查点

| M | 继续的信号 | 止损的信号 |
|---|---|---|
| M2 | "点列头排序确实比文本好" | "跟 TUI 视觉差不多,不值得开 GUI" |
| M3 | "能用 ash-gui 做 30 分钟真实工作不别扭" | "我还是想回 CLI" |
| M4 | "每种输出都有合适渲染,信息密度高" | "18 种里有 12 种都退化成文本" |
| M5 | "AI 面板 + Block 引用是杀手锏" | "AI 面板可有可无" |

**M2 是最关键的检查点**。如果 M2 不让人兴奋,M3-M5 都不该做。

---

## 第 6 节:与现有产品的对比、风险、非目标

### 6.1 与现有产品的精确对比

| 维度 | bash+WinTerm | Warp | Alacritty | **ash-gui** |
|---|---|---|---|---|
| 底层 shell | bash/pwsh | bash/zsh | 任意(PTY) | **ash 专属** |
| 输出结构化 | ❌ | ⚠️ 解析文本 | ❌ 字符网格 | **✅ 原生 Atom** |
| 命令输出渲染 | 纯文本 | Block + 表格 | 字符网格 | **18 种专用 widget** |
| 可点击交互 | ❌ | ⚠️ Block 级 | ❌ | **✅ 单元格级** |
| Block 数据引用 | ❌ | ⚠️ 视觉滚动 | ❌ | **✅ `@{block:42}`** |
| AI 集成 | ❌ | ✅ | ❌ | **✅ F4+SmartCommand** |
| 命令可发现性 | 记忆/man | 命令面板 | ❌ | **工具浏览器** |
| SmartCommand | ❌ | ❌ | ❌ | **✅ 表单+AI** |
| 跑其他 shell | ✅ | ✅ | ✅ | **❌(Shell-native)** |
| 成熟度 | 极高 | 高(17B) | 高 | **0** |
| 生态 | 巨大 | 中 | 大 | **小** |

**核心差异化**(Warp 做不到):原生结构化 / SmartCommand 表单 / Block 数据引用 / 工具浏览器。

**核心劣势**(必须正视):只能跑 ash / 从零开始 / 生态小 / Windows Terminal 免费。

### 6.2 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| **M2 验证失败**(结构化渲染没比文本好) | 中 | 致命 | M2 硬检查点,止损成本最低(3-4 周) |
| **iced 性能不够**(大量 Block 卡顿) | 中 | 高 | AutoUI GPUI 后端作 fallback;iced lazy 渲染 |
| **M0 feature 隔离伤筋动骨** | 中 | 中 | 先做 spike;若 cfg 要动 50+ 文件,改为全量依赖(方案 A) |
| **AutoUI widget 原语不够** | 高 | 中 | §3.1 混合方案,渲染层直接 iced |
| **跨平台一致性** | 高 | 中 | v1 主攻一个平台(建议 Windows),其余 v2 |
| **Plan 029 延期**(M5 阻塞) | 中 | 中 | M0-M4 不依赖 029;M5 可降级(只做 F4) |
| **auto-lang 再被打断** | 高 | 中 | pin 到稳定 commit;只用 ui-iced feature(相对稳定) |
| **投入 3-5 个月后用户不买单** | 中 | 致命 | 渐进式 M + 检查点;M2 就能判断生死 |
| **与 Warp 功能差距** | 高 | 中 | v1 不做 Tab/分屏/SSH,定位"ash 专属" |
| **iced Windows GPU/渲染问题** | 中 | 高 | M2 在 Windows 上验证;有坑则 GPUI 或 winit fallback |

### 6.3 非目标(明确排除)

- ❌ **PTY 兼容**(跑 bash/pwsh) —— 形态 A 范畴
- ❌ **Tab / 分屏 / SSH** —— C 路径,v2
- ❌ **远程 shell** —— 需要 028 M3(NDJSON)+ 客户端协议
- ❌ **自定义主题/配色/字体** —— v1 默认主题;Renderer 参数化预留
- ❌ **插件系统** —— 跟 ash 插件生态(C)一起做
- ❌ **全平台一致** —— v1 主攻一个平台
- ❌ **GPUI 后端** —— v1 用 iced
- ❌ **替代 ash CLI** —— CLI 永远保留,双前端

### 6.4 与其他 Plan 的关系

| 关联 Plan | 关系 |
|---|---|
| **Plan 014**(分层架构) | **本 Plan 是 014 的 GUI 前端落地** |
| **Plan 028**(Agent 执行引擎) | 强协同:GUI 消费信封、展示工具库 |
| **Plan 029**(SmartCommand) | M5 协同;M0-M4 不依赖 |
| **Plan 027**(F4 chat) | M5 集成 |
| **Plan 024**(Atom DSL) | 强依赖:18 种 AtomType 是渲染映射输入 |
| **MS2**(沙箱) | 强协同:GUI 里 system() 同样受 policy 约束 |

### 6.5 成功指标

1. **M2 检查点通过** —— 真人用户看到 ls 结构化表格后主观评价"比文本好"
2. **18 种 AtomType 全部有专用 widget** —— 每种有对应命令的渲染测试
3. **能用 ash-gui 做真实工作** —— 1 小时日常开发(代替 CLI)
4. **SmartCommand 表单可用** —— GUI 完成一次 `finish-worktree`
5. **Block 引用工作** —— `grep X @{block:42}` 拿到正确数据
6. **028 Agent CLI 测试在 GUI 下也通过** —— 同一 Shell 引擎,两个前端,行为一致
7. **性能可接受** —— 1000 Block 滚动 60fps
8. **auto-shell feature 隔离稳定** —— `--no-default-features` 干净编译

### 6.6 后续 Plan(明确不在 030 范围)

- **Plan 031**:ash-gui Tab/分屏/会话管理
- **Plan 032**:主题系统 + 配色编辑器
- **Plan 033**:远程模式(SSH + 客户端协议)
- **Plan 034**:GPUI 后端(如果 iced 性能不够)
- **Plan 035**:插件系统(第三方 widget 扩展)
- **Plan 036**:AutoCoder TUI Agent 应用(用 ash-gui 作外壳)

---

## 附录 A:实施前置勘探记录(2026-07-21)

本设计基于一次代码勘探,关键发现:

### ash-gui scaffold 现状
- `ash-gui/` 是独立 workspace(隔离 ui-iced feature,不污染 CLI workspace)
- `ash-gui-bin/Cargo.toml` 已配 `auto-lang = { features = ["ui-iced"] }`
- `main.rs` 只有 19 行,是 feature canary(证明 ui-iced 能编译),无任何 GUI 代码
- 启动成本零

### ash 当前渲染栈
- 用 `ratatui-core` + `ratatui-widgets`(不带 crossterm backend)做**内存渲染**
- `buffer_to_ansi.rs` 把 ratatui Buffer 转成 ANSI string(不接管终端)
- **生产/渲染干净分离**:`Shell::format_output`(shell.rs:856)是唯一分发点
- `render_table_with`(renderer/table.rs:29)是唯一渲染入口
- 命令的 `run_atom` 只返回 AtomPipeline,不碰渲染

### AutoUI 集成状态
- `auto-shell` 当前**不**用 ui-iced feature(CLI workspace 隔离)
- 零 `use auto_lang::ui::*` 调用
- 零 `use iced` 调用
- AutoUI 的 iced 后端已就绪(`run_app` / `ComponentIced` / `IntoIcedElement`)
- GPUI 后端也存在(Zed 的框架)

### Plan 014 的预留
Plan 014(2026-06-10)早就设计了:
- 分层架构(Backend 零终端依赖 + Frontend TUI/GUI 双前端)
- GUI 前端的位置("未来支持 TUI+GUI 双前端借助 AutoUI")
- 关键发现:"ratatui 可以不接管终端,Buffer+Widget::render 内存渲染后转 ANSI"

### 关键设计推论
基于以上发现,ash-gui 设计:
1. **Renderer trait + RenderedOutput** 是前端无关的中间表示,放 ash-core
2. **TuiRenderer** 迁移现有 render_table_with(重构,回归测试守护)
3. **GuiRenderer** 直接产 iced widget(新增)
4. **混合 AutoUI**:骨架用 AutoUI DSL(热重载),渲染层直接 iced
5. **同进程方案**(v1):Shell 嵌入 GUI 进程,通过 iced Task 异步执行
6. **feature 隔离**(M0):auto-shell 加 `frontend-tui` feature,GUI 关掉它

---

## 参考

- `docs/plans/014-ash-layered-architecture-ratatui.md` —— 分层架构(GUI 前端位置已预留)
- `designs/028-agent-execution-engine.md` —— Plan 028,GUI 消费它的信封和 Tool Registry
- `designs/029-smartcommand.md` —— Plan 029,M5 集成它的 SmartCommand
- `D:\autostack\auto-lang\crates\auto-lang\src\ui\` —— AutoUI 模块(iced/gpui/headless 后端)
- `D:\autostack\auto-lang\examples\ui/` —— 25 个 AutoUI 示例(counter/todo/chat/...)
- `D:\autostack\auto-shell\ash-gui\ash-gui-bin\src\main.rs` —— 当前 19 行 scaffold
- `D:\autostack\auto-shell\ash\auto-shell\src\shell.rs:856` —— `Shell::format_output` 唯一渲染分发点
- `D:\autostack\auto-shell\ash\auto-shell\src\frontend\renderer\table.rs:29` —— `render_table_with` 唯一渲染入口
