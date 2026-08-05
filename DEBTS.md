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

---

## auto-lang parser:store/widget 语法限制(2026-08-05 实测)

> 以下限制用**含 fix043 修复的 debug 二进制**(2026-08-05 15:51 构建)实测复现,
> 全部以最小复现文件验证过。015-notes 均无对应先例,因此不是我们的语法问题。
> 处理原则:不做规避(不改写语义),记录并等 parser/codegen 修复。

### `msg` 消息多参数声明不被支持

**来源**:Plan 043 M2(shell_store.at)。

**现状**:`msg Msg { Complete(str, int) }`、`RunSmart(int, str, []str)` 等任何
**2 个及以上参数**的消息声明都报 "Expected term, got RBrace"(参数列表未正确消费,
在 `}` 处失败)。单参数消息(含 `[]str` 数组参数、自定义类型参数如 `CommandResult`)
均正常。015-notes 的所有 `msg` 全是 0/1 参数,无多参数先例。

**根因**:疑似 `msg` 参数列表解析只消费单个参数类型后没有继续循环
(parser.rs 的 msg 声明分支),需 auto-lang 侧确认。

**影响**:`shell_store.at` 的 `RunResult(CommandResult)` 等是单参数没问题,但
`Complete(str, int)`、`RunSmart(int, str, []str)` 无法声明 → shell_store.at 整体编译失败。

**推翻条件**:auto-lang parser 支持 msg 多参数。

### computed 只支持单表达式,不支持多行 body

**来源**:Plan 043 M2(shell_store.at 的 `history`/`git_label`)。

**现状**:`computed { history => .persisted_history }`(单表达式)正常;任何
`computed { name => { ...多条语句... return ... } }` 的多行 body 形式都报
"Expected term, got RBrace"。015-notes 的 computed(`pinned_notes => .notes.filter(...)`)
全是单表达式。

**根因**:computed 的 `=>` 右侧复用表达式解析路径,不进入语句块(parse_body)解析。

**影响**:`history`(需要 concat + for 循环拼接)和 `git_label`(多分支格式化)无法
用多行 body 表达。之前尝试把两者改成 `=> self.persisted_history` 和 `=> ""` 的桩实现
**已撤销**(那是规避,导致行为丢失),恢复为完整逻辑——当前 shell_store.at 会因此编译失败,
等 parser 支持 computed 多行 body。

**推翻条件**:auto-lang parser 支持 computed 多行 body(或引入 computed 帮助函数)。

### view 条件里 `None` 比较不被支持(handler/computed 里正常)

**来源**:Plan 043 M4(block_item.at)。

**现状**:`if .block.output != None { ... }`(**view 树**的 if 条件)报
"Expected term, got RBrace"。但**handler 里** `if s != None`(015-notes notes_store.at:120
同款)和 **computed 单表达式** `.git_status != None` 均正常。

**根因**:疑似 view 的 if 条件解析路径(expr_pratt 在 view 上下文)没有处理
`None` 字面量或比较链,而 handler/computed 走的语句表达式路径正常。

**影响**:`block_item.at` 的 `if .block.output != None`(决定是否显示 BlockBody)无法表达。

**推翻条件**:auto-lang parser 在 view if 条件里支持 `!= None`(或 `is Some` 形式)。

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

**现状**:015-notes 的 view fn(`NoteItem`)是**定义与使用同文件**,codegen 将其内联展开
到调用处。我们的 `renderers.at` 把 4 个 view fn 独立成文件,`block_body.at` 用
`use renderers: render_table` 跨文件引用——parser 接受,但 codegen 把调用生成成
`<div :output="output" />`(当作未知组件 + 属性),没有内联展开,行为错误。
另外纯 view fn 文件编译时报 "No widget or store declarations found"(警告,不阻塞,
因为作为模块被 use 引用时可解析)。

**影响**:renderers 独立成文件时,生成的 Vue 渲染器全部失效。

**临时处理**:把 view fn 从 `renderers.at` 移入使用它的 `block_body.at`(同文件定义+
调用,与 015-notes 一致)——这是**结构修正**不是规避,尚未应用,见计划 M4 收尾。

**推翻条件**:auto-lang codegen 支持跨文件 view fn 引用(或统一同文件定义)。
