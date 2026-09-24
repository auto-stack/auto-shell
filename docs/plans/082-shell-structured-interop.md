---
plan_id: PLAN-082
status: drafting
feature_name: ash 脚本与 `>` 命令的结构化互操作（for-in 消费命令记录 + `> {}` 命令块）
author: [agent]
created_at: 2026-09-24T00:00:00+08:00
updated_at: 2026-09-24T12:30:00+08:00
plan_revision: 2
current_step: 0
total_steps: 8
supersedes_spec_components: []
new_spec_components:
  - designs/039-shell-structured-interop.md
  - skills/ash-scripting/SKILL.md
touched_goals: []
---

# PLAN-082: ash 脚本与 `>` 命令的结构化互操作

## 0. 变更摘要

让 AutoScript(.ash) 以**结构化数据**消费 ash 命令结果：`for rec in > <管道> {}`
逐记录迭代，`var rows = > <管道>` 捕获记录数组（不再是 JSON 文本），循环体内
`> cmd2 $rec.field` 以记录字段为参数再调用命令，裸 `> cmd`（未消费）在函数体内
也打印（与顶层一致）；并新增 `> { A; B; C }` 多段命令块。技术上经
`ShellHost` 桥新增 `shell_query`（结构化）/`shell_run`（打印执行）两个
AutoLang native，脚本预处理层把 `>` 语法糖改写到它们。

## 1. 目标

1. `for rec in > <pipeline> { ... }`：rec 绑定为结构化记录（Obj），
   `.field` 字段访问、`.to_uint()` 等方法可用。
2. `var/let rows = > <pipeline>`：捕获 `Array<Obj>` 记录数组，
   `rows.len()`、`rows[i].field` 可用。
3. 循环体内 `> cmd2 $rec.field`：`$var` 插值扩展到字段路径（`$d.name`、
   `$u.path`），重写为 AutoLang 字符串拼接。
4. 语义统一：**未消费的 `>` 命令打印输出**（顶层与 fn/if/for 体内一致，
   修复当前体内静默丢弃的问题）。
5. `> { A; B; C }` 命令块：分号/换行分段，语句位置逐段执行打印；
   表达式位置（var/for 捕获）前段照常执行、取**末段**结构化结果。
6. 顺带修复 `du` 的两个输出缺陷：`-h` 短标志被帮助占用的 bug（阻塞人类
   可读示例），以及 **total 重复计数**（run() 把根行 `.`——已含全部内容——
   与子目录行再次相加，total 约为实际两倍；2026-09-24 实测
   `.`=16.08GB / 子目录和=16.08GB / total=32.17GB。该矛盾输出曾误导 AI
   模型判其损坏并反复重试，见 PLAN-083 背景）。

### 非目标

- **真·lazy 流式迭代**：ash 管道执行器当前整体物化
  （`Command::run_atom` 返回完整 `AtomPipeline`），逐记录拉取需重构执行器与
  VM 迭代协议。本期 `shell_query` 返回完整 `Value::Array`；native 契约设计为
  将来可替换为 Iter/Channel 返回而不改脚本语法（见 designs/039 的 future work）。
- `&&`/`||` 短路求值、VM parse-error 退出码为 0 的问题（另行立项）。
- REPL 交互态的 `>` 语法（本期只改脚本模式预处理层）。
- `system()` 文本桥语义（Plan 036 defect-A 的 bash-compat 决策保持不变）。

## 2. 架构方案

三层改动，自底向上：

```
AutoLang VM (auto-lang 仓)                ash (auto-shell 仓)
┌────────────────────────────┐           ┌─────────────────────────────────┐
│ ShellHost trait             │           │ 脚本预处理层 execute_script_     │
│   + query(cmd) -> Value     │◄──────────│ content (shell.rs ~2820)         │
│   + run(cmd)                │  实现      │   for x in > cmd   → shell_query │
│ native: 1804 shell_query    │           │   var x = > cmd    → shell_query │
│         1805 shell_run      │           │   裸 > cmd         → shell_run   │
│ codegen intrinsics 注册      │           │   > { A; B } 块收集/分段          │
└────────────────────────────┘           │   $var.field 插值重写增强         │
                                          │ ShellHostImpl::query → execute_  │
                                          │ query（拦截终段 Atom.value，      │
                                          │ 不走 format_output）              │
                                          └─────────────────────────────────┘
```

关键点：

- **结构化拦截点**：`execute_inner` 的管道链终段 `cmd.run_atom(...) -> AtomPipeline`
  在 `format_output`（表格/bash-compat/JSON 渲染）之前，`Atom.value` 就是
  `Value::Array<Obj>` 记录。`execute_query` 是"执行但不渲染、返回 Value"的
  兄弟路径，不新造数据模型。
- **兼容性**：`system()`/`execute_capture`（bash-compat 文本）完全不动；
  新增通道只服务 `>` 语法糖。`ShellHost::query/run` 提供默认实现
  （返回空数组/空操作），不破坏 auto-lang 仓内其他实现者与纯 AutoLang 场景。
- **跨仓**：auto-lang 改动落在其仓 master（junction
  `ash/ash/Cargo.toml -> ../../../auto-lang/crates/auto-lang`），沿用
  PLAN-081 的跨仓协作模式：auto-lang 先行合入，auto-shell 侧计划引用其提交。
- **预处理层统一**：顶层 `var x = > cmd` 从"解析期立即执行+文本转义内嵌"
  （`try_capture_assignment`）改为与体内一致的 VM 内求值（`shell_query`），
  消除两套语义。全仓盘点：examples/tests 中 `= > ` 存量用法为 0（2026-09-24
  实测），语义升级无仓内破坏面。

## 3. 技术栈

- Rust；auto-lang crate（VM native/`vm/native.rs`、`vm/codegen.rs` intrinsics、
  `vm/engine.rs` 注册、`host.rs` trait）与 auto-val（`Value/Obj/Array`）。
- auto-shell crate：`shell.rs`（预处理层、`ShellHostImpl`、`execute_query`）、
  `cmd/commands/du.rs`（-h 修复）、`cmd/parser.rs`（help 短标志优先级）。
- 验证：`ash <script>.ash` 实跑 + `echo $?`；auto-lang `cargo test`（host mock）；
  examples 冒烟。

## 4. 需求分析与背景调查

**授权**：用户 2026-09-24 会话明确要求——(a) for 循环访问 `>` 命令结构化结果
（"最好是 lazy 的、类 iter"——lazy 已按非目标处理并留演化接口，见 §10）；
(b) 对每个结果做简单处理后作为参数调用另一个 `>` 命令；(c) 评估
`> { a; b; c }` 多行命令结构（采纳为命令块）。示例由 agent 选定。
范围：auto-shell 仓 + auto-lang 仓（junction 依赖，PLAN-081 先例）。
无用户指定预算/自动延续限制。

**现状实锚（ash v0.1.0 debug 构建，2026-09-24 实测）**：

| 能力 | 现状 | 证据 |
|---|---|---|
| 顶层 `> cmd` | ✅ 执行+打印（含管道、表格渲染） | shell.rs:2867；probe 实测 |
| 体内 `> cmd` | ⚠️ 改写 `system()`，返回值**静默丢弃** | shell.rs:2895（Plan 034 Bug 3）；probe4 无输出 |
| `var x = > cmd` | ⚠️ 存在但捕获**JSON 文本**（221 字节字符串），`rows.name` 静默返 0 | shell.rs:3220 `try_capture_assignment`；probe2 实测 |
| `for x in > cmd` | ❌ parse error `Expected term, got Gt`，且 exit 0 | probe3 实测 |
| `$var` 插值 | ✅ 仅裸 `$name`，**不支持** `$d.name` 字段路径 | `rewrite_shell_cmd_to_system` shell.rs:3163 |
| 命令结构化输出 | ✅ `ls`→`{name type size modified}`、`du`→`{path size bytes}`，`to_json` 可导出 | `ash -c "du | to_json"` 实测 |
| VM 语言面 | ✅ `Expr::Dot`(obj.field)、`for x in <Call>`、`.to_uint()` 均为既有语法 | auto-lang `ast.rs:337`、`ast/for_.rs`、examples/csvsum.ash |
| native 注册路径 | `NATIVE_SHELL_SYSTEM=1800..1803` 已占；下一个 **1804** | auto-lang `vm/native.rs:588`、`codegen.rs:527`、`engine.rs:670` |
| du `-h` | ❌ 短标志被 help 占用，打印帮助 | 实测 `du -h` → help；`--human-readable` 正常 |
| 存量兼容面 | examples/tests 中 `= > ` 用法 **0 处** | grep 实测 2026-09-24 |

**规范载体**：本仓无 `docs/specs/` 体系（PLAN-081 判例：designs/ + 技能文档
为契约载体）。脚本模式的操作性知识在 `skills/ash-scripting/SKILL.md`（其中
"`> cmd` 不支持管道"的说法已过时——顶层实测支持，本计划一并修正）。

## 5. 详细设计

### 5.1 auto-lang 侧（其仓 master）

1. `host.rs` — `ShellHost` 增加两个带默认实现的方法：
   - `fn query(&self, cmd: &str) -> auto_val::Value`：执行管道，返回终段
     结构化值（默认 `Value::Array` 空）。
   - `fn run(&self, cmd: &str)`：执行命令并走 shell 自身输出（默认空操作）。
2. `vm/native.rs` — `NATIVE_SHELL_QUERY: u16 = 1804`、`NATIVE_SHELL_RUN: u16 = 1805`
   与 shim：`shim_shell_query`（pop string → host.query → **压栈 Value**）、
   `shim_shell_run`（pop string → host.run → 压栈空值）。压栈 Value 的构造
   路径（heap/rc）由 T-01 spike 确认参照实现。
3. `vm/codegen.rs` intrinsics 表注册 `shell_query` / `shell_run`；
   `vm/engine.rs:670` 处注册两个 shim。
4. 单测：mock host 返回固定 `Array<Obj>`，断言 `for r in shell_query("x")`
   迭代次数与 `r.f` 取值；`shell_run` 透传字符串到 mock。

### 5.2 auto-shell 侧

1. `host.rs` — `ShellHostImpl` 实现 `query`/`run`：
   - `query`：`shell.execute_query(cmd)`（新），`$1`/`$@` 位置参数插值沿用
     `execute_capture` 的 Plan 034 Bug 2 先例。
   - `run`：等价顶层 `> cmd` 路径——`execute` + `print_or_emit`。
2. `shell.rs` — 新增 `execute_query(&mut self, input) -> Result<Value>`：
   复用 `execute_inner` 管道链（env 前缀、链解析、每段 `run_atom` 传递
   `AtomPipeline`），终段**不调 `format_output`**，直接取 `Atom.value`。
   单命令/管道均适用；外部命令（非 registry）回退为其 stdout 文本按行拆分的
   `{line}` 记录或单字符串（T-03 定案，倾向按行记录）。
3. 预处理层（`execute_script_content` 主循环）：
   - 新形态：行匹配 `for <pat> in > <cmd> {`（含 `for (k, v) in > ...`）→
     改写 `for <pat> in shell_query(<expr>) {`；`<expr>` 由
     `rewrite_shell_cmd_to_system` 生成（拼接 + 转义）。
   - `var/let x = > cmd`（顶层与体内统一）→ `var x = shell_query(<expr>)`；
     删除顶层 `try_capture_assignment` 的立即执行路径（函数保留给过渡期或
     直接移除，T-04 定案）。
   - 裸 `> cmd`（顶层与体内统一）→ `shell_run(<expr>)`。
   - `rewrite_shell_cmd_to_system` 增强：`$name(.field)+` 词法——
     `> du $d.name` → `system/shell_query("du " + d.name)`。
   - `> { ... }` 块：`> {` 起、匹配 `}` 止（支持多行与 `;` 分段）；
     语句位置逐段生成 `shell_run(<段>)`；捕获位置（var/for 的 cmd 是块时）
     前段 `shell_run`、末段 `shell_query`。块内允许管道；不允许嵌套块（首期）。
4. `cmd/parser.rs` — help 短标志冲突修复：命令自注册的短标志（如 du 的
   `-h`→human-readable）优先于自动 help（help 保留 `--help`；如需短标志
   fallback 用 `-?`，T-05 实测定案，原则：不再吃掉命令自己的短标志）。

### 5.3 用户可见语法（成稿进 designs/039 与 SKILL.md）

```auto
// 1) for 迭代结构化记录
for d in > ls | where type == dir | select name {
    print(d.name)
}

// 2) 捕获记录数组 + 字段/方法处理
var rows = > du | where path != total | where path != .
for r in rows {
    if r.bytes.to_uint() > 1048576 { print(r.path + " over 1MB: " + r.bytes) }
}

// 3) 处理后作为参数再调用另一个 > 命令（未消费 → 打印）
for d in > ls | where type == dir | select name {
    > du --human-readable $d.name | where path != total | select path size | to_json
}

// 4) 多段命令块：语句位逐段打印；捕获位取末段记录
> { cd ../examples; ls | where type == dir | select name }
var t = > { cd $dir; du | where path == total }   // t = [{path:"total",...}]
```

### 5.4 验收示例（agent 选定，覆盖全部 AC）

- **示例 A（改造 examples/du-top）**：`for d in > ls | where type == dir`
  逐目录 `> du --human-readable $d.name | where path != total | select path size`
  ——同时覆盖 AC-01/03/04 与 du -h 修复（AC-07）。
- **示例 B（新增 examples/dirdu-alert 或并入 A）**：`var rows = > du | ...`
  捕获 + `r.bytes.to_uint()` 阈值告警——覆盖 AC-02。
- **示例 C（多行块演示）**：prepare-then-query 形态——覆盖 AC-05。

### 规范增量

| delta_id | add/modify/retire | docs/specs/... target | before/after rule | rationale | acceptance IDs |
|---|---|---|---|---|---|
| SD-01 | add | designs/039-shell-structured-interop.md | 无 → `>` 语法五形态契约（裸语句/var 捕获/for 捕获/$var.field 插值/`> {}` 块）+ shell_query/run native 契约 + lazy 演进预留 | 本仓无 docs/specs 体系，designs 为契约载体（PLAN-081 判例） | AC-01..05 |
| SD-02 | modify | skills/ash-scripting/SKILL.md | "互操作仅 system() 文本；`>` 不支持管道" → 结构化互操作章节 + 纠正过时说法 + 新语法速查 | 技能是脚模式操作性知识的权威入口 | AC-08 |
| SD-03 | modify | docs/bash-to-ash.md | 无 for-over-command 对照 → 增补（bash `for f in $(ls)` vs ash `for f in > ls`） | bash 用户迁移对照 | AC-08 |

## 6. 测试设计

- **auto-lang 单测**：shell_query/run shim 的 mock-host 行为（含 host=None
  默认路径）；intrinsics 注册可编译调用。
- **auto-shell 集成**（tests/ 或脚本冒烟）：
  - for-in 记录数与字段值断言（对照 `ash -c "...| to_json"` 同目录实跑）；
  - 体内裸 `>` 打印断言（probe4 场景回归）；
  - `> {}` 语句/捕获两种位置；
  - `$d.name` 插值（含路径含空格的目录名）；
  - `du -h` 不再打印帮助、输出带单位。
- **存量回归**：examples 全量冒烟（重点：此前依赖 `> cmd` 体内改写 system()
  的脚本——语义从静默变打印，需逐个过目输出变化）。

## 7. 验收标准

- **AC-01** `for rec in > <pipeline> { }` 迭代结构化记录，`rec.field` 取值。
  验证：示例 A 在真实目录实跑，输出与等价 `ash -c` 管道逐行一致。
- **AC-02** `var rows = > <pipeline>` 得 `Array<Obj>`：`rows.len()`=记录数、
  `rows[i].field`/`r.bytes.to_uint()` 可用。验证：示例 B 阈值告警输出正确。
- **AC-03** 循环体内 `> cmd2 $rec.field` 字段插值执行且输出打印；其结果可再被
  for/var 消费。验证：示例 A 嵌套形态 + 含空格目录名用例。
- **AC-04** 未消费的 `>` 命令在 fn 体内打印（与顶层同路径渲染），不再静默。
  验证：probe4 回归（fn 内 `> ls` 有表格输出）。
- **AC-05** `> { A; B; C }`：语句位逐段执行打印；捕获位返回末段记录数组、
  前段已执行（如 cd 生效）。验证：示例 C。
- **AC-06** 兼容：`system()` 语义不变；examples 冒烟全绿；体内 `> cmd` 从静默
  变打印的输出差异逐脚本过目并记录。验证：冒烟脚本 + 人工核对清单。
- **AC-07** `du -h` 输出人类可读大小而非帮助文本；`--help` 仍出帮助。
- **AC-09**（rev2）du 输出自洽：`total` 行 bytes 等于根行 `.` 的 bytes（=
  直接子项之和 + 根下直接文件），不再对子目录行重复累加。验证：真实目录
  实跑 `ash -c "du | to_json"` 断言 total == "." 行 bytes，且 total ≈ 各
  子目录 bytes 之和 + 根下文件（不再 ≈ 2×）。
- **AC-08** designs/039、SKILL.md、bash-to-ash.md 更新合入，SKILL.md 无与实测
  相悖的陈述（含删除"`>` 不支持管道"过时条目）。

## 8. 执行步骤

- **T-01**（spike，跨仓）验证 VM 数据面三个前提：host shim 能把
  `Value::Array<Obj>` 压栈供 for-in 迭代；`Index`+`Dot`（`rows[0].name`）
  与 `.to_uint()` 在 host 构造的动态记录上可用。产出：探针脚本 + 结论记入
  designs/039 草稿。任一不成立 → 触发 revision（VM 侧补迭代/字段运行时）。
  验证：探针脚本实跑输出。→ AC-01/02 前置
- **T-02**（auto-lang 仓）ShellHost `query/run` + native 1804/1805 + codegen/
  engine 注册 + mock 单测。验证：`cargo test -p auto-lang` 相关用例绿。
  → AC-01/02/04 底座
- **T-03**（auto-shell）`ShellHostImpl::query/run` + `execute_query`（终段
  Atom.value 拦截；外部命令回退策略定案并记录）。验证：临时探针
  `var x = shell_query("ls | to_json 化前的原始管道")` 直接调用断言。
  → AC-01/02
- **T-04**（auto-shell）预处理层五形态改写 + `$var.field` 插值增强 + 移除
  `try_capture_assignment` 旧路径。验证：AC-01..05 各自探针脚本。
  → AC-01..05
- **T-05**（auto-shell）du 输出修复：help 短标志冲突（`-h`）+ total 重复
  计数（run() 的 total 求和改为以根行为准，排除子目录行重复累加；单测覆盖
  多层目录与空目录）。验证：`ash -c "du -h"`、`ash -c "du --help"`、
  `ash -c "du | to_json"` 断言 total == `.` 行。→ AC-07、AC-09
- **T-06** 示例落库：改造 examples/du-top + 新增示例 B/C + README。
  验证：三示例实跑 + `echo $?`。→ AC-01..05 载体
- **T-07** 文档：designs/039 成稿 + SKILL.md + bash-to-ash.md。
  → AC-08
- **T-08** 回归：examples 全量冒烟 + 输出差异清单（AC-06）+ auto-lang
  `cargo test`。→ AC-06

依赖：T-01 → T-02 → T-03 → T-04 → T-06 → T-07/T-08；T-05 独立可并行。

## 9. 复审记录

- 2026-09-24 stage: new, plan_revision: 1 — 起草 handoff：背景调查含 2026-09-24
  实测锚点（probe 见会话记录）；outcome: **pass**（用户已授权范围：结构化
  for-in + 参数互调 + `> {}` 块；lazy 降级为非目标并留接口，见 §10-Q1）；
  next: **work**（/auto-plan:work 按 T-01 起步）。
- 2026-09-24 plan_revision: 2 — 修订：T-05 从"du -h 短标志修复"扩展为
  "du 输出修复"（并入 total 重复计数 bug，新增 AC-09）。触发源：AI 模式
  du 工具调用失败问题的同日诊断（PLAN-083 §4）——实测发现 du 的 total 约
  为实际两倍，是模型判定命令损坏的诱因之一。目标/验收阈值未变，无既有
  进度作废（status 仍 drafting，未开步）。

## 10. 待澄清事项

- **Q1 lazy 迭代**：本期 eager `Array`（ash 执行器整体物化的现实约束），
  `shell_query` 的 native 契约留 Iter 演进位。若坚持本期 lazy → 需新增执行器
  流式化设计任务并提升 total_steps。owner: 用户；next: work 前拍板，默认按
  非目标执行。
- **Q2 顶层 `var x = > cmd` 语义升级**（文本→结构化）为授权内 breaking change
  （用户 2026-09-24 明确"应该当作结构化数据"）；仓内存量用法 0。owner: 无阻塞，
  T-04 直接执行；若 auto-shell 仓外有下游脚本依赖文本形态，work 期发现再回报。
- **Q3 跨仓合入节奏**：auto-lang 改动需先落其仓 master 再联动本仓 junction。
  owner: work 阶段执行者；next: T-02 提交时在计划中登记 auto-lang commit id
  （PLAN-081 判例）。
