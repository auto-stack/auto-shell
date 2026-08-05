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

### `#[api]` 注解 + `store` 声明解析导致 stack overflow

**来源**:Plan 043 M1(API 层)+ M2(Store 层)。

**现状**:`Parser::parse()` 解析含 `#[api(...)]` 注解或 `store Name { ... }` 声明的
源码时,触发无限递归 → stack overflow。**015-notes 自己的 `api.at` 和 `notes_store.at`
也会 overflow**,所以这是 pre-existing bug,不是我们的语法问题。两个声明都会触发,
很可能是同一条递归路径(属性/声明解析的某个分支)。

**根因**:`#[api]` 属性解析路径(parser.rs `TokenKind::Hash` 分支)在某个递归调用中
没有正确终止。需要用 `RUST_MIN_STACK` 或 gdb 调试定位具体递归点。

**影响**:Plan 043 的 API 层 `.at` 源码无法被 parser 验证。codegen 无法运行。

**临时绕过**:先写 `.at` 源码(格式已确认与 015-notes 一致),等 parser 修复后批量验证。

**推翻条件**:auto-lang parser 修复 `#[api]` 解析。

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
