# Plan 043: ash-gui 反向生成 Auto 语言版 — 从手写 Vue 到 .at 源码

> **日期**: 2026-08-05
> **状态**: 🔄 实施中(M1-M4 完成,M5 阻塞于 auto-lang parser/codegen 限制)
> **来源**: Plan 042 完成(v0.1.6),ash-gui 的 Vue 手写版已可用且功能完备。下一步:
> 从手写 Vue/Tauri 代码反向生成对应的 Auto 语言(.at)源码,验证正向生成能产出等价的 Vue 工程。
> **范围**: 新建 `.at` 源码文件(参照 015-notes 结构)+ 手动/半自动验证正向生成一致性
> **前置**: Plan 042 已完成(ash-server + 前端完整),auto-lang 编译稳定
> **参照**: `auto-lang/examples/ui/015-notes/`(.at 源码 → gen/vue 的完整范例)

---

## 0. 背景:为什么要做反向生成

### 目标

ash-gui-vue 当前是**手写**的 Vue3 + shadcn-vue + Tauri 工程(Plan 039-042)。AutoUI 的
正规模型是:写 `.at` 源码 → auto-lang codegen 生成 Vue/Rust 代码。本计划的目标是:

1. **从手写 Vue 反推 `.at` 源码**——让 ash-gui 成为 AutoUI 管线的"一等公民"(源码驱动)
2. **验证正向生成一致性**——`.at` → codegen → 生成的 Vue 工程,应与手写版行为/UI 一致
3. **建立映射规则**——Vue 组件 → Auto widget、composable → Auto store、API → `#[api]`

### 为什么不直接用 auto-lang 从零写

因为 auto-lang 的 Vue 生成器还在演进中(12,742 行 `vue.rs`),对 ash-gui 这种**非典型
CRUD 应用**(交互式 Shell,流式输出,动态渲染器分发)的支持尚未验证。反向生成是务实的
验证路径:先有能跑的手写版作为"正确答案",再反推源码,最后正向生成对比。

### 关键挑战(来自调研)

| 层 | 难度 | 原因 |
|---|---|---|
| **API 层**(`#[api]` + types) | ★☆☆ 易 | TS interface → `pub type`,fetch fn → `#[api]` fn,映射基本 1:1 |
| **Store 层**(useShell composable) | ★★★ 难 | Vue composable 是实例化的(per-call),Auto store 是单例的(module-level);传输选择(HTTP vs Tauri)无 Auto 对应 |
| **Widget 层**(Vue SFC → widget) | ★★☆ 中 | template → view tree 需要反向识别 flex class → col/row;emit/props 命名差异(lowercase vs PascalCase) |
| **流式/外部交互**(listen/invoke/SSE) | ★★★ 难 | Tauri `listen()`、`EventSource`、discriminated-union renderer 分发——这些是命令式的,Auto 的声明式 `~Stream<T>` 模型不直接覆盖 |

---

## 1. AutoUI 正向管线回顾(来自调研)

```
.at 源码
├── src/front/*.at     widget/store/view 定义
├── src/back/api.at    #[api] 端点定义 + pub type
└── src/back/db.at     业务逻辑
        │
        ▼ auto-lang codegen
├── gen/front/vue/     Vue SFC + stores + api.ts
├── gen/back/axum/     axum routes (HTTP server)
└── gen/back/tauri/    #[tauri::command]
```

**关键 IR 类型**:
- `AuraWidget` — `{ name, state_vars, computed, messages, view_tree, handlers, props }`
- `AuraStore` — `{ name, state_vars, messages, handlers, computed }`
- `AuraNode`(view tree)— `Element{tag, children} | Conditional | ForLoop | Component | Text`
- `ApiModule` — `{ endpoints: Vec<ApiEndpoint>, types: Vec<ApiType> }`

**命名约定**:
- Widget `EditorPanel` → `components/EditorPanel.vue`
- Store `NotesStore` → `stores/useNotesStoreStore.ts` + `useNotesStoreStore()`
- API fn `list_notes` → 保持 snake_case
- Message `TogglePin` → emit `TogglePin`,template `@click="TogglePin"`
- Callback prop `on_delete: msg` → `on_delete: () => void`

---

## 2. ash-gui-vue 手写结构(反向生成目标)

### 组件树
```
App.vue
├── ToolSidebar.vue        props: commands, smartCommands
│                          emits: pick(name), runSmart(name)
├── <header> inline        cwd + git label
├── BlockList.vue          props: blocks, home
│   └── BlockItem.vue      props: block, home; emits: openPath, rerun, stop
│       └── BlockBody.vue  props: output → dispatch to renderer
│           ├── TableView.vue / RecordView.vue / CodeView.vue
│           └── TextView.vue / ErrorView.vue
└── PromptBar.vue          props: cwd, home, commandNames, history, complete
    └── HistorySearch.vue  props: history, open; emits: run, close
```

### 数据源(composable)
- `useShell.ts` — 传输选择器(Tauri vs HTTP)
- `useShellTauri.ts` — Tauri `invoke`/`listen`
- `useShellHttp.ts` — `fetch`/`EventSource`

### 类型(`types/shell.ts`)
- `RenderedOutput`(discriminated union: Table/Record/Text/Code/Empty/Error)
- `Block`, `CompletionItem`, `PromptContext`, `CommandResultPayload`, etc.

---

## 3. 反向映射规则(逐层)

### 3.1 API 层(`back/api.at`)

**规则**:每个 TS interface → `pub type`;每个 fetch/invoke fn → `#[api]` fn。

| 手写 Vue | Auto .at |
|---|---|
| `interface BootSnapshot { cwd: string; ... }` | `pub type BootSnapshot = { cwd: str, ... }` |
| `fetch('/api/command_list')` | `#[api(method="GET", path="/api/command_list")] pub fn command_list() BootSnapshot` |
| `fetch('/api/complete', {body:{line,cursor}})` | `#[api(method="POST", path="/api/complete")] pub fn complete(line str, cursor int) []CompletionItem` |
| `fetch('/api/run_command', {body:{block_id,cmd}})` | `#[api(method="POST", path="/api/run_command")] pub fn run_command(block_id int, cmd str)` |
| SSE `/api/stream` | `#[api(method="GET", path="/api/stream")] pub fn stream() ~Stream<ShellEvent>` |
| `EventSource` onmessage | `~Stream<T>` 的声明式消费 |

**类型映射**(双向):
```
number → int / float    string → str    boolean → bool
T[]     → []T           T | null → ?T   void → (无返回)
```

### 3.2 Store 层(`front/shell_store.at`)

**规则**:`useShell` 的 reactive state → `model`;`invoke`/`fetch` 调用 → store 的 `on` action
里的 API fn 调用;`listen`/`EventSource` → `~Stream` 消费。

```auto
store ShellStore {
    use back.api: command_list, complete, run_command, cancel, ...
    use types: Block, CompletionItem, PromptContext, ...

    model {
        var blocks List<Block> = List<Block>.new([])
        var cwd str = ""
        var home str = ""
        var commands List<ToolEntry> = List<ToolEntry>.new([])
        var smart_commands List<SmartCommandEntry> = List<SmartCommandEntry>.new([])
        var git_info PromptContext = PromptContext{}
    }

    computed {
        history => // blocks + persisted history
        command_names => // commands.map(c => c.name).sort()
    }

    msg Msg {
        Init,
        RunCommand(str),        // block_id 由前端生成
        RunResult(CommandResult),
        RunOutput(CommandOutput),
        Cancel,
        RunSmart(int, str, []str),
        Complete(str, int),
        RefreshGit,
    }

    on {
        .Init -> {
            let snap = command_list()
            store.cwd = snap.cwd
            store.home = snap.home
            store.commands = snap.commands
            // ...
        }
        .RunCommand(cmd) -> {
            run_command(store.next_id, cmd)
            // 结果通过 .RunResult 回来
        }
        .RunResult(result) -> {
            store.cwd = result.cwd
            // 更新 blocks
        }
        .RunOutput(chunk) -> {
            // 追加到 Running block 的 streamedText
        }
        .Cancel -> { cancel() }
    }
}
```

**难点**:传输选择(Tauri vs HTTP)。Auto store 是单例的,transport 由 codegen target
决定(axum → HTTP,Tauri → IPC)。反向生成时,transport 选择逻辑应该**不在 .at 里**,
而是由 build target 决定——这正是 015-notes 的模式(同一份 api.at,生成 axum 或 tauri)。

### 3.3 Widget 层(`front/*.at`)

**规则**:每个 `.vue` SFC → 一个 `widget` 定义。

| Vue SFC | Auto widget |
|---|---|
| `<script setup>` refs | `model { var name Type = init }` |
| `defineProps<{...}>` | `widget Name(prop: Type, ...)` 参数 |
| `defineEmits` | `msg Msg { VariantA, VariantB(str) }` |
| `<template>` flex div | `view { col { row { ... } } }` |
| `v-if/v-else` | `if cond { ... } else { ... }` |
| `v-for` | `for item in list { ... }` |
| `@click="Fn"` | `onclick: .Fn` |
| `<Child :prop="v" @ev="H" />` | `Child(prop: v, on_ev: .H)` |
| handler fn | `on { .Fn -> { ... } }` |
| `onMounted` | `.Init` 消息 |

**命名映射**(关键):
| Vue(lowercase) | Auto(PascalCase) |
|---|---|
| `@run` | `on_run: .Run` |
| `@open-path` | `on_open_path: .OpenPath` |
| `@pick` | `on_pick: .Pick` |
| `@rerun` | `on_rerun: .Rerun` |
| `@stop` | `on_stop: .Stop` |
| `@clear` | `on_clear: .Clear` |
| `@exit` | `on_exit: .Exit` |

**特殊组件**(无 Auto 等价,保留为 ext component):
- shadcn `ui/` primitives(Button/Card/Badge/...)——Auto shadcn 模式自动映射
- `HistorySearch.vue`——可表达为独立 widget
- `BlockBody.vue` 的 renderer 分发——可表达为 `view fn` 或 `if output is Table {...}`

### 3.4 渲染器(BlockBody 的 discriminated union dispatch)

当前 `BlockBody.vue` 用 `if ('Table' in o)` 分发到不同 renderer。Auto 里可以:

```auto
view fn render_output(output RenderedOutput) {
    if output is Table { t } {
        render_table(t)
    } else if output is Record { r } {
        render_record(r)
    } else if output is Code { c } {
        render_code(c)
    } else if output is Text { s } {
        text(s)
    }
}
```

这要求 Auto 的 view tree 支持 **discriminated union 的 pattern matching**——需确认
生成器是否支持(015-notes 没有 union 类型)。

---

## 4. 里程碑

### M1: API 层反向生成(`back/api.at` + `types.at`)

手写 `back/api.at`——把 ash-server 的 8 个 HTTP 端点 + SSE 流定义为 `#[api]` 函数,
把 `types/shell.ts` 的 interface 定义为 `pub type`。

**验收**:auto-lang codegen 从 `api.at` 生成的 `api.ts` 与手写 `useShellHttp.ts` 的
fetch 调用一致(URL/method/body 结构相同)。

### M2: Store 层反向生成(`front/shell_store.at`)

手写 `shell_store.at`——把 `useShellTauri.ts` + `useShellHttp.ts` 的 reactive state、
API 调用、事件处理表达为 Auto store。transport 选择不在 .at 里(由 target 决定)。

**验收**:auto-lang codegen 从 `shell_store.at` 生成的 `stores/useShellStoreStore.ts`
包含:blocks/cwd/commands 等 state、runCommand/cancel/complete 等 action。

### M3: 核心 Widget 反向生成(App + PromptBar + BlockList)

手写 `app.at`、`prompt_bar.at`、`block_list.at`——把三个核心 Vue SFC 表达为 Auto widget。
重点:template → view tree 的反向映射(flex class → col/row、v-if → if、v-for → for)。

**验收**:auto-lang codegen 生成的 `App.vue`/`PromptBar.vue`/`BlockList.vue` 的 template
结构与手写版等价(布局、事件绑定、props 传递一致)。

### M4: 辅助 Widget 反向生成(BlockItem + BlockBody + ToolSidebar + HistorySearch)

手写剩余 `.at` 文件。BlockBody 的 renderer 分发用 `view fn` 或 `if` 表达。

**验收**:全部 widget 生成的 Vue SFC 与手写版等价。

**状态**:✅ `.at` 源码完成(2026-08-05 提交 `545959b`)。M4 收尾(2026-08-05):
- `block_body.at` 的位置参数 view fn 调用改为命名参数(与 015-notes 惯例一致);
- `renderers.at` 删除,4 个 view fn 移入 `block_body.at` 同文件定义;
- view fn 改名为 **PascalCase**(`RenderTable`/`RenderCode`/`RenderText`/`RenderError`)
  ——view fn 内联仅对 PascalCase 标签触发(extract.rs `is_pascal` 检查);
- 参数引用用裸标识符(`output.columns` 而非 `.output.columns`);
- 验证:4 个渲染器在生成的 BlockBody.vue 中**全部正确内联展开**(见 DEBTS)。

### M5: 正向生成 + 对比验证

用 auto-lang 从全部 `.at` 源码正向生成 Vue 工程,与手写版逐文件对比。

**验收**:生成的 Vue 工程 `vue-tsc --noEmit` 通过;`npm run dev` 浏览器版能连 ash-server,
`ls`/`cat`/`show`/补全/历史/ghost text 功能与手写版一致。

**状态**:🚧 阻塞于 auto-lang parser/codegen 限制(2026-08-05 实测,详见 DEBTS.md)。

**当前可编译文件**(使用含 fix043 修复的 debug 二进制,2026-08-05 15:51 构建):
- ✅ `back/api.at`(`#[api]` 已不 stack overflow,015-notes 同款)
- ✅ `front/app.at`、`front/block_list.at`、`front/prompt_bar.at`、
  `front/tool_sidebar.at`、`front/history_search.at`、`front/block_body.at`
- ⚠️ `front/types.at`(纯类型文件,"No widget or store declarations" 警告,预期)
- ❌ `front/shell_store.at`(msg 多参数 `Complete(str,int)`/`RunSmart(int,str,[]str)` +
  computed 多行 body)
- ❌ `front/block_item.at`(view if 条件里 `None` 比较)

**注意**:`#[api]`/`store` stack overflow 修复已合入 auto-lang master
(`d896d263`,dot_item 方案)。M5 验证用 master 最新构建的二进制即可。
`renderers.at` 已删除(M4 收尾),view fn 内联问题已解决(见 M4 状态)。

---

## 5. 风险与策略

| 风险 | 策略 |
|---|---|
| Auto 生成器不支持某些 Vue 模式(discriminated union renderer、callback prop、SSE) | 先做支持的;不支持的标记为 `ext_component`(手写逃逸),记录差距 |
| Store 的 transport 选择无法表达 | 不在 .at 里表达——transport 是 build target 的事(axum vs tauri codegen 各自处理) |
| 命名差异导致生成不匹配 | 在 .at 源码中统一用 PascalCase message + snake_case API fn(与生成器约定一致) |
| auto-lang 生成器在演进中(Plan 364/380) | pin 到当前稳定 commit;记录生成器版本 |
| 流式输出(`~Stream<T>`)的生成器支持未验证 | M2 先验证 `~Stream` 是否正确生成 SSE/EventSource;如不支持,降级为 ext_component |

### 分层策略(渐进)

```
M1(API 层) → 最干净,1:1 映射,先验证 codegen 能正确生成 api.ts
     ↓
M2(Store 层) → 验证 store codegen + ~Stream 支持
     ↓
M3(核心 Widget) → 验证 template → view tree 反向映射
     ↓
M4(辅助 Widget) → 补全剩余组件
     ↓
M5(对比验证) → 正向生成,逐文件 diff
```

如果某一层遇到生成器不支持的模式,标记为 ext_component 并记录差距,不阻塞后续层。

---

## 5.5 M5 收尾 Phase:cat-3 action 互调链式合并修复(parser)

> **状态**:**已完成**(auto-lang commit `654ba12e`,已合 master)。实施时在原方案
> (双 guard)基础上发现需要 **3 个 guard + store composable 重构**,详见 DEBTS。
> **前置**:M5 的 store-codegen 架构(a96d4da2)与质量修复(cat-1/2/4 = 31c4b84d)已合 master。
> **目标**:消除 `useShellStoreStore.ts` 最后 1 个 vue-tsc 错误(8→0)。

### 问题

store handler 里 action 互调(如 `.Init` 末尾的 `.RefreshGit()`)被 parser 错误地
**链式合并**到上一条语句,生成无效 JS:
- `.cwd = result.cwd` + `.RefreshGit()` → `result.cwd.RefreshGit()`(对字符串属性调用)
- `.x = history()` + `.RefreshGit()` → `history().RefreshGit()`(对 Promise 属性调用)

### 根因(已用 AST probe 验证,**纠正了上一轮的错误判断**)

**不是** "dot_item 的 RHS `parse_expr` 跨行消费"(上一轮的错误结论;模式 B 证明 RHS
不跨行消费)。**真正根因是 `parse_body` 的 body-chaining 逻辑(parser.rs:6020)有两个 bug**:

body-chaining 的设计意图:`let x = A.new().b().c()`(方法链跨行)。它把以 `.` 开头的
后续语句(self-dot-call)合并到前一个表达式。但它**无法区分**:
- `.Method()` = 对上一行结果的**方法链**(合法,模式 D)
- `.Method()` = 对 self 的 **action 调用**(应独立,模式 A/C/E)
- `.field = expr` = 点前缀**赋值**(应独立,不是链式接收者)

两个 bug(用 5 模式 AST probe 验证):

**Bug 1 — target 搜索(parser.rs:6024-6037)**:链式目标候选包含点前缀赋值
(`Stmt::Expr(Bina)`,`is_dot_self_call` 返回 false → 被当合法 target)。导致
`.RefreshGit()` 链到 `.cwd = result.cwd`。

**Bug 2 — pop 阶段(parser.rs:6054-6062)**:`chain_count = stmts_len - target_idx - 1`
假设 target 之后全是 self-dot-call,pop 全部。中间夹杂的点前缀赋值被错误 pop + 卷入链。
单独修 Bug 1(跳过点前缀 target)会让模式 C 更糟:target 跳过中间所有点前缀,找到更早的
`let snap`,pop 把中间的点前缀赋值全卷进去 → `let snap = (.cwd = snap.cwd)`。

### 修法(已用 probe 验证:**两个 guard 必须组合**)

**Guard 1(target 搜索)**:候选筛选加门控,跳过 `stmt_starts_with_dot[i] == true` 的
语句(点前缀语句不是合法链式接收者;真正的接收者 `let x = ...` 不以 `.` 开头)。

**Guard 2(pop 阶段)**:pop 循环里,若 `stmts.last()` 不是 self-dot-call(`is_dot_self_call`
为 false),立即停止链式(遇到点前缀赋值等独立语句不再 pop)。

**两个 guard 组合的 probe 结果(5 模式)**:

| 模式 | 源 | 修前 | 修后 | 正确? |
|---|---|---|---|---|
| A | `.cwd=x` + `.RefreshGit()` | ❌ 合并 | 2 条独立 | ✅ |
| B | `let` + `.cwd` + `.home`(无 action) | 3 条 | 3 条 | ✅ |
| C | `let` + `.cwd` + `.home` + `.RefreshGit()` | 末尾合并 | `let snap=command_list().RefreshGit` + 2 条赋值 | ⚠️ 残留 |
| D | `let x=A.new()` + `.b()` + `.c()` | 链式 | 链式 | ✅ |
| E | `.x=history()` + `.RefreshGit()` | ❌ 合并 | 2 条独立 | ✅ |

**模式 C 残留**:`.RefreshGit()` 跳过中间两个点前缀赋值,找到最早的 `let snap`,
链成 `command_list().RefreshGit()`。这是 `.Method()` 固有歧义(parser 无法区分 action
调用 vs 方法链)。真实 Init handler 里 `.RefreshGit()` 后面紧跟 `for` 循环(非点前缀),
会自然打断,实际影响小于 probe 的纯点前缀序列。**接受这个残留**(从"对字符串/Promise
属性调用"降级为"对 API 结果属性调用",危害更小)。

### 实施清单

| 文件:位置 | 改动 |
|---|---|
| `auto-lang/crates/auto-lang/src/parser.rs:6024-6037`(target 搜索) | 循环开头加 `if *stmt_starts_with_dot.get(i).unwrap_or(&false) { continue; }` |
| `auto-lang/crates/auto-lang/src/parser.rs:6054-6062`(pop 阶段) | pop 前 peek `stmts.last()`,非 self-dot-call 则 break;加 `stopped_early` 标志 |
| 测试 | parser 加 `test_dot_prefix_assignment_not_chained`(5 模式 A/B/D/E 断言 + C 残留文档化) |
| 回归 | 全量 `cargo test -p auto-lang`(重点:gdscript 方法链、a2r actor handler);22 pre-existing 失败不变 |

### 验证(闭环)
- `auto build` on ash-gui-auto → `useShellStoreStore.ts` vue-tsc 错误 **1 → 0**
- (模式 C 残留若仍出现,记录为新 DEBT;真实 Init 的 `.RefreshGit()` 后跟 `for` 可能不触发)

### 风险
- **低-中**:body-chaining 是 parser 核心路径,但两个 guard 都是"收紧"(只减少合并,不新增)。
  Guard 1 跳过点前缀 target;Guard 2 提前停止 pop。合法方法链(模式 D)不受影响(接收者
  `let x` 不以 `.` 开头,pop 的都是 self-dot-call)。
- **必须跑的回归**:gdscript 测试(方法链密集)、a2r actor handler 测试、015-notes 的
  notes_store.at(有 action 互调的话)。

---

## 5.6 M5 收尾 Phase:widget props 声明 + api 类型定义(.at 源码补全)

> **状态**:**已完成**(2026-08-06)。vue-tsc 错误 **19→0**,vite build 成功。
> 历程:A 类 14 个 .at 源码补全(提交 `84c4858` 等);B 类 19 个 codegen 问题
> 在 auto-lang worktree `fix/043-m5-bclass`(commit `718e94aa`)**已合 master**
> (merge `e4fd405d`)。完整性核查另发现并修复 2 个功能性缺口(非类型错误):
> App.at `on_run` 空桩(codegen 生成 `function RunCommand(){}`,回车不执行命令)+
> computed 的 if/else-if 表达式生成 `undefined`(状态图标丢失,auto-lang master
> `92314c2d` 给 `expr_to_js` 加 `Expr::If` → IIFE)。详见 DEBTS.md。
> **剩余 M5 验收项**:运行时验证(`npm run dev` 浏览器版连 ash-server,核对
> `ls`/`cat`/`show`/补全/历史/ghost text 与手写版一致)——尚未执行,待做。

### 问题(store codegen 清零后剩余的 42 个 vue-tsc 错误)

| 类型 | 数量 | 根因 |
|---|---|---|
| TS2339(Property does not exist) | 33 | 子组件 widget 声明缺参数列表 → 无 `defineProps` |
| TS2304(Cannot find name) | 8 | `back/api.at` 用 `use types:` 导入类型但没声明 → api.ts 无 interface |
| TS2552(Cannot find name) | 1 | 同 TS2304(`ShellEvent`) |

### 根因对照(015-notes vs auto-shell)

**TS2339 — widget props**:
- 015-notes 子组件声明带参数列表 → 生成 `defineProps<{...}>()`:
  `widget NavTree(active_folder: str, active_id: int, ...) { ... }`
- auto-shell 子组件**无参数列表** → 不生成 defineProps:
  `widget BlockBody { ... }`(模板用了 `output` 但未声明为 prop)
- 这是 **M3/M4 反向生成的遗漏**(当时只写了 widget 体,没写 props 签名)。

**TS2304 — api 类型**:
- 015-notes 的 `back/api.at` 有 `pub type Note = {...}` → api.ts 生成 `export interface Note`
- auto-shell 的 `back/api.at` 用 `use types: BootSnapshot, ...` 导入,但**没声明**
  (`pub type BootSnapshot = {...}` 缺失) → api.ts 零个 interface
- 类型都在 `front/types.at`(纯前端,api codegen 不读它)

### 修复清单(auto-shell 侧 .at 源码,不涉及 auto-lang)

**A. 子组件 widget 声明补参数列表**(参照调用点传参):

| widget | 当前 | 补成(从调用点反推) |
|---|---|---|
| `BlockBody` | `widget BlockBody {` | `widget BlockBody(output: RenderedOutput, on_open_path: msg) {` |
| `BlockItem` | `widget BlockItem {` | `widget BlockItem(block: Block, home: str, on_open_path: msg, on_rerun: msg, on_stop: msg) {` |
| `BlockList` | `widget BlockList {` | `widget BlockList(blocks: List<Block>, home: str, on_open_path: msg, on_rerun: msg, on_stop: msg) {` |
| `PromptBar` | `widget PromptBar {` | `widget PromptBar(cwd: str, home: str, command_names: []str, history: []str, injected_command: str, on_run: msg, on_injected: msg, on_clear: msg, on_exit: msg) {` |
| `ToolSidebar` | `widget ToolSidebar {` | `widget ToolSidebar(commands: List<ToolEntry>, smart_commands: List<SmartCommandEntry>, on_pick: msg, on_run_smart: msg) {` |
| `HistorySearch` | `widget HistorySearch {` | `widget HistorySearch(open: bool, matches: []str, on_run: msg, on_move: msg) {`(参数从模板用法反推) |

**B. `back/api.at` 补 `pub type` 声明**:把 `front/types.at` 里被 api 函数引用的类型
(`BootSnapshot`/`CompletionItem`/`PromptContext`/`SmartResult`/`ShellEvent`/`Stream`)
复制成 `back/api.at` 的 `pub type X = {...}`(参照 015-notes 的 `pub type Note = {...}`)。

**C. handler 参数名不匹配(待确认是否 codegen bug)**:
`BlockList.at` 的 `.Rerun(cmd)` 声明参数名 `cmd`,但生成的 vue 函数签名用 `b`(来自 emit
调用点 `Rerun(b)`),body 里 `emit('Rerun', cmd)` 用的是声明的 `cmd`。生成:
```ts
function Rerun(b: any): void { emit('Rerun', cmd) }  // cmd 未定义!
```
这可能是 codegen 把 handler 参数名和 emit-wrapper 参数名搞混了。如果补 props 后仍存在,
需在 auto-lang 调查(可能涉及 worktree)。

### 验证
- `auto build` → `vue-tsc` 错误 42 → 25(props + pub type 补全后),剩 25 见下
- 对照:015-notes 的子组件 + api 类型生成的 vue-tsc 是通过的(既有先例)

### 剩余 25 错误分类(实施后实测)

> **2026-08-06 更新**:**全部解决**。A 类 14 个在 auto-shell 侧修(提交
> `84c4858` 等,补 computed + 命名统一);B 类实际 19 个在 auto-lang worktree
> `fix/043-m5-bclass`(commit `718e94aa`)修,`auto build` 后 vue-tsc **0 错误**。
> 下方保留原始分类供追溯;根因与修法详见 DEBTS.md。

**A. .at 源码问题(可继续在 auto-shell 修)**:
- 未声明的组件内部变量(7 个 TS2339):`status_glyph`/`cwd_display`(BlockItem、PromptBar)、
  `matches`(HistorySearch)、`cthis`/`sthis`(ToolSidebar)。这些是反向生成时遗漏的 computed
  或命名不一致(如 PromptBar 用 `.cwd_display` 但 App 传的是 `cwd`)。修法:补 computed 或
  统一命名。
- 类型未 import(5 个 TS2304/2552):`Block`/`CompletionItem`/`ToolEntry`/`SmartCommandEntry`
  在组件 defineProps 里引用但组件没 `use types` import。修法:确认组件 use types。
- handler 参数名不匹配(2 个 TS2304):BlockList 的 `.Rerun(cmd)` 参数名 `cmd` vs 生成的
  emit-wrapper 参数 `b`。修法待定(可能需 codegen 修)。

**B. auto-lang codegen 问题(需 worktree)**:
- `[][]T` 字段不生成 interface(2 个 TS2339):`RenderedOutput.rows`/`code_lines` 是
  `[][]RenderedCell`/`[][]CodeSpan`,parser 不报错但 codegen 的 `to_ts_type` 不生成
  这些字段(生成的 interface 里缺了它们)。
- msg 回调签名不匹配(6 个 TS2322):`on_pick: msg`(msg 带 str payload)生成的 defineProps
  类型是 `() => void`,但 App 传的是 `(name: any) => void`。codegen 对带 payload 的 msg prop
  没生成正确的函数签名。
- BootSnapshot 字段在 store 不可见(3 个 TS2339):interface 有 `commands` 字段,但
  `useShellStoreStore.ts` 里 `snap.commands` 报不存在——可能是 store composable 的类型
  上下文问题。

### 风险
- **A 类**(14 个):.at 源码补全,参照 015-notes,不涉及 auto-lang。
- **B 类**(11 个):auto-lang codegen,需 worktree 调查 `to_ts_type`/msg prop 签名/store 类型。
- **2026-08-06 更新**:B 类实际 19 个(6 子类,部分计数与最初 11 个的估计不同——
  实测 `[][]T` 与 BootSnapshot 字段同根因:lenient `parse_fields` 只认冒号字段,
  `to_ts_type` 本就支持 `[][]T`)。已全部修复。

---

## 5.7 M5 运行时验证 Phase:功能缺口补修(2026-08-06)

> **性质**:运行时验证(起 ash-server + 生成版 dev)发现 2 个功能性缺口,非类型错误。
> G2 已修复;G1 需 auto-lang codegen 增强,方案见下。

### 实测反馈修复(auto-lang worktree `fix/043-m5-runtime-bug` commit `1f11616b`,待合 master)

用户手动实测(打开生成版 dev)发现 2 个问题,均已修复:

**R1 — `ls` 输入后 block 内容区不显示结果**:根因是 struct-literal
`Block{ id: id, command: cmd, ... }` 的字段被 `parse_node_body` 当语句解析
(报 "Expected term, got RBrace" 收集后丢弃),codegen 收到**空 args** Node →
生成 `let block = {}` → `RunResult` 的 `b.id == result.block_id` 永不匹配,
结果永远回填不到 block。修复(UI scenario 专用):
- `atom()` 构造分支 + `node_or_call_expr` 的 rhs 构造:用 `object()` 把
  `{ field: value }` 解析为**命名 args**
- 独立 helper `parse_braced_struct_args`(避免热路径栈帧增大——gdscript
  dodge_player 勉强 2MB 栈,DEBTS 记录过同类回归;非 UI dialect 保持旧行为)
- 验证:生成的 RunCommand 为
  `let block = { id: id, command: cmd, cwd: cwd.value, status: { kind: 'Running', message: '' }, output: null, streamed_text: '', duration_ms: 0 }`

**R2 — 样式丑/非黑色主题**:根因是生成的 `index.html` 无 `class="dark"`
(shadcn 的 `.dark` tokens 不生效,落到浅色 `:root`),且
`regenerate_source_files` 从不重写 index.html(只在初始脚手架写一次)。
修复:
- `generate_index_html` 加 `class="dark"`
- `regenerate_source_files` 加入 index.html 重建
- 验证:index.html 为 `<html lang="en" class="dark">`,vue-tsc 0 + build 成功

**回归**:auto-lang lib 2823 passed / 22 failed = **精确 pre-existing 集合**
(17 dstr + route::discovery + ark×2 + vue button + vm if_stmt,纯净 master 已验证),
零新增失败。parser 改动同时修复了此前 master 上的 ark/vue/vm 个别失败
(用户 VM 重构合并后已消失)。

**R3 — `ls -al` 仍无结果:RenderedOutput 数据契约不匹配**(已修复,auto-lang
worktree `fix/043-m5-runtime-bug` commit `540fbcdb`,待合 master)

**现象**:R1/R2 修完后用户实测仍"没有反应"。curl `/api/stream` 确认服务端
发送 **serde externally-tagged union**(`{"Table":{columns,rows}}` /
`{"Text":"..."}`,单元格 `{"Tagged":{text,tag}}`),而生成版前端仍按**扁平
kind 字段**(`output.kind`/`output.columns`)访问 → 全部 undefined。api.at 的
`RenderedOutput` 已改为 variant-keyed 可选字段(`Table: ?TableOutput` 等),
但 block_body.at 重写后 parse 报 `"Expected term, got RBrace"`——暴露 auto-lang
两个独立缺陷:

1. **struct-literal widening 误伤 dot 表达式 RHS**(parser.rs,`in_dot_rhs`):
   R1 让"任意 PascalCase 标识符 + `{` → 结构体构造",在 `text cell.Text { }`
   中 `Text` 后紧跟元素 props 的 `{`,被当 `Text{...}` 构造吞掉括号 → desync。
   `cell.text`(小写)不受影响,所以此前一直没暴露。修复:parser 加
   `in_dot_rhs` 标志,仅 `Op::Dot` 的 RHS 抑制 widening(Asn 等不受影响,
   `x = Type{...}` 仍正常)。
2. **view fn 内联时 ForLoop iterable 未做参数替换**(extract.rs):iterable 是
   字符串,`expand_fragment_node` 原样保留 → 原版靠 widget 同名 prop 碰巧
   解析。修复:iterable 也走 `substitute_condition`,`for col in output.columns`
   → `output.Table.columns`;`expr_to_condition_str` 修 self-dot 基座
   (`.output.Table` → `output.Table`,而非 `self.output.Table`)。

**验证**:新回归测试 `test_dot_rhs_field_access_not_struct_construction`;
parser 161 + gdscript 63 + aura 46 全过;ash-gui-auto vue-tsc 0 + build 成功;
生成 BlockBody.vue 为 `v-if="output.Table != null"` + `v-for="col in
output.Table.columns"` + `cell.Tagged.text`;SSE 实测 `status:"Success"` /
`Table{columns,rows}` / `Tagged{text,tag}` 与 api.at 契约完全吻合。

### G2(已修复,auto-shell commit `2d88ae0`):PromptBar 输入交互

**现象**:生成的 PromptBar 输入框完全不可用——`<input :name="input" @input="Run">`:
- `:name="input"` 是 name 属性,不是值绑定(应 v-model)→ 输入不更新 `.input`
- `@input="Run"` 映射错(打字触发执行命令处理器)
- 无 Enter 处理、补全无人触发

**修法**(参照 013-todo 的 `value: + oninput:` → v-model 模式):
| 位置 | 改动 |
|---|---|
| `prompt_bar.at` input | `input { value: .input, oninput: .OnInput, onkeyup: .OnInput, onenter: .Run(.input) }` → 生成 `v-model="input"` + `@keyup.enter="Run(input)"` + `@keyup="OnInput"`(v-model 会吞 oninput,用 onkeyup 兜底触发补全) |
| `prompt_bar.at` `.Run(cmd)` | 改为纯转发(清状态 + 自动 `emit('Run', cmd)` → App 执行),消除之前 PromptBar+App **双执行** `store.RunCommand` |
| `prompt_bar.at` `.OnInput` | `await complete(.input, .input.len())` → 填 `.suggestions`(补全 UI 已存在);需 `use back.api: complete` |
| `app.at` `.RunCommand(cmd)` | 加空命令守卫 `if cmd.trim() != ""` |

**验证**:vue-tsc 0 + vite build 成功;生成代码 v-model/@keyup.enter/@keyup 均正确。

**遗留(非核心,记录)**:ghost text 语法高亮 overlay、↑/↓ 历史导航、Ctrl+R 搜索、
Tab 接受补全——.at 有 stub(`AcceptGhost`/`HistoryOlder`/`ToggleHistorySearch` 等)
但无 keydown 事件接线。输入→执行→补全主链路已通。

### G1(已实施并合 master,merge `209f938c` 已 push origin):SSE 流式输出未接线

**现象**:生成的 `useShellStoreStore.ts` 有 `RunOutput`/`RunResult` action
(命令输出/结果处理器),但**没有 `EventSource('/api/stream')` 订阅**——命令执行后
界面永不更新。手写版 `useShellHttp.ts` 有 `connectSSE()`。

**服务器侧已确认正常**(重建 ash-server 后):`/api/stream` 推送
`{"event":"command_result",...}`,`ls -la` → `Table{columns,rows}`、`cat` → `Text`
——与生成版 BlockBody 渲染器的数据契约完全吻合。之前 :3000 上跑的是**过期二进制**
(SSE 无输出),误导排查。

**修法(auto-lang codegen,`generate_store_composable`,已实施)**:当
`store.api_imports` 含 `stream` 且 store 有 `RunOutput`+`RunResult` action 时,注入:
```ts
// 模块级(单例守卫,多个 widget 各自 reactive(useStore()) 只连一次)
let __streamConnected = false;
// 函数体内(action 声明之后,return 之前):
if (!__streamConnected) {
  __streamConnected = true;
  const es = new EventSource('/api/stream');
  es.onmessage = (ev) => {
    try {
      const data = JSON.parse(ev.data);
      if (data.event === 'command_output') RunOutput(data);
      else if (data.event === 'command_result') RunResult(data);
    } catch { }
  };
}
```
**判据**:`stream` api 函数 + `RunOutput`/`RunResult` action 命名(Store 消费
`~Stream` 的实用模式;待 codegen 正式支持 `~Stream<T>` 后替换为类型驱动)。

**验证(已做)**:生成 store 含 EventSource 订阅;vue-tsc 0 + vite build 成功;
回归 auto-lang lib 2821 passed / 23 failed(均 pre-existing)。**浏览器实测未做**
(当前环境 IAB 不可用),命令执行后 blocks 实时更新待浏览器验证。

**备选方案(否决)**:手写逃逸(ext_component)——破坏"源码驱动"的正向生成一致性。

---

## 6. 文件清单(预期产物)

### `.at` 源码(`ash-gui/ash-gui-auto/src/`)

```
src/
├── front/
│   ├── app.at              App widget(根布局)
│   ├── prompt_bar.at       PromptBar widget(输入框 + ghost text + 补全)
│   ├── block_list.at       BlockList widget(滚动列表)
│   ├── block_item.at       BlockItem widget(单个命令块)
│   ├── block_body.at       BlockBody view fn(渲染器分发)
│   ├── table_view.at       TableView view fn
│   ├── code_view.at        CodeView view fn
│   ├── text_view.at        TextView view fn
│   ├── tool_sidebar.at     ToolSidebar widget
│   ├── history_search.at   HistorySearch widget
│   ├── shell_store.at      ShellStore store
│   └── types.at            前端类型定义
├── back/
│   └── api.at              #[api] 端点 + pub type
└── pac.at                  包定义
```

### 验证产物(`ash-gui/ash-gui-auto/gen/`)

```
gen/front/vue/             正向生成的 Vue 工程(与 ash-gui-vue 对比)
gen/back/axum/             正向生成的 axum 路由(与 ash-server 对比)
```

---

## 7. 参考文件

- `auto-lang/examples/ui/015-notes/src/`(范例 .at 源码)
- `auto-lang/examples/ui/015-notes/gen/front/vue/src/`(范例生成输出)
- `auto-lang/crates/auto-lang/src/ui_gen/vue.rs`(Vue 生成器)
- `auto-lang/crates/auto-lang/src/ui_gen/ts_adapter.rs`(Auto→TS 转译)
- `auto-lang/crates/auto-lang/src/api/targets/{typescript,axum,tauri}.rs`(API codegen)
- `auto-lang/crates/auto-lang/src/aura/types.rs`(widget/store IR)
- `ash-gui/ash-gui-vue/src/`(手写 Vue 目标)
- `ash-gui/ash-server/src/`(手写后端,API 层目标)
