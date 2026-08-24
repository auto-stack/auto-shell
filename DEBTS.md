# DEBTS — 已知局限与权衡记录

> 这里记录的是**已经做出的、有意识接受的**技术局限与权衡——不是待办,而是
> "我们知道这里不完美,且有明确理由暂时不修"。每条带足够上下文,让未来的维护者
> 不必重新推导一遍。若要推翻某条决定,先读这里的理由。
>
> 与 [`TODO.md`](TODO.md) 的区别:TODO 是"以后可能做"的方向;DEBTS 是"现在故意
> 不做,因为代价/收益不划算"的记录。
>
> ---
>
> **2026-08-06 复审注(Plan 043)**:下方 auto-lang parser/store-codegen 各条
> "待合 master"措辞**均已过时**——`fix/043-m5-lang-limits`(6 项)、`fix/043-m5-bclass`、
> `fix/043-m5-runtime-bug`、`fix/043-m5-g1-sse`、5.5/5.7/5.8 的修复在 rebase 后**全部
> 合入 auto-lang master**,原 worktree 分支已删除。Plan 043 整体状态
> = **✅ 完成**(`auto build` + vue-tsc 0 + vite build 成功 + playwright 全 PASS)。
>
> **§5.9 闭环(2026-08-06 晚)**:原 §8.4 列的 3 项"剩余权衡"**全部做掉**:
> ① stream() SSE 改 `~Stream<T>` 类型驱动(auto-lang `c1b05e48`,消除命名启发式 + 死代码 fetch fn);
> ② Record 变体完整实现(RecordOutput + RenderRecord + MemoryInfo Progress,auto-lang `48d924cc` 加 tuple 下标 + Progress 动态 value);
> ③ types.at 过时 RenderedOutput 已删除。
> **Plan 043 至此无任何遗留 workaround。** 正文保留原始措辞仅供追溯;实际 master commit
> 映射见 `docs/plans/old/043-*.md` §8.3。

---

## ash-gui-vue 后端(`ash-gui/ash-gui-vue/src-tauri/`;**该目录 2026-08-23 已退役删除**,条目留档,见 git 历史)

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

> **2026-08-06 复审**:**功能上已解决**(lenient 提取路径,见本条末尾的 2026-08-06 更新)。
> `api.at` 的 `rows [][]RenderedCell` / `lines [][]CodeSpan` 现可正常进入生成的 interface
> 并通过 vue-tsc + vite build。**仅 full-parse 路径仍不支持**(理论限制,`auto build` 不走
> 该路径),故保留本条记录,推翻条件不变。`front/types.at` 里残留的 `List<List<T>>` 是
> 该文件的过时定义(已不被生成器使用,实际生效的是 `api.at` 的 variant-keyed 定义)。

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

### 比较运算符(`>` `<` `==`)在表达式上下文中导致 stack overflow

> **2026-08-06 复审**:**已不复现**——当前 `.at` 源码大量使用 `!=`/`==`/`>`/`<`
> (app.at `!= ""`、block_item.at `== "Success"`、tool_sidebar.at `> 0`、
> shell_store.at `== "Success"` 等),`auto build` + vue-tsc 0 错误 + vite build 成功。
> 推测后续 parser 修复(`dot_item` 演进 + cat-3 链式合并 `654ba12e` 等)已顺带覆盖了
> 比较运算符的 RHS 解析。下方"临时绕过"未再应用(.at 源码直接用比较运算符)。本条保留
> 供根因追溯,推翻条件视为已满足;如未来再现再深挖。

**来源**:Plan 043 M5(正向生成验证)。

**现状**:`fn f() { x > 0 }` 或 `if x > 0 { ... }` 在任何 body 中解析时,
触发 `expr_pratt_with_left` 的无限递归 → stack overflow。`x = 0`(赋值)和
`x + 0`(加法)可以正常解析,但 `>` `<` `==` `!=` 等比较运算符不行。

**根因**:`expr_pratt_with_left` 的 infix loop 在处理比较运算符时,RHS 解析
`expr_pratt(power.r)` 返回后 token 流不推进,导致循环无限重复。具体原因未定位
(可能在 `self.next()` skip binary op 与 RHS 解析的交互中有 bug)。

**已知部分修复**:auto-lang master 上已有 `dot_item` 中对 `=` (Asn) 的拦截
(parser.rs ~line 1970,标注 "Plan 043"),但仅覆盖赋值,不覆盖比较运算符。

**影响**:Plan 043 的 `shell_store.at` 无法解析(on handler 里有 `if .x > 0`),
M5(正向生成验证)被阻塞。所有使用比较运算符的 Auto UI 代码都受影响。

**临时绕过**:在 .at 源码中避免在 body/computed 中使用比较运算符。
将条件逻辑移到前端 Vue 组件中(手写逃逸)。

**推翻条件**:auto-lang parser 的 `expr_pratt_with_left` 正确处理比较运算符
的 RHS 解析(不无限递归)。

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

**043 M5 Phase 5.6 B 类合 master + 功能修复(2026-08-06)**:
- `fix/043-m5-bclass` 已合 auto-lang master(merge `e4fd405d`)。
- **功能缺口补修**(完整性核查发现,非类型错误):
  1. **App.at `on_run: .RunCommand` 空桩**:view 引用了未声明的 `.RunCommand`,
     codegen 生成 `function RunCommand(){}` → PromptBar 回车不执行命令。补
     `msg Msg` 加 `RunCommand(str)` + on-block 加 `.RunCommand(cmd)` 处理器。
  2. **computed 的 if/else-if 表达式生成 `undefined`**:`status_glyph`/`status_cls`
     落 `expr_to_js` 兜底(Expr::If 无分支)→ `computed<any>(() => undefined)`,
     状态图标/颜色类静默丢失。auto-lang master commit `92314c2d` 给 `expr_to_js`
     加 `Expr::If` 分支(镜像 ts_adapter 的 IIFE 方案):
     `(() => { if (c1) { v1 } else if (c2) { v2 } else { v3 } })()`。
  验证:生成的 BlockItem.vue 状态字形/类正确,App.vue RunCommand 带
  `store.RunCommand(cmd)`,vue-tsc 0 + vite build 成功。

**043 M5 Phase 5.7 R3(2026-08-06 已解决)**:RenderedOutput 数据契约不匹配 —
auto-lang worktree `fix/043-m5-runtime-bug` commit `540fbcdb`(待合 master)。

**现象**:R1(struct-literal)修完后用户实测 `ls -al` 仍"没有反应"。curl
`/api/stream` 确认服务端发 **serde externally-tagged union**
(`{"Table":{columns,rows}}` / `{"Text":"..."}`,单元格 `{"Tagged":{text,tag}}`),
而生成版前端按扁平 `output.kind`/`output.columns` 访问 → 全部 undefined。
api.at 的 `RenderedOutput` 改 variant-keyed 可选字段后,block_body.at 重写报
`"Expected term, got RBrace"`——暴露 auto-lang 两个独立缺陷:

1. **struct-literal widening 误伤 dot 表达式 RHS**(parser.rs,`in_dot_rhs`):
   上一轮 struct-literal 修复让"任意 PascalCase 标识符 + `{` → 构造"。
   `text cell.Text { }` 里 `Text` 后紧跟元素 props 的 `{`,被当 `Text{...}`
   构造吞掉 → desync。小写 `cell.text` 不受影响,所以此前一直没暴露。
   修复:parser 加 `in_dot_rhs` 标志,仅 `Op::Dot` 的 RHS 抑制 widening;
   `x = Type{...}`(Asn)等不受影响。dot 表达式 RHS 是字段/方法名,结构体
   字面量永远是独立 `Type{...}`。

2. **view fn 内联时 ForLoop iterable 未做参数替换**(extract.rs):iterable 是
   字符串,`expand_fragment_node` 原样保留(原版靠 widget 同名 prop 碰巧
   解析)。修复:iterable 也走 `substitute_condition` — `for col in
   output.columns` → `output.Table.columns`(窄化到 variant 子类型);
   `expr_to_condition_str` 修 self-dot 基座:`.output.Table` → `output.Table`
   (而非 `self.output.Table`)。

**验证**:回归测试 `test_dot_rhs_field_access_not_struct_construction`;
parser 161 + gdscript 63 + aura 46 全过;ash-gui-auto vue-tsc 0 + build 成功;
生成 BlockBody.vue 为 `v-if="output.Table != null"` + `v-for="col in
output.Table.columns"` + `cell.Tagged.text`;SSE 实测 `status:"Success"` /
`Table{columns,rows}` / 单元格 `Tagged{text,tag}` 与 api.at 契约完全吻合。

**遗留(记录)**:`Record` 变体在 api.at 简化为 `?str`(实际为 `{fields,atom_type}`
对象,stat/date/version 命令走此分支会显示 `[object Object]`);单元格 `tag`
类型简化为 `str`(实际为 `"Dir" | {FileName: Kind}`)。均不影响当前 ls/Table
链路,后续如需 Record/着色可再补。

**043 M5 Phase 5.7 R4(2026-08-06 已解决)**:子组件回调事件名不匹配 —
auto-lang master commit `2456a18b`(已 push)。

**现象**:R1-R3 修完契约后 playwright-cli 实测仍无反应——block 不出现、
无 /api/run_command 请求。`text=ls -al` 匹配到的是输入框自身的值(假阳性)。

**根因**:生成的父组件监听 `@_run`,子组件 emit `Run`——事件名永不匹配。
vue.rs `base_event_to_dom` 兜底把 `on_run` strip "on" → `_run`(本是为 DOM
事件 `onclick`→`click` 设计的);而子组件从 msg 变体 emit PascalCase 名
(`Run`/`OpenPath`/`Stop`)。PromptBar 回车从未触发 App.RunCommand —— 这才是
"ls -al 没有反应"的直接元凶(R1-R3 修的是 RenderedOutput 契约,两者叠加)。

**修法**(vue.rs 两处):
1. `sub_widget_event_to_vue`:known sub-widget 的 `on_*` 回调 prop 绑定为
   `@Pascal`(msg 变体名,与 prop_to_ts_type 的 `on_pick`↔`Pick` 约定一致);
   非 `on_` 事件保持 DOM 映射(`onkeyup`→`@keyup` 等)。
2. `prop_is_emitted_callback`:有匹配 msg 变体的 `on_*: msg` 回调 prop 从
   defineProps 移除——回调经 emit 到达(Vue 把 `@Run` 转 onRun fallthrough),
   保留必需 `on_run` 会让父级对象缺字段 → TS2345。无匹配变体的 `on_*` prop
   仍是真实 prop(`:on_xxx="..."` 绑定)。

**验证**:3 个新回归测试(`test_sub_widget_on_prop_binds_pascal_emit_name` /
`test_sub_widget_omits_emitted_callback_prop_from_define_props` /
`test_custom_type_import_in_define_props`)+ B-1 测试改新契约(`Pick: [string]`
在 defineEmits)。干净 master **2828 passed / 22 pre-existing**。ash-gui-auto
vue-tsc 0。**playwright-cli 实测全 PASS**:ls -al 表格(5 列头 + 数据行)、
cat Cargo.toml Text 输出。测试脚本 `ash_gui_test.cjs`(playwright chromium
1228 headless,指定 executablePath;ESM 需 file:// 或 CJS require)。

**注意**:事件绑定 `@_run` 的 DOM 兜底仍保留(用于真正的 DOM 事件);R4 只
影响 known sub-widget 的 `on_*` 回调 props。HistorySearch 的 `on_close` 在
.at 里接了 `.ToggleHistorySearch` 但子组件从不 emit `Close` —— 潜在空接线,
记录备查,不影响主链路。

**043 M5 Phase 5.7 R4b(2026-08-06 已解决)**:Phase 1 组件的 PascalCase 兜底
子组件事件名 — auto-lang master commit `6445b9c3`(已 push)。

**现象**:R4 只覆盖 known_sub_widgets(Phase 2 app.at 经 with_sub_widgets 传入)。
Phase 1 前端文件(prompt_bar/block_list 等)编译时无 known_sub_widgets,兄弟
子组件(HistorySearch/BlockItem)经 map_tag 的 PascalCase 兜底走 plain-element
路径 → 事件仍用 DOM 兜底 `@_run`/`@_open_path`,与子组件 emit(Run/OpenPath/Stop)
不匹配。对比生成/原生代码发现。

**修法**:plain-element 事件发射处,`html_tag` 为 PascalCase(map_tag 兜底成
的自定义组件)时用 `sub_widget_event_to_vue`(`on_*` → `@Pascal`);DOM 元素
保持 `auto_event_to_vue`。新增回归测试
`test_pascalcase_fallback_element_on_prop_binds_pascal_emit_name`。

**验证**:干净 master 2829 passed / 22 pre-existing;ash-gui-auto vue-tsc 0;
生成 PromptBar `<HistorySearch @Run @Close>`、BlockList `<BlockItem @Stop
@OpenPath @Rerun>` 全部对齐子组件 emit;playwright 11 项全 PASS。

**生成 vs 原生对比遗留缺口**(结构对比发现;1-2 已于 2026-08-06 修复):
1. ✅ **已修复** BlockItem rerun 按钮:block_item.at 加悬停显示的 ⧉ 复制(调
   navigator.clipboard)+ ↻ 重跑(onclick .Rerun(.block.command))→ BlockList
   @Rerun 激活,playwright 实测点击触发 POST /run_command
2. ✅ **已修复** BlockBody 单元格可点击:单元格 onclick .OpenPath(text) →
   BlockBody emit → BlockItem → BlockList → App .OpenPath → store.OpenPath →
   POST /api/open_path(OS 打开);playwright 实测点击 Cargo.toml 触发。所有
   单元格可点击,非路径点击无害失败(服务端忽略)
3. ✅ **已修复** PromptBar 键盘快捷键:Ctrl+R 历史搜索 / Ctrl+L 清屏 /
   Ctrl+C 清输入 / ↑↓ 历史 / Tab 补全(依赖 auto-lang R5 之前的 R4b 事件绑定)。
   watch 块处理 injected_command 填输入框。Ctrl+D 退出仍无条件 emit(浏览器
   无操作;Tauri 会误退,记录待修)
4. ✅ **已修复** HistorySearch 过滤 + 键盘:输入过滤(大小写不敏感子串)、
   ↑↓ 选中、Enter 执行、Esc 关闭;选中项高亮(bg-accent)
5. ✅ **已修复** 首列着色:单元格条件样式(
   if idx == 0 { sky } else { cursor })。依赖 auto-lang R5(b4ab6d4c):
   shadcn 路径支持 Expr::If → :class 三元(此前 text/span 等 registry
   组件静默丢弃条件样式)
6. ✅ **已修复** 表格用 shadcn <Table> 布局:thead/tr/th/tbody/tr/td 结构,
   列对齐(依赖 R5 前已有的 table 标签映射)
   注:tag 按类型着色 2026-08-06 已补全——server 的 RenderedCell::Tagged
   序列化附加扁平 `kind` 字段(ash-core renderer.rs 自定义 Serialize,枚举不
   动 → iced/手写 vue 零影响),.at 用 `TaggedCell.kind` 多分支条件 style
   镜像 cellStyle.ts 配色(Dir→sky / CodeAtRs→emerald / Executable→cyan /
   Config→amber / Permission→muted / Plain→fg);依赖 auto-lang R6(a7c5b684)
   的 else-if 链→嵌套三元。Ctrl+D 空输入退出:expose{ .Exit } 标记 used +
   OnCtrlD 条件调用(浏览器无操作,Tauri 正确)。HistorySearch 退格关闭。

---

## Plan 043 §5.9 归档前复审发现的功能缺口(2026-08-06)

> **性质**:归档前按 plan-archiver 技能 Step 2.5 做的 debt-review pass 发现。playwright
> 覆盖的 ls/cat/stat/date/version/sys mem 成功路径通过,但**相邻用户可见功能有真实缺口**。
> 这些推翻了此前"无遗留 workaround"的说法——§5.9 解决了原列 3 项权衡,但复审又发现下列
> 🔴/🟡 项。
>
> **2026-08-07 更新**:**H1/H2/H5 已修复**(auto-lang `db384870`,worktree
> `auto-shell-043-codegen-parity`)。这 3 项是 auto-lang codegen bug(.at 源码正确),
> 已用 `auto-shell-` 前缀 worktree 修复合 master + push。playwright 实测:状态字形 ✓
> 渲染、`show` 语法高亮(75 个着色 span)、重跑按钮 + `$event` 转发、ls/cat/stat 不回归。
>
> **2026-08-07 二次更新**:**H3/H4/H6 已修复**(auto-shell `.at` 源码,commit 见下)。
> 这 3 项是 .at 源码契约错误或样式写法问题。
> - **H3** playwright 实测 ✅:失败命令显示 ✗ 字形 + `result.status.Failed` 错误消息。
> - **H4** 代码正确但**运行时无法端到端验证**——当前 ash-server 未注册任何 smart 命令
>   (`/api/command_list` 返回 `smart_commands: []`),ToolSidebar 不渲染 smart 按钮。
>   生成的 `RunSmart` 已推 block + 设 `output = { Text: result }`,待有 smart 命令时即生效。
> - **H6** 代码正确(选中行有 `:class="idx == selected ? '...bg-accent...' : '...'"`)但
>   **playwright headless 难以验证**——HistorySearch 面板 `absolute bottom-full` 在
>   900px 视口下定位到视口外(`Element is outside of the viewport`),且过滤依赖 keyup 事件
>   (与 PromptBar G2 同款的 v-model 吞 oninput 兜底)。生成代码层面已修复。
> **全部 6 项 H1-H6 修复完成。** 剩余仅为 🟡 一致性打磨项。

### 🔴 高风险(功能错误或缺失,codegen/.at 双方都有)

| # | 缺口 | 根因 | 位置 |
|---|---|---|---|
| ~~H1~~ ✅ | ~~状态字形永不渲染~~ | ~~codegen IIFE 漏 return~~ → **已修**(auto-lang `db384870`:`transpile_body_as_return` 给 Expr::If IIFE 的分支加 return) | 生成 `BlockItem.vue:8-9` |
| ~~H2~~ ✅ | ~~重跑/点击打开失效~~ | ~~msg-forwarding 用 loop var~~ → **已修**(auto-lang `db384870`:`msg_payload_arities` 索引;带 payload 的 handler 转发 `$event`,无 payload 的保留 loop var) | 生成 `BlockList.vue:47` |
| ~~H3~~ ✅ | ~~失败命令不显示错误~~ | ~~`failed_message` 字段服务端不存在~~ → **已修**(`api.at` CommandResult 删 `failed_message`,`status: str` 联合擦除成 any;store RunResult 改读 `result.status.Failed`,JS 动态派发——`status=="Success"` 走成功,否则 `.Failed` 取消息) | `back/api.at` + `shell_store.at:101` |
| ~~H4~~ ✅ | ~~SmartCommand 永无输出~~ | ~~store RunSmart no-op~~ → **已修**(`api.at` run_smart 返回 `str` 裸文本而非 SmartResult 结构;store RunSmart 镜像手写 runSmartCommand:推 Running block → `run_smart()` → `output = RenderedOutput{ Text: result }`) | `back/api.at` + `shell_store.at:121` |
| ~~H5~~ ✅ | ~~`show` 语法高亮丢失~~ | ~~push_style_class 丢弃 Expr::Bina 拼接~~ → **已修**(auto-lang `db384870`:动态 style 表达式渲染为 `:style="<expr>"`) | 生成 `BlockBody.vue:78` |
| ~~H6~~ ✅ | ~~HistorySearch 选中无高亮~~ | ~~`+` 拼接的 if/else 条件被 codegen 渲染成 null~~ → **已修**(改纯 `if/else` 条件 style,不拼接;生成 `:class="idx == selected ? '...bg-accent...' : '...'"`) | `history_search.at:50` |

### 🟡 一致性遗漏(功能可用但与手写版不一致)

- **BlockItem 缺 duration 标签**(手写版显示 `123ms` Badge)、**cwd 未缩写**(手写用 `abbrevPath`)、**copy 无错误兜底**(`navigator.clipboard` 无 `?.`/`.catch`,非安全上下文抛异常)。
- **HistorySearch**:大小写敏感(手写不敏感)、最旧在前(手写最新在前且 cap 50)、无自动聚焦/执行后关闭/计数。
- **ToolSidebar 丢弃命令描述**(手写有 `:title` tooltip + 内联描述,生成只有名字)。
- **PromptBar 补全无 debounce**(每次 keyup 即触发,手写 80ms debounce + 序列号防竞态)。
- **SSE 无 onerror/无重连**:`__streamConnected` 永久 latch;EventSource 关闭后不重连(手写同样无 onerror,但无 latch)。`Number(field[1].Text)` 无 `isFinite`/剥 `%`(`"75%"` → NaN)。

### 🟢 已知限制(设计权衡,手写版有但生成版暂缺)

- **PromptBar ghost text 自动补全**:`.at` 有 `AcceptGhost`/`HistoryOlder` stub 但无 keydown 接线;无内联补全预览、Ctrl+F 接受全词、Ctrl+Right 接受单词。
- **PromptBar 语法高亮 overlay + 多行续行**:生成版是单行 `<input>`,无 `tokenize()` overlay、无 `·` 续行提示。
- **store 全 `any` 类型**:10 个 ref + ~25 个 handler 参数为 `any`,掩盖了上述类型契约错误(H3)。这是 codegen 的有意简化(用户自定义类型在 TS 端擦除)。

### 推翻条件
- ~~H1/H2/H5 已修(auto-lang `db384870`)~~ ✅
- ~~H3/H4/H6 已修(auto-shell `.at` 源码)~~ ✅
- **全部 6 项 H1-H6 修复完成。** Plan 043 可按"功能对等"归档(剩余仅为 🟡 一致性打磨:
  duration 标签 / cwd 缩写 / copy 兜底 / HistorySearch 排序与大小写 / ToolSidebar 描述 /
  PromptBar debounce / SSE onerror / ghost text overlay——均非功能缺失,是体验打磨)。
- 验证:H3 playwright ✅;H4 待 smart 命令注册;H6 生成代码正确(面板布局/keyup 为独立项)。

## Vue 可做 / VM(iced) 不能做:能力差距记录(2026-08-21 实测,Plan 058/059)

> 2026-08-21 表格增强与行内编辑两轮开发中实测撞到的 VM 端能力缺口。
> 分两类:**A. 渲染层**(iced 布局引擎 vs 浏览器 CSS,多数是结构性差距);
> **B. VM 语言/运行时**(参数传递/事件/原语缺陷,属可修引擎 bug 或未实现)。
> 每条附根因与对策;推翻条件 = 引擎侧补齐后可解除对应 workaround。

### A. 渲染层(浏览器 CSS 原生支持,iced 无对应机制)

| 能力 | Vue(浏览器) | VM(iced 0.14) | 根因 |
|---|---|---|---|
| 列宽拖拽 | pointer 事件 + CSS grid px 值 | ❌ | iced grid 只有等宽 `FillPortion` 轨道,无逐列宽度、无拖拽事件(renderer.rs build_grid) |
| 表头吸顶 | `position: sticky` 一行 CSS | ❌ | iced 无视口定位,Sticky 降级为普通流(iced_adapter.rs) |
| 行 hover 高亮 | `:hover` 伪类 | ❌ | StyleClass 不解析 `hover:` 前缀,静默跳过(class.rs);mouse_area 仅用于调试 |
| 文本溢出 ellipsis | 原生 | ✅(text-ellipsis 有) | — |

**对策**:此类功能只在 Vue 端做(auto gen 产物 + codegen 注入脚本,计划经
`ash-table-*` 标记类识别);VM 端接受降级(等宽/无 hover/无吸顶),.at 里的
`hover:`/sticky 类对 VM 无害(被忽略)。

### B. VM 语言/运行时(引擎缺陷或未实现,Plan 059 三重限制即源于此)

1. **嵌套列表经 widget 参数传递解析为空**
   - 现象:`widget T(output: TableOutput)` 内 `for row in output.rows`
     (rows `[][]RenderedCell`)迭代 0 次;同结构 `output.columns`(`[]str`)正常。
   - 根因:widget 参数绑定的物化只处理一层列表,嵌套 list-of-list 解析为空
     (aura_view_builder 值物化路径)。view fn 的**文本替换**走状态路径
     (`output.rows` → `.block.output.Table.rows`)则正常——两条通道语义不一致。
   - 对策:表格渲染内联在持有数据的 widget view 里,或经 view fn 参数文本替换。

2. **view fn 内不允许事件**
   - 现象:view fn 体内 `onclick:`/`oninput:` 解析报
     `unsupported event argument`(block_item.at 曾 20 连错)。
   - 根因:view fn 是调用点内联的模板片段,事件必须解析到**宿主 widget** 的
     msg;解析器直接拒绝。
   - 对策:带事件的标记必须内联在 widget view 中(Plan 059 表格即如此)。

3. **事件参数文法受限:不允许表达式**
   - 允许:model ref(`.field`)、循环变量、字符串/数字字面量、map 字面量、`$event`。
   - 不允许:拼接/方法调用(`.Sort(id.str() + ":" + ci.str())` 报错)。
   - **多参数合法**:`.Sort(.block.id, ci)` 可解析且 payload 可解码
     (decode_payload 返回参数向量)——Plan 059 排序桥接即用双参。
   - 根因:事件参数在渲染期编码进事件名字符串,只支持可静态求值的简单形式。

4. **`sort_by`/`sort_by_key` 比较器被静默忽略**
   - 根因:native.rs shim_list_sort_by 与 engine.rs sort_by 臂弹出闭包后直接
     丢弃,按默认比较排序(cookbook 003_sort_struct 的断言实际未过排序)。
   - 对策:手写选择排序(push 构建新列表;勿用 `rows[i]=rows[j]`,见下条)。

5. **`SET_ELEM` 列表索引赋值按 i32 弹值且仅支持 ListData<Value>**
   - 根因:engine.rs SET_ELEM 只 downcast `ListData<Value>` 且 `pop_i32` 取值
     —— 赋字符串/引用必然错坏(Tab 补全 PickCompletion 静默中止的根因)。
   - 对策:重建列表(push)或 Rust 侧直改;字符串拼接用 substr 扫描替代
     split/join(见下条)。

6. **`str.split` 返回字符串表索引的 i32 列表**
   - 根因:engine.rs split 臂把 part 压成 `strings` 池索引存入
     `ListData<i32>`(非 `ListData<String>`),join 不解码 → 往返产出
     `"-1151|-1152"` 之类数字。
   - 对策:.at 内避免 split/join 往返;需切词用 substr 逐位扫描(生产验证)。

7. **子→父 callback emit 被剥离(桥接负担)**
   - 根因:VM handler codegen 剥离 callback prop 调用(Plan 370 D-GAP-4),
     空体 handler 不会上行 —— 所有子 widget → store 的交互都要 renderer
     特例直改状态(ToggleCollapse/Plan 059 Sort/Filter/OpenPath 均此模式)。
   - 推翻条件:child callback emit 桥实现后,可批量删除 renderer 特例。

### 排查经验(记录给未来)

- **传递加载静默吞解析错误**:transitive 模块 parse 失败被 lib.rs 2470 静默
  丢弃(且 `auto <file>` 路径不初始化 logger,warn 也看不到),widget 不注册
  → 渲染为空。**诊断用 `auto ui inspect <file>`**(唯一能暴露这类错误的路径)。
  widget 参数必须带冒号(`output: T`),view fn 参数不带 —— 混写即触发此坑。
- **`auto <file>` 每次启动全量重扫 use 链**,新增 .at 文件只需父级 `use` 声明,
  无需注册/缓存操作(.auto/ui-cache.json 只属 vue codegen,与 VM 运行时无关)。

### 推翻条件

- B1/B3 修复(widget 参数嵌套物化、事件参数表达式)→ Plan 059 的 renderer
  桥接可退役为纯 .at 实现;
- B4/B5/B6 修复 → .at 侧可恢复惯用 sort/索引赋值/split 写法;
- B7 修复 → renderer 特例(Sort/Filter/OpenPath/ToggleCollapse/OnCtrlL)收敛为
  标准 emit 链。

### B8. 事件参数的前导点路径不解析(2026-08-21 追加,Plan 059 排序错对象根因)

- 现象:事件参数写 `.block.id`(带前导点)时,渲染期烘焙失败 —— 实测点
  下方 block 的表头,排序/折叠都作用到**上方 block**(id 恒解析为 0)。
- 根因(烘焙侧 + 解码侧双因):
  1. `event_to_message_with` → `resolve_binding_path(".block.id")` 按 `.`
     切分得 `["", "block", "id"]`,首段为空 → `bindings.get("")` None →
     落入**字面量分支**,参数变成垃圾字符串(aura_view_builder.rs);
  2. renderer 侧 `args.first().map(|v| v.as_int())` 对 Str 返回 0 →
     id 恒 0 → 永远命中第一个 block。
  循环变量参数(`ci`、`s`)无前导点,**烘焙正常**。
- 影响面:所有 `.X(.block.y)` 形态的 leaf-button 事件(ToggleCollapse/
  Rerun/PickCompletion/Plan 059 Sort)在真实点击下同样错对象 —— 此前
  "collapse 正常"的认知存疑(多为单 block 场景未暴露)。
- 修复方向:① resolve_binding_path 剥前导点 + 把 widget 参数(block)
  播种进 bindings;② renderer 解码对非 Int 参数显式报错而非静默 0。
- 临时对策:参数只用循环变量/字面量;block 身份改经其它通道传递。


## ash-gui 外部后端(Plan 061 延期项)

### 后端插件 ABI 为 `extern "Rust"`(同工具链约束)

- **约束**:宿主(auto.exe)与后端 cdylib 必须同一 rustc/同机同 target 树构建;
  跨工具链/跨机装载由 ABI 版本号拒载兜底(不会错装,但不可用)。
- **接受理由**:本机开发工作流天然满足;C-ABI 全量 marshalling(富类型/闭包)
  成本远超收益。设计定稿见 designs/ash-gui-external-backend.md §3。
- **参考**:`auto-lang/crates/auto-lang/src/vm/backend_abi.rs`。

### `auto run --http`:后端项目独立起服的 auto 入口未实现

- **现状**:HTTP 形态需 `cd ash-gui/ash-server && cargo run --bin ash-server`(:3000 + SSE)。
- **设计意图**(designs/ash-gui-external-backend.md §2):后端项目自带 api.at,
  启动形式(merged cdylib / HTTP)应只是部署参数——merged 已由 `auto run -r vm`
  兑现,HTTP 侧缺 auto 入口。
- **接受理由**(用户裁定,2026-08-23):日常主路径是 merged;HTTP 使用频率低,
  cargo run 足够;auto-man 侧需新增后端项目识别 + --http 编排,优先级不划算。
- **参考**:merged 装载编排在 `auto-man/src/rust_ui.rs`(load_external_backend);
  HTTP bin 入口 `ash-server/src/bin/ash-server.rs`。
- **连带观察**:`AUTO_VM_MERGE=0` split 开关(Plan 340 既有机制)实测未生效
  (仍走 merged 分支;2026-08-23 finish-plan 复审发现,非 061 回归)——实现
  --http 时顺带排查。

## ash-gui CLI 对齐(Plan 062 延期项/已知限制)

### T12 升级件:AiChunk SSE 事件族 + 右侧抽屉 chat 面板(需引擎窗口)

- **现状**:T12 已落零引擎「块内 chat」(`??` 前缀,流式经既有 CommandOutput
  事件,§12);原规格的 AiChunk/AiToolCall/AiToolResult 事件族 + 抽屉面板未做。
- **接受理由**:新 SSE 事件族需动 auto-lang master(renderer 白名单/vue 链),
  430/432 在途占用;块内形态已达成 CLI block_tui 对齐。
- **参考**:`ash-server/src/worker.rs` spawn_chat_worker。

### T13 suggest-next:环境依赖 Ollama(本机未装)

- **现状**:命令完成后的「接下来」建议未做;CLI 侧同为 best-effort 后台线程。
- **接受理由**:依赖本地 Ollama 服务,本机未安装;装好后可按 CLI ai/suggest.rs
  的 PENDING 槽 + CommandResult 后拉取的 RefreshContext 链(与 ai_pending 同款)接入。

### T14 smart NL 路由:与 T12 升级件同捆

- **现状**:run_smart 名字失败 → NL 路由未接;`nlu::route` 已确认可复用
  (client 注入,走 aaid local 池)。
- **接受理由**:需离主线程路由(Agent 整轮秒级)+ GUI 入口设计(与 chat 面板
  交互形态相关)+ local 池质量验证,三件与 T12 升级件重叠。

### 引擎侧预存:auto-lang 430/432 在途致 `test_auto_expression_execution` 挂

- **现状**:auto-shell 单测中 Auto 数组显示成 `<obj#…>`(期望 `[1, 2, 3]`),
  与本仓改动零交集(stash 对照因 auto-ai 编译漂移无法成立)。
- **接受理由**:auto-lang master 被 430/432 占用,重建 auto.exe 即吸入在途
  WIP;待引擎侧确认/修复后回归。

### 已知限制(小项,视觉/边角)

- AI 翻译/建议块在翻译中不可 Stop(全局 cancel flag 与其他命令生命周期有竞态,
  误判会把建议错标 Cancelled;翻译秒级)—— `worker.rs` Run 分支。
- `CommandResult.output = RenderedOutput::Empty` 序列化为裸字符串 `"Empty"`
  (非 null),引擎 update_block_in_state 不走 streamed_text 回退且清空之 ——
  长流式收尾必须自带 `Text(全文)`(chat 线程已如此,新调用方需注意)。
- Vue 端 `ai_pending` 编辑回填链未在本环境验证(块卡片按钮在 Vue 走 .at
  handler 原生可用)。
- prompt_bar `auto_hint`(# 符号)是引擎 is_auto_expression 静态强信号的
  启发式镜像,两端口径可漂移(仅视觉提示,执行路由以引擎为准)。

## ash-gui 引擎侧在册债(Plan 058/060 复审入账,2026-08-24)

> finish-plan 复审 056-060 时发现以下债此前仅记录于计划正文,未入账。
> 均为引擎(auto-lang)或 VM 机制域,短期内不可在本仓解决。

### MCP 键盘派发每实例偶发死(Plan 060 R16 发现②)

- **现状**:`autoui_keyboard`(Ctrl+r/Enter 等)在约半数启动实例上完全无效
  (重试/预热/聚焦均不救),另一半正常 —— 同一二进制,启动期竞态。真实键盘
  不受影响。
- **现行 workaround**:键盘依赖测试(history_search/pb09/CC-01)实例级 skip,
  注明 Plan 060 R16。
- **根因**:疑似 key_bindings 填充竞态,未定位;auto-lang 域(master 长期被
  并行 Plan 占用,2026-08-24 仍在 433 后演进)。
- **推翻条件**:auto-lang 侧定位并修复启动期键盘订阅竞态。

### 快速连打时 input 重放丢命令(Plan 060 第五轮)

- **现状**:type 后 <80ms submit 时,oninput debounce 重放旧 input,renderer
  RunCommand 桥判"input 非空"丢弃命令(不建块、无任何反馈)。
- **现行 workaround**:测试辅助 `_submit_command` type 后等待 input 落盘再
  submit(0.4s 间隔);真实用户快速操作仍可触发。
- **根因**:序列号守卫未覆盖 input 重放路径;引擎/renderer 机制域。
- **推翻条件**:renderer 侧对 input 重放加序列号守卫或以提交时快照为准。

### 字符串池无 GC / VM 确定性析构未接线(Plan 060 第十二/十三轮)

- **现状**:VM 字符串池只增不减;`DROP=0x05` 为空壳、codegen 零发射,
  设计声明的三层生命周期(作用域清理/逃逸分析/Shared 引用计数)在 VM 后端
  均未接线。正确性风险已被 u32 化 + 内容去重消除,仅内存单调增长。
- **接受理由**:量级可控(ash-gui 重启即释放);真 RAII/GC 均为引擎大件。
- **推翻条件**:auto-lang 立项实现(路线见 060 §第十二轮:DROP 发射+引用
  计数 / 标记清除 / 定期重建三选)。

### 行内编辑平台限制(Plan 058 §5,iced 0.14 约束)

- **undo/redo 无 API**(C-_/C-x u 不做);yank-pop(M-y)、quoted-insert(C-v)、
  word-case(M-u/l/c)、Vim operator-pending(d3w/ciw)/visual/`:` 命令行、
  Vue 端 Vim 模式(需 JS 层)—— 均未实现,表结构留有扩展位。
- **接受理由**:iced 0.14 `TextEditor` 无撤销 API;其余为低频编辑件,
  键位表架构已就绪,后续可增量补。

## Vue 产物构建引擎侧阻塞(Plan 057 Phase 5 T-B,2026-08-24)

> `auto gen` 重生成 + 仓内契约修复后,vue-tsc 余 13 错 / 5 类,均为 auto-lang
> vue/ts codegen 域(master 时点被 Plan 443 会话占用,未动)。修复后 Vue 构建
> 应能全绿;当前 Vue/浏览器模式**不可构建**,merged VM 模式不受影响。

1. **子组件回调命名不一致**(3 错):BlockList/BlockItem 的 props 类型生成
   `on_delete: () => void`(snake_case 且必填),而 App/BlockList 绑定发射
   `onDelete` —— 名字永不相配(043 R4 修过 PascalCase emit,`Delete` 形态漏网)。
2. **可空变体字段模板访问**(2 错):`cell.Tagged.text` 在 v-if 守卫内仍报
   TS18049,生成物需 `?.`。
3. **多参 emit 参数数量**(2 错):`Sort(int,int)`/`Filter(str)` 的 emit 签名
   生成 0 参(043 B-1 只修了单 payload)。
4. **VM stdlib 泄漏进 JS**(4 错):prompt_bar cd 补全 handler 的
   fs.read_dir/File.is_dir/`await complete`/`fs` 原样输出到 .vue script ——
   VM-only 原语无 JS shim 时应降级或报错,而非生成坏 JS。
5. **str 字段动态变体读**(1 错):`.__sse_status.Failed`(status 是裸串或
   {"Failed":msg})—— 需 any 通道或契约化。
6. **v-for 容器缺 `:key`**(R006,strict build 阻塞):codegen 只给 registry
   组件(如 Button)发 `:key`,div/row 容器的 v-for 不发。
7. **gen 模板缺口**:`auto gen` 重写 package.json 丢 `@vueuse/core`;Button.vue
   stub 的 `:class="class"` 保留字写法、无引用的 CodeEditor.vue 残留不清理 ——
   gen 后需三件手工补丁(pnpm add @vueuse/core / 修 Button stub / 删 CodeEditor)。

另:**表格列宽拖拽(Vue)延期** —— 需 vue.rs 按标记类注入拖拽脚本或生成物后
处理(Plan 059 §4.2;hover/吸顶已于 2026-08-24 以纯 CSS 类落地)。
