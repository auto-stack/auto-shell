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

> **状态**:已调研 + 已验证修法,**待执行**(等近期 worktree 合并清理后开新 worktree 实施)。
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
