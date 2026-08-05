# Plan 039: ash-gui-vue — Vue3 + shadcn-vue + Tauri 的 ash Block GUI

> **日期**: 2026-08-04
> **状态**: 🚧 **进行中(M1–M4 代码完成,前端 build ✅,后端 cargo check ✅,完整 Tauri 联调待 auto-lang 编译稳定)**
> **来源**: Plan 030(iced 版 ash-gui)的路线修正——iced 版本地渲染效果不佳(表格未对齐、无 block 边界、无 prompt、无 cwd 显示),转用 Vue/Tauri 技术栈重做 UI
> **跨 workspace**: ash-core + auto-shell + ash-gui + auto-lang(生成器约定)
> **预估**: M1–M4,~2500 行(前端 ~1800 + 后端 ~700)
>
> **⚠️ 架构变更(Plan 042)**: 本计划手写了 Tauri 专用后端(`#[tauri::command]`),浏览器版
> 被迫用 `useShellMock` 假数据。Plan 042 将后端提取为独立的 `ash-server` crate,同时支持
> HTTP(axum,浏览器版)和 Tauri IPC(Tauri 版),让两版行为一致。本计划的
> `shell_worker.rs`/`commands.rs` 逻辑将迁入 `ash-server`。

---

## 0. 背景与定位

### 为什么有这个计划

Plan 030 用 Rust + iced 手写了 ash-gui 原型(`ash-gui-bin`),验证了"Shell 引擎与 GUI 渲染分离"的架构可行(worker 线程持有 `!Send` Shell,mpsc + 轮询回流)。但原型渲染质量不达标:

| iced 原型问题 | 根因 | 修复 |
|---|---|---|
| 表格没对齐 | `table_view` 用 `row(cells).spacing(16)`,不是真表格 | Vue 用真 `<table>` + shadcn Table 组件,列宽由浏览器自动对齐 |
| block 无边界 | 只有 `column![header, body]`,无卡片容器 | Vue 用 shadcn `Card`(圆角 + 边框 + 阴影) |
| 没 prompt | 输入框光秃秃,无 `❯` 符号 | `PromptBar` 带 `❯` 提示符 |
| 不知当前目录 | `boot()` 硬编码 `cwd = "."`,且 `Block.cwd` never read | 顶栏 + PromptBar + 每个 block 的 metadata 行都显示 cwd(`~` 缩写) |

### 技术路线决策(2026-08-04)

- **放弃**:纯 iced 打磨(每个样式细节要手写 `Style` 结构体,成本高)
- **放弃**:Vue 当一次性设计草稿(视觉打磨不迁移,UI 成本付两次)
- **选定**:**Vue3 + shadcn-vue + Tailwind + Tauri 作为真正的目标**。理由:
  1. shadcn/Tailwind 让漂亮 UI 近乎免费(圆角/阴影/hover/对齐表格是默认值)
  2. **Auto 工具链最成熟的生成路径就是 Vue+Tauri**(`ui_gen/vue.rs` 557KB 有 Shadcn 模式;`api/targets/tauri.rs` 生成 Tauri commands)——`.at` 复刻成本最低
  3. Tauri 天然解决 `!Send` Shell:Shell 住 Rust 后端,结果经 Tauri event 流回前端(最标准的 Tauri 模式)
- **iced 版保留为参考实现**(`ash-gui-bin`),不动,对照用。

### 与 Plan 030 的关系

Plan 030 的**设计**(Block 模型、RenderedOutput 抽象、18 种 AtomType 的 widget 映射、M0-M5 里程碑)仍然有效。Plan 039 只改变**实现技术栈**(iced → Vue/Tauri),范围 = 030 的 M3 等价物 + 部分 M4。

## 1. 架构

```
┌──────────────────────────────────────────────┐
│  Vue 3 前端 (ash-gui-vue/src)                  │
│  • Pinia 风格 composable: useShell             │
│  • Block 组件(shadcn-vue 原语)                  │
│  • invoke 发命令,listen('command-result') 收结果 │
└──────────────────┬───────────────────────────┘
                   │ Tauri IPC (command + event)
┌──────────────────┴───────────────────────────┐
│  Tauri Rust 后端 (src-tauri)                   │
│  • #[command] run_command → enqueue           │
│  • Shell worker 线程(持有 !Send Shell)          │
│    auto_shell::Shell + tokio::sync::mpsc       │
│  • emit('command-result') 回推                 │
└──────────────────┬───────────────────────────┘
                   │ path dependency
┌──────────────────┴───────────────────────────┐
│  ash-core + auto-shell                        │
│  (唯一改动:渲染类型加 serde::Serialize)          │
└──────────────────────────────────────────────┘
```

### 为什么 worker 线程必须存在

`auto_shell::Shell` 持有 `AutovmReplSession`(auto-lang VM 用 `Rc<RefCell>`,单线程),是 `!Send`。Tauri 的 `State<Mutex<Shell>>` 要求 `Send`,不能直接托管。所以:专用 `std::thread` 拥有 Shell,`tokio::sync::mpsc` 收命令,结果用 `app_handle.emit` 发 Tauri event。

### 与 iced 版 `ShellHandle` 的对应

| iced 版 (`ash-gui-bin/src/main.rs`) | Vue 版 (`shell_worker.rs`) |
|---|---|
| `ShellHandle::spawn` 起 worker 线程 | `spawn(app)` 起 worker 线程 |
| `mpsc::sync_channel` 收命令 | `tokio::sync::mpsc::unbounded_channel` |
| GUI 每 100ms 轮询 `result_rx` | worker 直接 `emit("command-result", …)` |
| `render_structured`(registry→parse→run_atom→render) | 原样搬入 `run_command`/`render_structured` |

## 2. 数据契约(Rust ↔ 前端)

### ash-core 新增 Serialize(对现有代码的唯一改动)

`ash-core/src/renderer.rs`:`RenderedOutput`、`RenderedCell`、`CellTag`、`FileNameKind`、`RenderErrorKind`
`ash-core/src/pipeline/atom.rs`:`AtomType`

serde 已是 ash-core 依赖(`Cargo.toml:29`),每个类型一行 derive。这些类型语义就是"传给渲染器的数据",可序列化是合理职责。

### Tauri 命令

| 命令 | 签名 | 说明 |
|---|---|---|
| `run_command` | `(block_id: usize, cmd: String)` | 送进 worker,立即返回;结果走 event |
| `command_list` | `() -> BootSnapshot` | `{cwd, home, commands[], smart_commands[]}`,worker 异步填充,boot 轮询等待 |
| `open_path` | `(path: String)` | OS 默认程序打开(镜像 iced 版 `open_with_default`) |

### `command-result` event payload

```json
{
  "block_id": 0,
  "cwd": "C:\\Users\\zhaop\\projects\\ash-gui",
  "status": "Success",
  "output": { "Table": { "columns": [...], "rows": [...], "atom_type": "FileList" } },
  "duration_ms": 12
}
```

**serde 陷阱(已修复)**:unit variant `Success` 外部标签序列化为**字符串** `"Success"`,不是 `{"Success": null}`。前端必须 `r.status === 'Success'` 判断(`'Success' in r.status` 恒 false,会误判失败)。

## 3. 前端 Block UI 设计

### 布局

```
┌──────────────────────────────────────────────┐
│ [🛠] ash · ~/projects/ash-gui        (顶栏)    │
├──────────┬───────────────────────────────────┤
│ Commands │ ┌─ Block 卡片 ──────────────────┐  │
│ ls       │ │ ❯ ls -al        ⧉ ↻  12ms ✓  │  │
│ cd       │ │ 📁 ~/projects/ash-gui        │  │
│ ...      │ │ name type size  modified     │  │
│          │ │ src  dir  4096 Aug 4 15:30   │  │
│          │ │ ... (真 <table>, 列对齐)      │  │
│          │ └───────────────────────────────┘  │
├──────────┴───────────────────────────────────┤
│ 补全建议: [ls] [less] [lsusb]                │
│ ❯ ~/projects/ash-gui ▍                      │
└──────────────────────────────────────────────┘
```

### 组件与职责

| 组件 | 职责 |
|---|---|
| `App.vue` | 根布局;浏览器环境自动用 `useShellMock`,Tauri 环境用 `useShell` |
| `ToolSidebar.vue` | 左侧命令 + SmartCommand 列表,点击注入输入框 |
| `BlockList.vue` | 可滚动 block 列表,新增时自动滚到底部 |
| `BlockItem.vue` | 卡片:❯ header + hover 操作按钮(⧉复制/↻重跑)+ cwd 行 + body |
| `BlockBody.vue` | RenderedOutput 分发(script computed 判别,避开模板收窄坑) |
| `TableView.vue` | 真 `<table>` + shadcn Table;列对齐;`FileName`/`Dir` 单元格可点 |
| `RecordView.vue` | 键值列表;`MemoryInfo` 带 `usage_percent` Progress 进度条 |
| `TextView.vue` / `ErrorView.vue` | 等宽文本 / 红色错误卡片 |
| `cellStyle.ts` | `CellTag` → Tailwind class 映射 |
| `PromptBar.vue` | ❯ + cwd(`~` 缩写)+ 输入 + 补全建议 + ↑↓ 历史 + 侧栏注入 |
| `lib/path.ts` | `normalizePath` + `abbrevPath`(对齐 TUI `directory.rs` 规则:~缩写、正斜杠、无截断) |
| `useShell.ts` / `useShellMock.ts` | 真实后端 invoke/listen / 浏览器 mock,形状一致 |

### 配色(对齐 TUI / iced 跨后端一致性)

- 默认深色主题(`index.html class="dark"`)
- 状态色:成功 `text-emerald-500` ✓ / 失败 `text-red-500` ✗ / 运行 `text-amber-500` …
- 文件名着色:`Dir` 天蓝、`.rs`/`.at` 绿、`.exe` 青、配置 金、`Permission` muted(来自 iced `tag_color` 的 RGB 值映射到 Tailwind class)
- prompt `❯`:`text-emerald-500`;cwd `text-sky-300/80`

## 4. 里程碑状态

| 里程碑 | 内容 | 状态 |
|---|---|---|
| M1 | scaffold + ash-core Serialize + tauri dev 起窗口 + `run_command` 契约 | ✅ |
| M2 | Block 列表 + 表格(修对齐/边框/着色),真实链路验证 ls 有结果 | ✅ |
| M3 | PromptBar(❯+cwd)+ 补全 + 历史 + 深色打磨 | ✅(代码) |
| M4 | Record/Text/Error 渲染器 + MemoryInfo 进度条 + 复制/重跑 + 工具侧栏 | ✅(代码) |

## 5. 与未来 `.at` 复刻的衔接

- 前端组件命名/导入严格对齐 Auto `VueMode::Shadcn` 生成器输出:`@/components/ui/*`、`<Table>`/`<Button>`/`<Input>`、`cn()`、CVA variants。
- 布局用 `<div class="flex flex-col/row">`(生成器对 col/row 的输出),不用自定义布局组件。
- 后端 `#[tauri::command]` 将来可被 `api/targets/tauri.rs` 的 `#[api]` 生成器替代。
- 将来 `.at` 复刻 = "把 SFC 内容换成生成器输出",不是重写。

## 6. 风险与回退

| 风险 | 缓解 |
|---|---|
| Tauri 2.10 + ash-core 编译版本坑 | component-gallery 已验证 Tauri 2.10.3 可用,照搬其 Cargo.toml |
| auto-lang WIP 编译不稳定(另一 agent 在 master 实时修改) | 与本次改动无关;联调等 auto-lang 稳定后执行 |
| 浏览器环境无 Tauri runtime | `useShellMock` 提供相同形状的 mock,`npm run dev` 可预览全部 UI |

## 7. 待办

- ⏳ 完整 Tauri 联调(`npm run tauri dev` 全量编译)——待 auto-lang 编译稳定
- ⏳ 验收:ls -al 表格、mem 进度条、cd 后 cwd 变化、↑↓ 历史、补全、侧栏、复制/重跑
- ⏳ M5+ (030 的后续):AI 面板、SmartCommand 表单、`@{block:N}` 引用、流式输出、更多 AtomType 专用 widget
