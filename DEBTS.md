# DEBTS — 已知局限与权衡记录

> 这里记录的是**已经做出的、有意识接受的**技术局限与权衡——不是待办,而是
> "我们知道这里不完美,且有明确理由暂时不修"。每条带足够上下文,让未来的维护者
> 不必重新推导一遍。若要推翻某条决定,先读这里的理由。
>
> 与 [`TODO.md`](TODO.md) 的区别:TODO 是"以后可能做"的方向;DEBTS 是"现在故意
> 不做,因为代价/收益不划算"的记录。

---

## ash-gui-vue 后端(`ash-gui/ash-gui-vue/src-tauri/`)

### `shell.execute()` 内阻塞的命令无法取消

**来源**:Plan 040 M5(命令取消)。详见 `docs/plans/old/040-ash-gui-vue-backend-gaps.md` §4.1。

**现状**:M5 的取消仅在**流式路径**(简单外部命令)真正 kill 子进程——`ExternalStream`
捕获子进程 PID,`drain_stream` 检测到取消时调用 `kill_from_handle`(Unix `kill -9` /
Windows `taskkill /T /F`)终止进程。但走 `shell.execute()` 阻塞路径的命令
(registered 命令 / builtin / Auto 函数 / 管道)无法中断,会自然跑完。

**为什么接受**:
1. **参照的 iced 版前端(`ash-gui-bin`)根本没有取消能力**——无 cancel/abort/stop/
   streaming/kill 任何相关代码。本 GUI 的流式取消已是领先能力,非缺口。
2. **真正会卡住的 registered 命令极少**:会阻塞的就 `http_get`/`http_post`/…
   (curl 网络请求,有 `--timeout` 参数)和 `sleep`(`std::thread::sleep`,无超时,
   罕见用法)。用户日常想取消的长命令(`find /`、`cargo build`、`ping`、长构建
   脚本)全是外部命令,已被流式路径覆盖。
3. **修复代价极高**:需把整个执行架构从同步改成协作式取消——给 `Command::run_atom`
   trait 加 cancel-token 轮询、几百个命令实现逐个插检查点、AutoLang VM 解释器循环
   加中断点、改 `execute_inner` 的展开/管道/链式全部路径。跨 3 个 crate、动 trait
   签名、影响 698 个测试,只覆盖边缘场景。
4. **业界先例一致**:fish / nushell 同样无法中断纯进程内计算(in-process
   computation),只能 kill 外部进程。

**务实替代补丁**(若未来有用户报告 `sleep`/`http_*` 卡住,优先做这些,而非重构):
- 给 `sleep` 命令加上限(如最大 1 小时);
- 给 `http_*` 命令设默认超时(curl 无 `--max-time` 时默认 30s)。

**推翻条件**:出现高频的、用户反复想取消的非外部命令,且上述替代补丁不足以缓解。

---

## auto-lang parser(`auto-lang/crates/auto-lang/src/parser.rs`)

> **2026-08-05 状态更新**:`#[api]`/`store` stack overflow 已修复**并已合入 master**。
> 修复历程:
> 1. `auto-lang-fix043` worktree(分支 `fix/043-parser-stack-overflow`)最初在
>    `parse_body` 的语句循环里加 `.field = expr` 点前缀分支,避免 expr_pratt 无限
>    递归——修复了 api.at 的深递归(>8MB 栈 → 2MB),但该分支增大了 parse_body 的
>    debug 栈帧,把 `test_godot_demo_dodge_player`(player.at,需 ~2MB)推过 2MB
>    libtest worker 线程栈 → 引入回归。
> 2. 最终方案(`d896d263`):把 `.field = expr` 处理移入 `dot_item()` 尾部——
>    parse_body / parse_stmt_inner 完全不动,栈帧与修复前一致。gdscript 63 测试全过
>    (dodge_player 恢复绿色),api.at 修复保持(2000KB 内可解析)。
>
> 当前 master 用**dot_item 方案**,验证结论:
> - 015-notes 的 `api.at`/`notes_store.at` 可正常编译(旧条目所述的 overflow 不再发生);
> - 我们的 `back/api.at` 编译通过(2000KB 栈内);
> - `store` 声明、`.field = expr` 赋值(handler/computed)均正常;
> - player.at 解析栈需求 2000KB,与修复前相同(零回归)。

### `[][]T` 嵌套数组类型不被支持

**来源**:Plan 043 M1(types.at 反向生成)。

**现状**:`parse_array_type`(parser.rs:8929)在解析完 `[]` 后调用 `parse_ident`(期望类型
名如 `int`/`str`),而不是递归调用类型解析(会处理 `[`)。所以 `[][]str` 在第二个 `[`
处失败:"Expected term, got RBrace"。

**根因**:parser.rs:8951 `let type_name = self.parse_ident()?` 应该改为能递归解析
嵌套 `[]` 的类型解析函数(如 `parse_type` 或 `parse_type_expr`)。

**影响**:`RenderedOutput` 的 `rows [][]RenderedCell`、`code_lines [][]CodeSpan` 等
嵌套数组类型必须用 `List<List<T>>` 替代(已确认可用)。

**临时绕过**:`types.at` 中用 `List<List<T>>` 替代 `[][]T`(已应用)。

**推翻条件**:auto-lang parser 的 `parse_array_type` 支持递归嵌套 `[][]T`。

**2026-08-06 部分解决(B 类修复,auto-lang commit `718e94aa`,分支
`fix/043-m5-bclass` 待合 master)**:`api.at` 的 `[][]T` 字段现在能进生成的 interface,
但**不是**靠修 `parse_array_type`——而是修了 **lenient API 提取路径**:
`api_gen.rs::parse_fields` 之前只认 `name: type`(带冒号)字段,把无冒号的
`rows [][]RenderedCell`/`commands []ToolEntry` 等**静默丢弃**(生成的 interface 缺字段,
调用方 TS2339)。这是 `auto build` 实际走的路径(`try_full_parse` 因 `use types:`
模块解析失败回退到 lenient 正则提取)。修复后 `parse_fields` 同时接受 `name type`
(空格)形式,`[][]T` 字段作为类型字符串原样进入 `ApiField`,`to_ts_type`/`auto_type_to_rust`
本就支持 `[][]T`(→ `T[][]` / `Vec<T>`)。**注意**:full-parse 路径(不走 lenient 时)
仍不支持 `[][]T`,此条 parser 级限制保留;推翻条件不变。

---

## auto-lang parser:store/widget 语法限制(2026-08-05 实测)

> 以下限制用**含 fix043 修复的 debug 二进制**(2026-08-05 15:51 构建)实测复现,
> 全部以最小复现文件验证过。015-notes 均无对应先例,因此不是我们的语法问题。
> 处理原则:不做规避(不改写语义),记录并等 parser/codegen 修复。

### `msg` 消息多参数声明不被支持

**来源**:Plan 043 M2(shell_store.at)。

**现状**:**2026-08-06 已解决**——auto-lang worktree `fix/043-m5-lang-limits`
(commit `f08539b5`)把 `MsgVariant.payload` 从 `Option<Type>` 改为 `Vec<Type>`,
parser 的 msg payload 解析改为循环 `parse_type()` 逗号分隔(参照 EnumItem tuple),
Rust 后端发多字段 enum(`Complete(str, int)` → `Complete(String, i32)`)。
Vue/Kotlin 后端保持 `.first()` 单字段(store 不走 Vue 事件链路;多参数为
Rust 后端能力,部分功能)。修复已合入该分支,待合 master。

### computed 只支持单表达式,不支持多行 body

**来源**:Plan 043 M2(shell_store.at 的 `history`/`git_label`)。

**现状**:**2026-08-06 已解决**——auto-lang worktree `fix/043-m5-lang-limits`
(commit `db19b947`)让 `parse_computed_block_inner` 在 `=>` 后遇到 `{` 时走
`Expr::Block(self.body()?)`(复用 on-handler 的语句块解析路径),单表达式路径不变。
AST 字段类型不动(`ComputedProperty.expr: Expr`)。Rust 后端 `ast_expr_to_rust`
新增 `Expr::Block` 分支(渲染成 `{ stmt; ...; tail }`);Vue 后端 `expr_to_js`
新增 `Expr::Block` 分支(走 `transpile_handler_body`,渲染成 `{ ...; return x; }`)。
修复已合入该分支,待合 master。

### view 条件里 `None` 比较不被支持(handler/computed 里正常)

**来源**:Plan 043 M4(block_item.at)。

**现状**:**2026-08-06 已解决**——auto-lang worktree `fix/043-m5-lang-limits`
(commit `f42ca89c`)的根因是 `parse_condition_expr`(parser.rs:12715 附近)只
match 了特定 token kind(True/False/运算符/Str/Int/括号等),**漏了 `NoneKW`/`Nil`**,
所以 `None` 未被消费,parser 在 arm 的 `}` 处 desync 报 "Expected term, got RBrace"。
加 `NoneKW`/`Nil` 分支即可。同时 vue.rs 的 `convert_condition` 把 Auto 的
`None`/`nil` 重写成 JS `null`(生成的 `v-if="block.output != null"` 是合法 JS)。
验证:`block_item.at` 现可编译,生成的 BlockItem.vue:43 为
`<template v-if="block.output != null">`。修复已合入该分支,待合 master。

### view 里位置参数调用 view fn 不被支持(必须命名参数)

**来源**:Plan 043 M4(block_body.at)。

**现状**:`render_table(.output)`(位置参数调用 view fn)报 "Expected term, got RBrace";
`render_table(output: .output)`(命名参数)正常。015-notes 里 view fn 调用
(`NoteItem(note: note, ...)`)全部是命名参数,无位置参数先例。

**根因**:view fn 调用的解析把位置参数当组件属性语法处理,预期的是 `name: expr` 形式。

**影响**:`block_body.at` 当前写法(位置参数)编译失败。**注意**:把调用改为命名参数
`render_table(output: .output)` 是**规范写法修正**(与 015-notes 一致),不是规避,
已应用。但跨文件 view fn 仍有下一项问题。

**推翻条件**:auto-lang parser 支持位置参数调用 view fn(或我们统一用命名参数即可绕开——
此条记录为低优先级)。

### view fn 跨文件 `use` 不被 codegen 支持(生成 `<div :output>` 而非内联展开)

**来源**:Plan 043 M4(renderers.at + block_body.at)。

**现状**:**2026-08-05 已解决**——根因不是"跨文件",而是**命名约定**:
extract.rs 的 view fn 内联检查 `is_pascal`(extract.rs:650),只有 **PascalCase 标签**
(如 `NoteItem`)触发内联;snake_case 名(如 `render_table`)不内联,落回普通组件
生成 `<div :output>`。解决:
1. 把 4 个 view fn 从 `renderers.at` 移入 `block_body.at`(同文件定义,与 015-notes
   一致)——`renderers.at` 已删除;
2. view fn 改名为 **PascalCase**(`RenderTable`/`RenderCode`/`RenderText`/`RenderError`);
3. 参数引用用**裸标识符**(`output.columns` 而非 `.output.columns`)——substitute_expr
   只替换裸 Ident 和单层 self-dot,嵌套 self-dot 无法替换。

**验证**:生成的 BlockBody.vue 中 4 个渲染器全部正确内联展开(Table/Code/Text/Error
分支的结构、循环、样式均正确)。

**推翻条件**:已推翻(见上)。

### view else-if 链生成嵌套错误(新发现,2026-08-05)

**来源**:Plan 043 M4(block_body.at 的 if/else if 链)。

**现状**:**2026-08-06 已解决**——auto-lang worktree `fix/043-m5-lang-limits`
(commit `e487e223`)的根因是 `vue.rs:3321` 的 `AuraNode::Conditional` 分支对**每个**
Conditional 都生成 `<template v-if>`,即使是 else-if 链的延续节点。`else if` 经
parser(parser.rs:12515)解析后是 `else_body` 内嵌套的单 Conditional,链尾第 3+ 层
在 `vue.rs:3346` 递归重新进入 3321 → 生成嵌套的 `<template v-else><template v-if>`
而非平铺 `v-else-if`。抽 `emit_conditional(node, indent, is_continuation)` helper:
头 arm 发 `v-if`,延续 arm 发 `v-else-if`,普通 else 发 `v-else`,整条链同 indent
(Vue 要求连续兄弟)。验证:`block_body.at` 现编译通过,生成的 BlockBody.vue 是
扁平的 `v-if`/`v-else-if`×4/`v-else` 6 个 `<template>` 兄弟。修复已合入该分支,待合 master。

### model 字段 `Type{...}` struct-literal 初始化:导入类型不被识别(2026-08-06 新发现)

**来源**:Plan 043 M5 闭环验证(shell_store.at)。

**现状**:**2026-08-06 已解决**——auto-lang worktree `fix/043-m5-lang-limits`
(commit `16f8188f`)根因是 `atom()`(parser.rs:3103)把 `Ident {` 的结构体构造
门控在 `is_type`(`lookup_ident_type`),而导入类型未解析时返回 `None` → 跳过
构造分支 → `{...}` 残留导致 desync。放宽:在 **UI scenario** 下,PascalCase 且
非已知变量(`Meta::Store`/`Meta::Ref`)的 ident 也接受 `{...}` 构造——与 013-todo/
015-notes 里 `Todo{...}`/`Note{...}` 的既有用法一致。**仅限 UI scenario**:
gdscript 等其他 dialect 复用 atom(),放开会重解释它们的 `Ident {` 导致栈溢出。
小写 ident 永远是普通变量。

**顺带修复(同一闭环发现)**:on-handler 多参数绑定 bug(commit `d6517a27`)。
`parse_on_block` 收集 handler 参数 token 后只保留**偶数下标**(假设 `name type` 成对),
对裸参数 `.RunSmart(block_id, name, args)` 会丢掉 `name`/`cursor` → codegen 报
`UndefinedVariable`。改为按逗号分组,每组取首 token(名),丢弃可选的 type。

**验证**:`shell_store.at` 现完整解析通过(所有 .at 文件零 parse error;
仅 `types.at` 报"No widget or store declarations"——那是纯类型文件,非组件,预期)。
6 项 Plan 043 M5 auto-lang 限制全部解决(msg 多参数 / computed 多行 body /
view None 比较 / else-if 链 / struct-literal init / handler 多参数)。

**剩余观察(非 parser 阻塞,记录备查)**:store 声明(`store ShellStore`)解析通过。
**2026-08-06 架构层已解决**(auto-lang commit `a96d4da2`,分支 `fix/043-store-codegen`
已合 master):根因是 `STORE_EXTRA_FILES` thread-local 在 `generate_component_from_file`
开头被 clear(api.rs:181),多文件 workspace 里只有最后一个 .at 的 store 幸存 →
`prepare_vue_sources` drain 到空 Vec → store 文件没写盘。修复:`VueProject` 加
`store_files` 显式字段,`from_workspace` 三个编译点(app.at/pages/front_dir)直接
从 `result.store_composables` 收集,`generate`/`regenerate_source_files` 显式写盘。
顺带修:`all_tags` 注入加 `has_notes` 门控(015-notes 专用,ShellStore 无 notes);
`store_init_to_js` 认 `List<T>.new([])` → `[]`;computed 多行 body getter 不再用
`return {…}`(无效 JS)。验证:`auto build` 现输出 `✓ Store composable: useShellStoreStore.ts`,
文件结构正确(refs + actions + getters)。

**store handler-body codegen 质量(2026-08-06 大部分已解决)**:auto-lang commit
`31c4b84d`(分支 `fix/043-store-codegen-quality` 已合 master)在 `ts_adapter` 补了 3 个分支:
- `List<T>.new([])` / `Array<T>.new()` → 数组字面量(原样输出 `List.new`)
- struct-literal `Type{…}`(Expr::Node)→ 对象字面量 `{ field: val }`(原 `new Type()`)
- `var x []str = []` / `let x int = 0` → 带 TS-builtin 类型注解(`let result: string[]`)
  (原隐式 any[]);**用户自定义类型(Block 等)不注解**(TS 端擦除成 `any`)
验证:`auto build` 的 `useShellStoreStore.ts` vue-tsc 错误 **8 → 1**。

**cat-3 action 互调链式合并(2026-08-06 已解决)**:auto-lang commit `654ba12e`
(Plan 043 Phase 5.5,已合 master)。根因是 `parse_body` 的 body-chaining 逻辑有 3 个
bug(用 5 模式 AST probe 验证,纠正了之前"RHS 跨行消费"的错误判断):
- **Guard 1**:target 搜索跳过点前缀语句(`.field = expr` 不是合法链式接收者)
- **Guard 2**:pop 阶段遇非 self-dot-call 立即停止(不把点前缀赋值卷进链)
- **Guard 3**:若回溯找 target 时跨过了任何点前缀语句,放弃链式(方法链接收者必须是紧邻的上一行)
另修 store composable 的 action 可见性:`generate_store_composable` 把 action 从返回对象的
内联属性改成 `const ActionName = ...` 闭包变量(返回对象引用它),这样 action 互调
(`RefreshGit()`)在闭包作用域里可见。验证:`useShellStoreStore.ts` vue-tsc 错误 **1 → 0**
(整个 store-codegen 旅程:8 → 1(cat-1/2/4)→ 0(cat-3))。

**043 M5 Phase 5.6 B 类(2026-08-06 已解决)**:auto-lang worktree 分支
`fix/043-m5-bclass`(commit `718e94aa`,待合 master)修 6 个子类 codegen 问题,
ash-gui-auto 的 vue-tsc 错误 **19 → 0**(+ vite build 成功)。分类与修法:

| 子类 | 错误数 | 根因(实测) | 修法 |
|---|---|---|---|
| B-1 msg 回调签名 | 6 (TS2322) | `on_pick: msg` 的 defineProps 总是生成 `() => void`,msg 带 payload 时父传 `(name: any) => void` 不兼容 | `vue.rs::prop_to_ts_type`:按 `on_pick`↔`Pick` 约定查 msg variant,payload 生成 `(arg0: T) => void` |
| B-2 类型 import | 4 (TS2304/2552) | `custom_types` 只收集 `Type::User` 直接类型,`List<Block>`/`[]ToolEntry`(容器内)和 defineEmits payload(`CompletionItem`)漏掉 | `vue.rs::collect_custom_types` 递归容器(GenericInstance/Slice/List/Option/...)+ defineEmits payload 收集,import 移到两段之后统一输出 |
| B-3 handler 参数名 | 2 (TS2304) | 循环内 handler 模板调用 `OpenPath(b)`,函数签名用 loop var `b`,emit body 却引用 on-block 声明的 `path`(未绑定) | emit 参数优先用 loop var(仅当 handler 声明了参数;无参 `.Stop` 仍 emit 无参,否则 `Stop: []` 报 TS2769) |
| B-4 [][]T 字段 | 2 (TS2339) | 见上 `[][]T` 条目:lenient `parse_fields` 只认冒号,无冒号字段静默丢弃 | `parse_fields` 支持 `name type` 空格形式 |
| B-5 cthis/sthis | 2 (TS2339) | `parse_event_arg` 把 `.c.name` 拼成 `this.c`+`this.name`=`this.cthis.name`,Vue 剥 `this.` 后是未定义的 `cthis` | `parse_event_arg` 的 Dot 分支 `this.field` 后置 `prev_was_ident=true` → `.c.name`=`this.c.name` |
| B-6 store BootSnapshot | 3 (TS2339) | 与 B-4 同根因:`commands []ToolEntry`/`smart_commands []SmartCommandEntry` 无冒号被 lenient 丢弃,interface 缺字段 | 同 B-4 |

**关键认知修正**:B-4/B-6 之前被误判为 "codegen 的 to_ts_type 对 [][]T 不完整"。
实测 `to_ts_type` 本就支持 `[][]T`;真正原因是 **lenient 提取路径的 `parse_fields`
只认冒号字段** + api.at 里 5 个字段漏写冒号。修 lenient 路径后 `.at` 源码无需改动
(`rows [][]RenderedCell` 等保持原样即可)。全量回归:auto-lang 2818 passed /
22 pre-existing 失败不变(已在纯净 master 上验证同样的 22 个失败);auto-man
178 passed / 1 flaky(HTTP 端口测试,重跑通过)。
