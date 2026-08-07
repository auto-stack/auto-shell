# Plan: auto-ui-creator 技能

> 目标：扩展一个新的 skill —— `auto-ui-creator`，专门针对 **AutoUI**（Auto
> 语言的 UI DSL）优化代码生成。基于现有 `auto-lang-creator`（普通 Auto 代码）
> 的结构，融入 `auto-lang/examples/ui/*` 的真实用法，并以
> `ash-gui/ash-gui-auto`（本次生成的 Auto 版）对比 `ash-gui/ash-gui-vue`
> （原生 Vue 3 版）的实战差异作为「陷阱」来源。
>
> 本文件先记录全部调研结果，再给出实现计划，最后开始实施。

## 0. 输入与产物

| 输入 | 位置 | 角色 |
|---|---|---|
| 既有技能 | `D:/autostack/skills/auto-lang-creator/` | 结构模板（skill.md + references/ + tests/） |
| AutoUI 语料 | `D:/autostack/auto-lang/examples/ui/001..035` | 权威语法参考（35 个递进示例） |
| 实战 Auto 版 | `D:/autostack/auto-shell/ash-gui/ash-gui-auto/src/front/*.at` | 陷阱来源（含 codegen 注释里挖出的 gotcha） |
| 实战 Vue 版 | `D:/autostack/auto-shell/ash-gui/ash-gui-vue/src/**` | 对照基准（Vue→AutoUI 映射的事实依据） |

产物：

```
D:/autostack/skills/auto-ui-creator/
├── SKILL.md                       # 主文件：Gotcha Checklist + Vue→AutoUI 映射 + 模板
├── references/
│   ├── autoui-syntax.md           # AutoUI 完整语法参考（widget/store/view fn/msg/model/on…）
│   └── vue-to-autoui.md           # Vue 3 → AutoUI 逐特性映射（script setup/template/directives/composables）
└── tests/
    ├── README.md                  # 验证协议（对齐 auto-lang-creator/tests 的三层结构）
    ├── probes/
    │   ├── gotcha-probe.at        # 覆盖 Gotcha Checklist 每条的合成 .at（golden）
    │   └── todo-complete.at       # 一个全栈最小样例（widget+store+view fn+pac+types+api）
    └── verify.sh                  # （可选）断言 golden 含全部正确模式
```

---

## 1. 调研结论一：AutoUI 是什么（语言模型定位）

AutoUI 是 Auto 语言的 **UI 场景 DSL**，采用 **Elm Architecture**（model / msg /
view / on）：

- 项目由 `pac.at` 清单标记 `scene: "ui"`，`render: "vue"`（或 `jet`/`ark`/`rust`）。
- 入口是 `widget App { ... }`（**不是** `fn main()`，也**没有** `component`/`app`/`page` 关键字）。
- 默认编译目标是 **Vue 3 `<script setup>` + Tailwind + shadcn-vue**。
- 后端（可选）用 `#[api(method=…, path=…)]` 标注的 `pub fn`，前端通过 `use back.api: fn` 同步调用。
- 与普通 Auto 的关系：**复用**基础语法（type/enum/ext/fn/closures/is/for/var…），
  但**新增** widget/store/view/view fn/msg/model/on/computed/watch/expose/bind/slot 等结构化 DSL。

> ⚠️ 关键差异：普通 Auto 用 `fn main()` + task/actor 做并发；AutoUI **完全不用**
> `fn main()`、task、actor——UI 的并发由 codegen 驱动（SSE/Tauri 事件）。`.at` 源里
> **没有** async/await/.then。

---

## 2. 调研结论二：AutoUI 语法要素（来自 35 个示例 + ash-gui-auto）

### 2.1 顶层声明（只有这几种）

| 关键字 | 形式 | 用途 |
|---|---|---|
| `widget Name(props) { }` | 入口/子组件 | UI 组件。props 在括号里 `name: Type` |
| `store Name { }` | 共享状态 | 跨组件状态，无 `view` |
| `view fn Name(param Type) { }` | 模板片段 | 可复用渲染片段，**必须 PascalCase** |
| `type Name { f t }` / `pub type Name = { f: t }` | 数据形状 | 前端文档 vs codegen 源（见陷阱 U19） |
| `alias Name = T` | 类型别名 | 单行 |

### 2.2 widget 内部 block（约定顺序）

```
widget Name(props) {
    use { ... }          // 可选：外部 TS/Vue/npm 导入（029）
    msg Msg { ... }      // 事件枚举
    model { var x T = v }// 可选：响应式状态
    computed { k => e }  // 可选：派生
    watch { .s -> {} }   // 可选：监听
    view { ... }         // 模板树
    on { .M -> {} }      // 事件处理
    expose { .M, .x }    // 可选：defineExpose
    style { ... }        // 可选：scoped 原生 CSS（027）
    bind { "k" -> .M }   // 可选：全局键盘绑定（011）
}
```

### 2.3 状态与自引用 `.field`

- 状态：`model { var count int = 0 }`，读写都用 **`.count`**（前导点= self 引用）。
- props：括号里声明 `name: Type`，体内同样 **`.name`** 访问（`view fn` 例外，见陷阱 U8）。
- store：`use store_mod: MyStore` 后，**模板里 `.store.field`**，**handler 里 `store.Method()`**——这是**不对称**的（陷阱 U1）。

### 2.4 事件系统（与 Vue 的 `@click` 完全不同）

- DOM 事件作为元素**花括号内属性**：`onclick: .Inc`、`oninput: .Changed`、`onenter: .Add`。
- 带参：`onclick: .Toggle(item.id)`、`onkeydown: .Nav($event)`。
- 键修饰符用**点链**：`onkeydown.enter.prevent:`、`onkeydown.ctrl.r:`、`onkeydown.up:`。
- 全局目标：`onmousemove.window:`、`onwheel.document.capture.prevent:`。
- 自定义/带 `:` `-` 的事件名用**引号**：`on "update:modelValue": .X`、`on "autodown:slash-open".document: .Y`。
- **没有 `v-model`**：双向绑定 = `value: .x` + `oninput: .Handler`（陷阱 U5）。

### 2.5 模板 DSL（`view { }`）

- 布局原语：`row`（横）、`col`（竖）；**没有** `div`/`flex`/`v-` 指令。
- 内容标签：`text "lit"|.expr { }`、`button "lit"|.expr { }`、`input { }`、`textarea`、HTML 标签（`table/thead/tr/td/progress`）、shadcn 组件（`dialog`/`card`/`badge`…）。
- 子组件：`PascalChild(prop: .val, on_xxx: .Variant)`，可带花括号体传 slot。
- 控制流：`if .c { }` / `if .. else if .. else`；`for x in .coll { }` / `for i, x in .coll { }`。
- 文本插值：**三选一**——`text .expr`、`text f"${.field}"`、`` text `${.field}` ``。**没有 `{{ }}`**。

### 2.6 样式（三种，Tailwind 为主）

1. `style: "tw classes"`（或 `class:`，等价）—— Tailwind 工具类字符串。
2. `style_obj: { top: "${.x}px", "z-index": 50 }`—— 真·内联 `:style`（**注意 hyphen key 要引号**）。
3. `style { .sel { ... } }` block（widget 级 scoped 原生 CSS）+ `pac.at styles: []`（项目级 CSS）。

### 2.7 computed / watch / expose / slot / v-model / ref / dyn

- `computed { name => expr }`（箭头）；**陷阱**：handler/computed 里读 computed 需要 `.x.value`（陷阱 U14）。
- `watch { .src -> { } }`，支持 `.immediate`/`.deep` 和逗号多源。
- `expose { .M, .field }`——`defineExpose`；**陷阱**：模板未引用的 msg handler 会被 codegen 丢弃，必须在 `expose` 列出（陷阱 U6）。
- `slot`（默认出口）/ `slot(name: "x")`（命名出口）；父侧 `slot(name: "x") { ... }`。
- v-model：builtin 用 `open: .state`（dialog）；自定义组件声明 `modelValue` prop + `"update:modelValue"(str)` 引号变体（陷阱 U16）。
- 模板 ref：`ref: "elName"`，handler 里 `.elName.getBoundingClientRect()`。
- 动态组件：`dyn (.icon) { size: 16 }` → `<component :is>`。

### 2.8 后端 / 类型 / 全栈

- `pac.at`：`scene: "ui"`, `render: "vue"`, `api: "rust"`, `styles: [...]`, `npm_deps: [...]`。
- `pub type X = { f: t }`（**等号 + 冒号**，codegen 源）vs `type X { f t }`（前端文档）。
- `#[api(method="GET", path="/api/x")] pub fn x() ?T { ... }`。
- 流式：`pub fn stream() ~Stream<E>` —— `~Stream<T>` 触发 SSE codegen（陷阱 U18）。

---

## 3. 调研结论三：Gotcha 清单（AutoUI 专属，从 ash-gui-auto 注释 + 示例挖出）

这是本技能的**核心价值**——普通 `auto-lang-creator` 完全不覆盖这些。每条都标注来源。

| # | 陷阱（AI 易错） | 正确写法 | 来源 |
|---|---|---|---|
| U1 | handler 里写 `.store.Init()` | handler 里**`store.Init()`**（裸）；模板里才 `.store.field` | app.at:87, shell_store.at |
| U2 | `for x in .coll` 循环体里用裸 `x` | 体里用 **`.x`**（header 裸、body 带 dot）；`for i, x` 同理 | block_list.at:34, block_body.at:40 |
| U3 | `view fn F(p T) { ... .p ... }` | view fn 参数在体内是**裸标识符** `p.field`（不是 `.p`） | block_body.at:18-19 注释 |
| U4 | `view fn renderTable(...)`（小写开头） | **必须 PascalCase**，否则 codegen 不当内联标签 | block_body.at:11-12 注释 |
| U5 | `v-model="x"` / `:value` + `@input` Vue 写法 | AutoUI：`value: .x` + `oninput: .Handler`；**无 v-model**；keyup 兜底常见 | prompt_bar.at:91-97 |
| U6 | handler 里用了 `.Exit` 但模板没引用 → 运行时报函数不存在 | 在 `expose { .Exit }` 列出 | prompt_bar.at:43-47 注释 |
| U7 | `style: "lit " + (if c {..} else {..})` | **会渲染成 null**；用 `if/else` 作整值，或只 `"lit " + .var` | history_search.at:43-44 注释 |
| U8 | computed 写复杂表达式（多层 .field） | computed 用 parse_expr，过深会**栈溢出**；保持简单，多分支 if/else 字面量 OK | shell_store.at:42-43 注释 |
| U9 | `computed { x => .store.filtered.length == 0 }` 用 `??` | computed 不支持 `??` 和有类型 `||`；改写 | slash_menu.at 注释 |
| U10 | 写 `v-if`/`v-for`/`v-show`/`@click`/`v-on`/`:class` | AutoUI 用 `if`/`for`/`onclick:`/`style:`，**无 v-* 指令** | 全部示例 |
| U11 | 键盘事件写 `@keydown.ctrl.r` | AutoUI 写 `onkeydown.ctrl.r: .Variant`（点链，**非** @） | prompt_bar.at:96-103 |
| U12 | 自定义事件名 `onPoke` / `update:modelValue` 不加引号 | 含 `:`/`-` 的事件**必须引号**：`on "update:modelValue":` | 034-vmodel/field.at, 030-custom |
| U13 | `expose` 里写函数定义 | expose 只列 `.Msg`/`.field`/`.refName`（引用，非定义） | 032-expose |
| U14 | handler/computed 里读 computed 用 `.x` | 需 `.x.value`（模板里才自动 unwrap） | slash_menu.at 注释 |
| U15 | nil 检查混用 `!= None` / `!= nil` | 默认值/赋值用 `None`；检查倾向 `!= nil`（源码两种都有，codegen 都吃） | block_body.at:145 vs block_item.at:76 |
| U16 | 自定义 v-model 用 `modelValue` 但不声明 emit | 子组件声明 `msg Msg { "update:modelValue"(str) }` + 空 handler `."update:modelValue"(v) -> {}`；父侧 `on "update:modelValue": .X` | 034-vmodel |
| U17 | slot 用 `<slot />` / `slot="x"` | AutoUI：子 `slot` / `slot(name: "x")`；父 `slot(name: "x") { ... }` | 033-slots |
| U18 | SSE/流式：前端写 async/fetch/EventSource | `.at` 里**不写**；后端 `pub fn stream() ~Stream<E>` 触发 codegen 注入 | ash back/api.at:229-232 |
| U19 | `type X { f t }`（前端文档式）当 codegen 源 | codegen 读 **`pub type X = { f: t }`**（等号+冒号）；前端用前者做文档 | back/api.at:20 vs front/types.at |
| U20 | 联合类型 `status: str \| object` | `.at` 表达不了 string\|object 联合；用 `status: str` 擦除 + 运行时分派 | back/api.at:110-121 注释 |
| U21 | `expose` 写驼峰 vs 父监听驼峰 | 事件名用 kebab/引号；msg 变体名 PascalCase。父 `on_xxx:` prop 对应子 msg 变体 | ToolSidebar vs App（run-smart） |
| U22 | `input` 双向绑定丢 oninput | 加 `onkeyup:` 同 handler 兜底（v-model 会吞 oninput） | prompt_bar.at:91-92 |
| U23 | `style:`（类）和 `style_obj:`（内联）混用 | **类用 `style:`/`class:`；真内联样式用 `style_obj:`**；对象式动态类 `style: { active: cond }` | 030-custom README |
| U24 | for 循环 `:key` 无法控 | `for x in .coll { Child(key: x.id, ...) }` 显式 `key:` 覆盖自动 key | 035-vfor-key |
| U25 | 没有生命周期 hook（onMount） | 用 `Init` msg 惯例：父 `.Init -> { store.Init() }`，store `.Init` 启动加载；`onMounted` 由 `.window`/`.document` 监听自动生成 | 013/015, 026/030 README |

---

## 4. 调研结论四：Vue 3 → AutoUI 映射表（来自 ash-gui-vue 对照）

这是技能的**第二核心**——AI 最常见任务是「把 Vue 组件翻译成 AutoUI」。

### 4.1 结构映射

| Vue 3 | AutoUI | 备注 |
|---|---|---|
| `<script setup lang="ts">` 整个 SFC | `widget Name(props) { msg/model/view/on }` | 块化重组 |
| `defineProps<{...}>()` | `widget Name(name: Type, ...)` 头部括号 | 回调用 `msg` 类型 |
| `defineEmits` | `msg Msg { Variant }` + `on_xxx: msg` prop + 空 handler relay | 无独立 emits 块 |
| `ref(x)` | `model { var x T = v }`，访问 `.x` | |
| `reactive([])` | `model { var x []T = [] }` 或 `List<T>.new([])` | 两种数组写法并存 |
| `computed(() => …)` | `computed { k => expr }` | handler 里 `.k.value` |
| `watch(src, cb)` | `watch { .src -> { } }`；`.immediate`/`.deep` | |
| `onMounted`/`onUnmounted` | `Init` msg 惯例；`.window`/`.document` 监听自动生成生命周期 | 无显式 hook |
| `defineExpose` | `expose { .M, .field }` | |

### 4.2 模板映射

| Vue | AutoUI |
|---|---|
| `<div class="flex">` | `row { style: "..." }` / `col { style: "..." }` |
| `v-if`/`v-else-if`/`v-else` | `if .c { } else if .c { } else { }` |
| `v-for="x in list"` | `for x in .list { }`（体里 `.x`） |
| `v-for="(x, i) in list" :key` | `for i, x in .list { }`；`key:` prop 覆盖 |
| `@click="fn"` | `onclick: .Variant` 或 `onclick: .Variant(arg)` |
| `@keydown.ctrl.r.prevent` | `onkeydown.ctrl.r.prevent: .Variant` |
| `v-model="x"`（原生 input） | `value: .x` + `oninput: .Changed`（+ onkeyup 兜底） |
| `v-model`（自定义组件） | `modelValue` prop + `"update:modelValue"(str)` msg；父 `on "update:modelValue":` |
| `:class="{a: cond}"` | `style: { active: cond }` 或 `style: if c {"a"} else {"b"}` |
| `:style="{top: x}"` | `style_obj: { top: ... }`（hyphen key 引号） |
| `{{ expr }}` / `{{ f'..' }}` | `text .expr` / `text f".."` / `` text `..` `` |
| `<slot/>`/`<slot name="x"/>` | `slot` / `slot(name: "x")` |
| `<component :is>` | `dyn (.expr) { }` |
| `<Child @open="emit('open',$e)">` | `Child(on_open_path: .OpenPath)` + relay handler |

### 4.3 组合式/状态映射

| Vue | AutoUI |
|---|---|
| `composable`（useShell，全应用单例） | `store ShellStore { model/msg/on/computed }`，`use` 导入 |
| `provide/inject` | 暂无直接对应；用 store 或 prop 透传 |
| `EventSource` SSE | 不写；后端 `~Stream<T>` 驱动 codegen |
| Tauri `invoke`/`listen` | 不写；codegen 按 `pac.at` `render`/`api` 选 transport |
| `navigator.clipboard` | 直接 `navigator.clipboard.writeText(...)`（ts_adapter passthrough） |
| `document.querySelector` / `window.innerWidth` | 通过 `ref: "el"` 模板 ref + `.el.x`；`document`/`window` 透传 |
| `nextTick(() => el.scrollTop = …)` | 用 `watch` + 模板 ref |

### 4.4 样式映射

| Vue | AutoUI |
|---|---|
| `<style scoped>` | `style { .sel { } }` block |
| Tailwind 内联 class | `style: "tw …"` 或 `class:`（等价） |
| `:class="[base, cond ? a : b]"` | `style: "base " + .cls`（.var）或整值 `if/else`（**勿** `+ (if..)`） |
| shadcn-vue `<Card>` 等 | 直接 `card { }`/`badge { }`（小写标签） |
| `:style="{color: rgb(...)}"`（CodeView RGB） | `style_obj: { color: ... }` |

---

## 5. 实现计划

### 阶段 A：骨架（必做）

1. **创建目录** `D:/autostack/skills/auto-ui-creator/{references,tests/probes}`。
2. **写 `SKILL.md`**（目标 < 500 行，但允许略超；渐进披露到 references）：
   - frontmatter：`name: auto-ui-creator`，`description`（pushy，覆盖 Vue→AutoUI、写 .at UI、审查 AutoUI 代码、Tauri/Web 前端生成）。
   - `## When to use`（4 触发场景）。
   - `## AutoUI in 30 seconds`（Elm Architecture 一图流）。
   - `## Gotcha Checklist`（U1–U25，每条 WRONG vs CORRECT，标注来源）。
   - `## Vue 3 → AutoUI Quick Mapping`（浓缩表，完整版进 references）。
   - `## Code Templates`（widget / store / view fn / 全栈 todo 最小样例）。
   - `## Project layout`（pac.at + 目录结构）。
   - 指向 `references/autoui-syntax.md` 与 `references/vue-to-autoui.md`。
3. **写 `references/autoui-syntax.md`**（详细语法，按 2.1–2.8 展开 + 完整代码块）。
4. **写 `references/vue-to-autoui.md`**（按 §4 全表 + 真实 ash-gui 对照片段）。

### 阶段 B：验证（对齐 auto-lang-creator/tests 的理念）

5. **写 `tests/probes/gotcha-probe.at`**：合成 .at，每个 Gotcha 一段，带注释，覆盖 U1–U25 可静态表达的部分。
6. **写 `tests/probes/todo-complete.at`**：全栈最小样例（App widget + TodoStore + TodoList widget + view fn + types.at + pac.at 片段 + api.at 片段），作为「golden」。
7. **写 `tests/README.md`**：说明验证协议（对齐 auto-lang-creator 的三层思路，但因 AutoUI 无 a2r 等价物，以「模式断言 + auto ui build」为主）+ 一个 `verify.sh` 占位（grep 断言 golden 含全部正确模式）。
8. **写 `tests/verify.sh`**：bash，grep 断言 golden 含 U1–U25 的正确模式 + 排除 WRONG 模式。

### 阶段 C：收尾

9. **复审**：交叉比对 ash-gui-auto 实际 .at 与本技能规则一致性；修正措辞。
10. **不**改 README（`skills/README.cn.md` 是上层介绍，可选追加，先不做以免越权）。

### 非目标（明确不做）

- 不做 a2r 式编译验证管线（AutoUI 的 codegen 在 auto-lang 项目内，本技能只做静态 golden）。
- 不覆盖 Jetpack/ArkTS/GPUI 后端（只覆盖 vue，占绝大多数；其他后端在 references 提一句）。
- 不改既有 `auto-lang-creator`（保持普通 Auto 与 UI 两个独立 skill，按场景触发）。

---

## 6. 触发与定位（避免与 auto-lang-creator 冲突）

两个 skill 并存，按**场景**触发：

- `auto-lang-creator`：普通 Auto 代码（fn main / type / ext / spec / task / a2r 移植 / book 章节）。
- `auto-ui-creator`：**UI 场景**——`pac.at` `scene:"ui"`、widget/store/view fn、Vue/前端/Tauri 翻译到 AutoUI。

`description` 写明边界：UI/前端/widget/Vue→AutoUI 触发本 skill；纯语言/后端/a2r 移植仍归 auto-lang-creator。

---

## 7. 进度跟踪

- [x] 调研（auto-lang-creator + ui 示例 + ash-gui 双版本）
- [x] 写本计划文件
- [x] 创建目录结构
- [x] 写 SKILL.md（820 行：25 条 Gotcha + Vue→AutoUI 速查 + 3 个核心模板）
- [x] 写 references/autoui-syntax.md（928 行：完整语法 + 20 节）
- [x] 写 references/vue-to-autoui.md（1169 行：逐特性映射 + PromptBar 实例）
- [x] 写 tests/（README + gotcha-probe.at + todo-complete.at + verify.sh ✅ 43/43）
- [x] 复审一致性（subagent 评审 → 修正 E1 pub fn 冒号、U2/U3 过度断言、
      补 N5 on_xxx() 直调 emit、G3 tuple `.0`/`.1`、G1 center 标签、
      U2.5 三种声明形式的冒号规则）

## 8. 复审后修正记录（2026-08-07）

评审 subagent 对照 ash-gui-auto 与 auto-lang/examples/ui 逐条核查，发现并已修：

| 问题 | 严重度 | 处置 |
|---|---|---|
| E1 `pub fn` 参数写成 `text: str`（应为 `text str`，无冒号） | 高 | 改 SKILL.md / autoui-syntax.md / todo-complete.at 共 3 处；新增 U2.5 节明确三种声明形式的冒号规则 |
| N1/N2 U2「循环体必须 .x」过度断言 | 中 | 改为「state/prop/computed 必须 .field；循环变量与闭包参数裸/.x 都可」，附源码证据 |
| N5 缺 `on_xxx()` 直调 emit 替代模式 | 中 | U21 补「直调 callback prop」写法 + 015-notes 出处 |
| G3 tuple `.0`/`.1` 访问未给示例 | 低 | autoui-syntax.md §16 补 block_body.at 实例 |
| G1 `center` 布局标签漏列 | 低 | autoui-syntax.md 标签表补行 |
| 其余（N3/N4/E2/G2/G4/G5） | 极低 | 评估后判定不改（次要或与现有规则不冲突） |

`verify.sh` 修正后仍 43/43 通过。
