# 039 — 脚本与 `>` 命令的结构化互操作

- **Plan**: PLAN-082（docs/plans/082-shell-structured-interop.md）
- **状态**: 已实现（auto-lang plan-082-dev d5890d64b + auto-shell plan-082-dev）
- **变更范围**: AutoScript(.ash) 以结构化数据消费 ash 命令结果

## 1. 问题

ash v0.1.0 里 AutoLang 脚本与 ash 命令的互操作边界只有一个 String：

| 能力 | v0.1.0 现状 |
|---|---|
| 顶层 `> cmd` | 执行+打印（含管道、表格渲染）✅ |
| 体内 `> cmd` | 改写 `system()`，返回值**静默丢弃** ❌ |
| `var x = > cmd` | 立即执行并把 **JSON 文本**内嵌进脚本；`rows.name` 静默返 0 ❌ |
| `for x in > cmd` | 解析错误 `Expected term, got Gt`，且退出码 0 ❌ |
| `$var` 插值 | 仅裸 `$name`，不支持 `$d.name` 字段路径 ❌ |
| `> { ... }` | 无此语法 ❌ |

## 2. 契约：`>` 语法五形态

脚本预处理层（`execute_script_content`）把 `>` 语法糖改写为 AutoLang
native 调用 `shell_query` / `shell_run`，在 VM 内求值。**顶层与 fn/if/for
体内语义统一**。

### 2.1 裸 `> cmd`（任意深度）→ `shell_run(<expr>)`

执行并经 shell 自身输出路径打印（表格/文本渲染在 shell 侧）。体内命令
不再静默丢弃。顶层命令不再打断（flush）AutoLang 块——语句顺序在同一
VM 程序内保持。`$1`/`$@` 位置参数在执行期插值（`execute_capture` 先例）。

### 2.2 `var/let x = > <管道>` → `var x = shell_query(<expr>)`

捕获**结构化记录数组**（`Value::Array` of `Value::Obj`）：

- 注册命令（ls/du/…）：记录即命令的结构化行（`ls` → `{name type size
  modified}`，`du` → `{path size bytes}`）
- 外部命令：stdout 按行拆分为 Str 数组（每行一个元素）
- 纯文本（Text 管道）：单个字符串
- 失败/空：空数组 + 错误打印

`rows.len()`、`rows[i].field`、`r.field.to_uint()` 可用。**语义升级**
（v0.1.0 为 JSON 文本捕获）为授权 breaking change——仓内存量用法实测为 0
（2026-09-24）。

### 2.3 `for rec in > <管道> { ... }` → `for rec in shell_query(<expr>) { ... }`

逐记录迭代，`rec` 绑定为 Obj。支持 `for (k, v) in > ...` 形态的 pattern。
管道需在 `{` 前结束（首期不支持循环头内换行）。

### 2.4 `$var.field.path` 插值（增强）

`> cmd2 $rec.field` 重写为 `"cmd2 " + rec.field` 字符串拼接。字段路径可
多级（`$u.path.size`）。与裸 `$var`、字面量混合：`> du -h $dir/$d.name`。

### 2.5 `> { A; B; C }` 命令块

分号/换行分段；**首期不支持嵌套块**（首个不平衡 `}` 闭合）。

- **语句位置**（裸 `> { ... }`）：每段执行并打印
- **捕获位置**（`var t = > { ... }` / `for x in > { ... } {`）：前段执行
  （如 `cd` 生效），取**末段**的结构化记录

for 形态的循环体 `{` 可紧跟块闭合 `}`（同一行）或独立成行。

### 2.6 明确不支持（本期）

- `&&`/`||` 逻辑链出现在查询管道中（`execute_query` 明确报错）
- 嵌套 `> { { } }`
- REPL 交互态的 `>` 语法（仅脚本模式预处理层）

## 3. native 契约（auto-lang 侧）

```
1800..1803  system/system_status/export/exit（既有）
1804  shell_query(cmd: String) -> 记录   // ShellHost::query
1805  shell_run(cmd: String) -> null     // ShellHost::run
```

- `ShellHost::query(&self, cmd) -> auto_val::Value`（默认空数组）；
  `run(&self, cmd)`（默认空操作）——纯 AutoLang host 不受影响
- **数据面（T-01 spike 结论，三项全部成立）**：
  1. shim 把 `Value` 树物化为堆对象：Obj→`ObjectData`、Array→
     `ListData<Value>`、嵌套容器为 `Value::VmRef` 元素；**根引用按裸堆
     id 入栈（`rc_push_id`）**——TAG_OBJECT 编码仅部分消费者兼容
     （Plan 437 判例），`shim_list_len` 等按 `pop_arg_i32` 解码
  2. `Index`+`Dot` 链（`rows[0].name`）在动态记录上可用
  3. `.to_uint()` 等方法在记录字段（Str）上可用
- **类型跟踪**：codegen 把 `shell_query` 调用结果标为 `ObjectType::Array`
  ——否则 var 存储后 `.len()` 按 String 类型分派到 `str.len`，把裸堆 id
  字符串化后取长度（spike 实测 len=7，即 "4194307" 的长度）
- **lazy 演进位**：`query` 返回整个 `Value`（eager 物化，ash 执行器整体
  物化的现实约束）；将来可替换为 Iter/Channel 流式载荷而不改脚本语法
  （本期非目标，PLAN-082 §10-Q1）

## 4. ash 侧实现锚点

- `execute_pipeline_with_auto` 核心抽为 `execute_pipeline_to_atom`
  （返回终段 `AtomPipeline`）；`execute_query` 在其上经 `into_dsl_input()`
  取结构化值——与 DSL 阶段同一数据路径，不新造数据模型
- `execute_host_run`：`shell_run` 的 host 臂（插值 + 执行 + `print_or_emit`
  + 错误闩锁 `script_had_error`）
- 预处理层五形态见 §2；`ASH_DUMP_BLOCK=1` 可 dump 改写后的 auto_block

## 5. 同期修复（du 输出）

- **`-h` 短标志**：自动 help 让位命令自声明短标志（du/ls 的
  human-readable）；`--help` 恒有效；`-?` 通用兜底
- **total 重复计数**：total 恒等于根行 `.` 的 bytes（旧实现对根行与子目录
  行求和 ≈2× 实际）。单测：多层/空目录/summarize 三态
- 附带发现（未修）：`du --depth/-d` 以 `flag_with_short` 声明但按 named
  读取，`-d 2` 会把 `2` 当 positional

## 6. 回归面

- `system()` / `execute_capture`（bash-compat 文本桥）语义不变
- 体内裸 `> cmd` 从静默变**打印**、顶层 `var x = > cmd` 从文本变**结构化**
  ——两处输出/语义差异按 PLAN-082 AC-06 逐例过目（examples 冒烟全绿）
- auto-lang 全量测试的 260 个失败为预存/环境性基线（ui::* 231 个 +
  musk_vm_track 等；pristine main 抽查同样失败；p656_scroll 为顺序抖动，
  单独运行双检出均绿），本计划改动零新增失败
