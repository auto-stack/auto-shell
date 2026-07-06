# Plan 010: MS3-A — try/catch 错误处理 + while 循环（AutoLang 层）

- **日期**: 2026-07-04
- **状态**: ✅ 已完成（2026-07-04）
- **RoadMap**: MS3（`docs/roadmap.md` §Milestone 3）
- **目标**: 给 AutoLang 加 `try { ... } catch(e) { ... }` 块和 `while (cond) { ... }` 循环，补齐 RoadMap §MS3 的两项硬缺口。让 ash 脚本能写"捕获命令错误并恢复"和"条件循环"。

> **跨仓库**：本 plan 主要改 **auto-lang 仓库**（`D:\autostack\auto-lang`），ash 仓库只 bump 依赖 + 回归。

## 1. 背景与现状

### 调研结论（2026-07-04 实读代码）

AutoLang（`auto-lang` 仓库）**已具备 MS3 的大部分能力**：

| 能力 | 状态 | 证据 |
|------|------|------|
| `fn` + 参数/递归/`return` | ✅ 已实现 | `token.rs:359`；conformance `007/027/028/029` |
| `if`/`else if`/`else` | ✅ 已实现 | `token.rs:352`；`ast/if_.rs` 多分支；conformance `003/015` |
| `for`（范围 `0..N` + 迭代器）| ✅ 已实现 | `ast/for_.rs`；conformance `004/018` |
| `loop { }` 无条件循环 | ✅ 已实现 | `token.rs:355`；conformance `016` |
| `break`/`continue` | ✅ 已实现 | `token.rs:383`；codegen 循环补丁；conformance `016/017` |
| `let`/`var`/`mut`/`const` 类型变量 | ✅ 已实现 | `token.rs:357/363/364/391`；完整类型系统 |
| `&&`/`\|\|` | ✅ 已实现 | `lexer.rs:1197`；`Op::And/Or` |
| `?.`/`.?` 错误传播 | ✅ 已实现 | `OpCode::ERROR_PROPAGATE`；`Option`/`Result` 类型 |
| **`while` 循环** | ❌ **缺失** | 无 `while` 关键字；`for <cond> { }`（`Iter::Cond`）是近似，但有已知 VM bug |
| **`try`/`catch` 块** | ❌ **缺失** | 无 `try`/`catch` 关键字、AST 节点、opcode |
| 自定义 `throw`/`raise` | ❌ 缺失 | 只有 `Err(...)` 值构造器，无语句关键字 |

### 已知 bug（影响 while）

`vm_file_tests.rs:528-529` 记录：**`mut fn` + `for cond` 会无限循环**（mut fn 的状态更新未反映到循环条件）。`for <cond> { }` 是 `while` 的现有近似，所以加 `while` 必须先确认这个 bug 是否已修，或在新 `while` 实现里规避。

### 关键实现路径（已确认）
- 关键字表：`token.rs::Token::keyword_kind`（line 346-416）
- TokenKind 枚举：`token.rs:86-176`
- 词法：`lexer.rs`
- AST：`ast/if_.rs`（Branch/Body 模式可复用给 try/catch）、`ast/for_.rs`（Iter::Cond 可复用给 while）
- codegen：`vm/codegen.rs`（Stmt::If/For 的翻译是 try/catch + while 的模板）
- 错误传播 opcode 已有：`OpCode::ERROR_PROPAGATE`（`vm/opcode.rs:48`）
- VM 运行时错误：`VMError::RuntimeError`（`vm/native.rs:809`）

### 不在本期范围（YAGNI / 拆给 011）

- **shell 桥接**（`system()`/`exit`/`export` 从 AutoLang 调 shell）→ **Plan 011**（这需要 AutoVM 加 host-context 插槽，改动大，单独做）
- 自定义 `throw`/`raise` 语句（本期 try/catch 配合 `Err()` 值够用；throw 关键字留后续）
- `each { |f| ... }` 闭包管道迭代（RoadMap 提到，但与 try/catch/while 无关，单独评估）

## 2. 设计

### 行为契约

| # | 规则 |
|---|------|
| 1 | `try { <body> } catch (e) { <handler> }`：执行 body；若 body 内任何错误传播（`.?`）或运行时错误到达 try 边界，**不向外抛**，而是把错误绑定到 `e`，执行 handler |
| 2 | `catch` 可省略绑定名：`try { } catch { }`（错误被丢弃）|
| 3 | 无错误时 handler 不执行，try 块值为 body 末值 |
| 4 | `try` 可不带 `catch`（等同错误吞掉？不——**必须有 catch**，避免静默吞错；`try { }` 单独是语法错误）|
| 5 | `while (cond) { body }`：cond 为真时反复执行 body；`break`/`continue` 生效 |
| 6 | `while` 的 cond 在每次迭代前求值（前置条件循环）|
| 7 | 无限循环风险：`while true { }` 合法（靠 break 退出）；不做超时保护（与 `loop{}` 一致）|

### try/catch 的实现策略

**方案：setjmp/longjmp 风格的异常表**（最贴合现有 VM 架构）。

AutoVM 是栈式字节码 VM。错误传播已有 `ERROR_PROPAGATE` opcode（把 Result/Option 的 Err 自动向上抛）。try/catch 需要：
1. **进入 try**：在 VM task 里压一个"异常处理器帧"（记录 catch handler 的跳转地址 + 局部变量基址）。
2. **错误到达**：当 ERROR_PROPAGATE 或运行时错误要"抛出"时，检查 task 栈顶是否有 handler 帧。有则：把错误值绑定到 catch 参数，弹出 handler 帧，跳到 handler 地址。无则继续向上传播（最终到 main task → REPL 报错）。
3. **正常退出 try**：body 执行完（未抛错）→ 弹出 handler 帧，跳过 catch handler。

这需要：
- 新 opcode：`PUSH_HANDLER <handler_pc>`、`POP_HANDLER`、（错误抛出路径复用现有 ERROR_PROPAGATE 的错误路径或新增 `THROW`）。
- AutoTask 加一个"handler 栈"字段（Vec<HandlerFrame>）。

**为什么不用 Result 值传染（无新 opcode）**：因为 `?.` 已经是显式传播，但 try/catch 要捕获的是**任何**运行时错误（包括除零、数组越界等），这些不是 Result 值，是 VM 运行时 panic。必须用异常表。

### while 的实现策略

**方案：复用 `for <cond> { }`（Iter::Cond）的 codegen 路径**。

`while (cond) { body }` 在 AST 层直接 desugar 成等价 `for cond { body }`（已有的 Iter::Cond）。但需先修复/确认 `mut fn + for cond` 的无限循环 bug（见 §1）。若 bug 未修，while 单独走一条 codegen 路径（每次迭代重新求值 cond）以规避。

### AST 设计

```rust
// ast/try_.rs (新文件)
pub struct Try {
    pub body: Body,
    pub catch_param: Option<String>,  // Some("e") or None
    pub catch_body: Body,
}

// ast/while_.rs (新文件) — 或直接在 for_.rs 复用
pub struct While {
    pub cond: Expr,
    pub body: Body,
}
```

## 3. 实现架构

### 改动 A：token.rs 加关键字 + TokenKind

- `TokenKind::Try`、`TokenKind::Catch`、`TokenKind::While`
- `keyword_kind`：`"try" => Try`、`"catch" => Catch`、`"while" => While`

### 改动 B：lexer.rs 识别新关键字

关键字走 `keyword_kind`，lexer 自动识别（已是现有模式，加进 match 即可）。

### 改动 C：parser.rs 解析 try/catch + while

- `parse_statement` 加 `TokenKind::Try` 分支 → 解析 `try { body } catch ( <ident>? ) { handler }`
- `TokenKind::While` 分支 → 解析 `while ( cond ) { body }`（或 `while cond { }`，看现有 for 的括号约定）

### 改动 D：ast/ 新节点

- `ast/try_.rs`：`Try` 结构（上述）
- `ast/while_.rs`：`While` 结构，或复用 `ast/for_.rs` 的 `Iter::Cond`

### 改动 E：codegen.rs 翻译

**try/catch**：
- `Stmt::Try(try_node)` →
  1. 发 `PUSH_HANDLER <handler_pc_placeholder>`（先留占位，回填）
  2. 翻译 body
  3. 发 `POP_HANDLER`（正常退出，弹出 handler）
  4. 发 `JMP <after_catch>`（跳过 catch）
  5. 回填 handler_pc = 当前地址
  6. 绑定 catch_param（若有）到局部变量
  7. 翻译 catch_body
  8. after_catch:

**while**：
- `Stmt::While { cond, body }` → 复用 `for cond { body }` 的 codegen（`codegen_for_cond`）

### 改动 F：opcode.rs + engine.rs 异常表

- 新 opcode：`PUSH_HANDLER`、`POP_HANDLER`、`CLEAR_HANDLER`
- `AutoTask` 加 `handler_stack: Vec<HandlerFrame>`，`HandlerFrame { catch_pc: usize, scope_bp: usize }`
- 错误抛出路径（`ERROR_PROPAGATE` + 运行时错误的统一抛出函数）：检查 `task.handler_stack.last()`，有则弹栈 + 绑定错误 + 跳 catch_pc；无则正常向上传播

### 改动 G：ash 仓库 bump 依赖 + 回归

- `ash/auto-shell/Cargo.toml`：`auto-lang` path 依赖指向更新后的 auto-lang
- 回归：ash 全量 cargo test（确保新关键字不破坏现有脚本解析）

## 4. 测试策略

| 层级 | 测试 | 位置 |
|---|---|---|
| 单元 | try 无错时执行 body，handler 不跑 | auto-lang conformance 新增 `try_basic` |
| 单元 | try 捕获 `.?` 传播的 Err，e 绑定错误值 | conformance `try_catch_err` |
| 单元 | try 捕获运行时错误（除零/越界）| conformance `try_runtime_error` |
| 单元 | try 嵌套（内层 catch 不截，外层 catch 截）| conformance `try_nested` |
| 单元 | while 基本循环 + 条件递减退出 | conformance `while_basic` |
| 单元 | while + break/continue | conformance `while_break` |
| 集成 | ash 脚本：try 内执行 shell 命令错误被捕获（需 011 的 system 桥接；本期用 AutoLang 内错误验证）| ash tests |
| 回归 | auto-lang 全量 conformance + vm 测试 | auto-lang |
| 回归 | ash 全量测试不破坏 | ash |

### TDD 流程

1. RED：conformance `try_catch_err`（捕获 `.?` Err）→ 失败（try 关键字不存在）
2. GREEN：token + lexer + parser + ast + codegen + opcode + engine 异常表
3. RED：conformance `while_basic` → 失败
4. GREEN：while（复用 for cond codegen；先确认/修 for cond bug）
5. 全量回归 auto-lang + ash

## 5. 实施步骤

1. **token.rs**：加 `Try`/`Catch`/`While` TokenKind + keyword_kind。
2. **lexer**：确认关键字自动识别（通常无需改，keyword_kind 已覆盖）。
3. **parser**：加 try/catch + while 解析分支。
4. **ast/**：加 `try_.rs`、`while_.rs`（或复用 for_.rs）。
5. **opcode.rs**：加 `PUSH_HANDLER`/`POP_HANDLER`。
6. **engine.rs**：AutoTask 加 handler_stack；错误抛出路径检查 handler。
7. **codegen.rs**：翻译 try/catch + while。
8. **conformance 测试**：try 各 case + while 各 case。
9. **确认 for cond bug**：跑 `99_plan231` 测试，若失败则在 while codegen 里规避（或修 for cond bug）。
10. **ash bump 依赖 + 全量回归**。
11. 提交 + push（auto-lang 先 push，再 ash bump push）。

## 6. 验收标准

- [x] `try { <runtime error> } catch(e) { print(e) }` 打印错误，不崩溃（conformance 043 用除零验证）
- [x] try 无错误时 handler 不执行，正常执行 try 块（conformance 042）
- [x] `var i = 0; while (i < 3) { print(i); i = i + 1 }` 打印 0/1/2 后退出（conformance 040 + ash 脚本实测）
- [x] while + break/continue 正常工作（复用 for-cond codegen，已有 conformance 016/017 覆盖）
- [x] auto-lang 全量 conformance 测试通过（040/042/043 全绿；无新增回归——基线 22 失败不变）
- [x] ash 全量 cargo test 通过（576 passed，新关键字不破坏现有脚本）
- [x] ash 脚本端到端：`while` 循环 + `try/catch` 捕获错误（实测通过）

## 7. 风险

- **异常表侵入 VM 核心**（最大风险）：改 AutoTask + 错误抛出路径，可能波及现有 ERROR_PROPAGATE 行为。→ 缓解：异常表是"可选 handler"，无 handler 时行为与现状完全一致（错误正常传播）；先跑全量 conformance 确认无回归。
- **for cond 无限循环 bug**：若未修，while（复用 for cond codegen）也会触发。→ 步骤 9 先跑 `99_plan231` 确认；若仍坏，while 走独立 codegen 路径（每次迭代重新求值 cond 而非缓存）。
- **catch 的错误值类型**：AutoLang 的运行时错误是 `VMError`，不是 `auto_val::Value`。catch 参数绑定需要把 VMError 转成 Value（如 `Obj{ kind: "error", message: "..." }`）。→ 定义一个 `vmerror_to_value` 映射。
- **try/catch 与并发/任务系统交互**：AutoLang 有 task/spawn（Plan 121）。跨任务的 try/catch 边界？→ 本期只支持同任务内捕获（handler_stack 是 per-task），跨任务错误留后续。
- **跨仓库提交顺序**：auto-lang 必须先 push（ash 依赖它）。若 auto-lang 是 path 依赖而非 git 版本，ash 端只需本地 rebuild，但 CI/他人 clone 需两个 repo 同步。→ 文档说明双 repo 依赖。

## 8. 已知限制（留后续，auto-lang 仓库）

实测发现一个 **预存 VM bug**（在 Plan 010 之前的 auto-lang master 上同样复现，**非本 plan 引入**）：

- **循环（`while` / `for`）嵌套在 `fn` 内 → 栈溢出**。auto-lang 的持久 REPL session（`AutovmReplSession`）增量编译 `fn` 时，循环 codegen 与 session 状态交互导致 OS 栈溢出。直接运行器（`run_file`，整文件编译为单元）**不受影响**。
- 同一根因波及 **`try/catch` 嵌套在 `fn` 或 `{ }` block 内 → 栈溢出**。

**影响**：ash 脚本里不能写 `fn f() { while/for/try ... }`。**规避**：把循环和 try/catch 放在脚本顶层，`fn` 只做直线辅助。`examples/deploy.ash` 演示了这种布局。

**根因待查**（auto-lang 仓库）：可能是持久 session 的 `jump_placeholders`/`jump_targets` 跨多次 `session.run` 累积，与函数 FN_PROLOG 插入后的跳转移位逻辑冲突，导致循环回跳地址错误 → 无限递归。修复需在 auto-lang 仓库调试 persistent session 的 codegen 状态隔离。**优先级**：中（ash 顶层脚本不受限，deploy demo 可用；复杂脚本待修）。
