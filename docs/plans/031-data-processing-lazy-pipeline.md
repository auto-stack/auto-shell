# Plan 031: ASH 数据处理框架（lazy pipeline + Format 统一）— 详细 TDD 计划

> **日期**: 2026-07-30
> **分支**: `feat/031-lazy-pipeline`
> **状态**: 实施中
> **来源设计**: [`designs/031-data-processing.md`](../../designs/031-data-processing.md)
> **预估**: M0-M2 约 4-5 周；M3 可选约 1 周
> **回归基线（M0.0，2026-07-30）**: ash-core 352 + auto-shell 755 ≈ **1107 个测试，全绿**。`legacy_json_compat.rs`（028/007 信封回归，5 个）✅ 全绿。

---

## 0. 实施前必读：设计文档 vs 当前代码的偏差（已逐条核实）

设计文档（2026-07-21）写的若干「探勘事实」已随代码演进而过时。**本 Plan 以当前 main 代码为准**：

| # | 设计文档原文 | 实际代码（已核实） | 影响 |
|---|---|---|---|
| 1 | Stream bug 在 `shell.rs:987` | 实际在 **`shell.rs:1128`**（`execute_pipeline_with_auto` DSL 分支的 `_ => Value::Array(Array::new())`） | M0 测试锚点 |
| 2 | `find ... \| filter` 能验收 Stream bug | ❌ **不能**。`find` 是注册 builtin（`find.rs:93` 返回 `Atom::file_list`），走 `shell.rs:1124` 的 `Atom` 分支，不丢数据。**必须用真外部命令**（产 `ExternalStream` 的）验收 | M0 验收用例 |
| 3 | `PipelineOp` 16 个算子 | ✅ 属实（已 grep 确认 16 个） | — |
| 4 | 成功指标「现有 676 测试全过」 | ❌ 当前 **1107 个 test fn**（352 ash-core + 755 auto-shell） | 回归基线已重统 |
| 5 | `get_field` 已存在（风险缓解用） | ✅ 存在，但**私有** `operators.rs:300 fn get_field`（非 pub）。LazyNode 复用须提为 `pub` | M1 前置任务 |
| 6 | `AggOp` enum | ⬜ 尚不存在，需新建 | M1 新建 |
| 7 | `AtomPipeline::Stream` 零生产者 | ⚠️ 基本属实——无 DSL 命令产 Stream；但 `from_stream()` 被 batom decode（`batom.rs:642`）+ bench 用。措辞要精确为「无命令产生 Stream」 | — |
| 8 | 设计文档说「跑 028 回归测试」 | ⚠️ Plan 028 的结构化信封 + 测试已在 commit `30bca75` 整体删除，委托给外部 `auto-ai` 仓库。**本仓库现存的「信封」= Plan 007 的 `pipeline_to_json`（`shell.rs:971`）**，回归测试 = `legacy_json_compat.rs`（5 个） | 回归守这 5 个 |

### 路径修正
设计文档假设的 `crates/ash-core/`、`crates/auto-shell/`——**均不存在**。实际：
- 核心库：`ash-core/`（crate `ash-core`），pipeline 是**目录** `ash-core/src/pipeline/`（7 文件，非单 `pipeline.rs`）
- 主 shell：`ash/auto-shell/`（crate `auto-shell`，二进制 crate），`shell.rs` 在此（5331 行）
- 管道数据类型是 **`auto_val::Value`**（来自 `auto-lang/crates/auto-val`），**不是 `auto_lang::Value`**

---

## 1. 目标与范围

把 ash pipeline 从「每段全量 eager materialize」升级为「算子是 `Iterator`、逐行 pull、filter/take/select 流式、sort/aggregate 末端断流 collect」，同时：
- **M0**：修一个静默丢数据 bug（`shell.rs:1128`），抽 `Format` trait 统一 10 个格式转换器
- **M1**：新建 `LazyNode` + `impl Iterator`
- **M2**：谓词下推 pass + shell.rs 累积 DSL 段改造（lazy 真正产生价值的步骤）
- **M3（可选）**：ExternalStream → 真流式 `StreamSource`

**范围外**（明确不做）：Polars、批处理 RecordBatch、投影下推、算子融合、完整查询计划优化器、删除现有 10 个格式命令、lazy 的 explain 可视化。

### 测试约定
- **ash-core**：内联 `#[cfg(test)] mod tests`，**无** `tests/` 目录
- **auto-shell**：`ash/auto-shell/tests/` 集成测试（每文件一个二进制）
- **TDD 纪律**：每个任务先写失败测试，再写实现，最后回归全绿

---

## M0：Stream bug 修复 + Format trait 统一（1-2 周，~600 行）

### 任务 M0.0 — 建立回归基线（前置，无代码改动） ✅ 完成（2026-07-30）
- ash-core 352 + auto-shell 755 ≈ **1107 个测试，全绿**。
- `legacy_json_compat.rs`（028/007 信封回归，5 个）✅ 全绿。

---

### 任务 M0.1 — 修复 Stream bug（TDD，先写失败测试）

**引用**：设计文档 §4。bug 精确位置 `shell.rs:1128`（非 987）。

**根因**：DSL 分支的 match 只处理 `Atom` 和 `Text`，`Stream`/`ExternalStream`/`Empty`/`None` 全落到 `_ => Value::Array(Array::new())`，对 filter/sort 来说 = 静默零结果、数据丢失无提示。

**验收用例必须用真外部命令**（`find` 不行，它是 builtin 产 Atom）。

- [x] **先写失败测试**：
  - 单元测试 `ash-core/src/pipeline/atom_pipeline.rs` 的 `dsl_input_*`（4 个，用跨平台 `sort` 造 ExternalStream）—— 先编译失败（方法不存在）→ 实现后绿。
  - 端到端测试 `ash/auto-shell/tests/stream_bug_fix.rs`（2 个，`cfg(unix)` printf / `cfg(windows)` cmd，验证 `外部命令 | count/uniq` 不丢行）—— 临时回退 shell.rs 修复确认两测试 RED，恢复后 GREEN。
- [x] **实现修复**：新增 `AtomPipeline::into_dsl_input(self) -> Value`（`atom_pipeline.rs`）—— `Atom→value`、`Stream→items Array`、**`ExternalStream→按行 split 成 Array`**（核心）、`Text→str`、`Empty→空 Array`。`shell.rs:1122-1138` DSL 分支改为调用它（替换原只处理 Atom/Text 的内联 match）。
- [x] **守护 `pipeline_to_json`**（`shell.rs:971`）：`legacy_json_compat.rs` 5 个全绿——修复后 ExternalStream 走 `into_dsl_input`（不进 `pipeline_to_json` 的 other 分支），信封路径不受影响。
- [x] 回归：ash-core 356（+4）+ auto-shell 757（+2）全绿；`legacy_json_compat.rs` 全绿。

**实现偏差（vs 设计 §4.2）**：设计说「ExternalStream → read_all → 按行成 Array」，与实际实现一致。但实现落点从「shell.rs 内联 match」改为「抽成 ash-core 的 `into_dsl_input` 方法」——更可测、DRY，且 shell.rs 只剩单行调用。复现脚本验证：`printf '...' | count` 修复前 `0` → 修复后 `3`。

**验收**：外部命令 → DSL 段管道不再丢数据；新测试绿；028/007 回归不破。

---

### 任务 M0.2 — `Format` trait + 5 实现 + 注册表

**引用**：设计文档 §5.1-5.3。**放 `ash-core/src/format.rs`**（新增）。

- [x] **先写失败测试** `ash/auto-shell/src/cmd/format.rs` 内联 `mod tests`：json/toml/yaml/xml/csv roundtrip + registry 查询 + 错误传播（11 个）。
- [x] **实现 trait**（设计 §5.1 签名）：`Format: Send + Sync`（`name/parse/serialize`）+ `FormatError`。
- [x] **5 个实现**：`JsonFormat`/`CsvFormat`/`YamlFormat`/`XmlFormat`/`TomlFormat`，复用现有 free function（`parse_json`/`value_to_json` 等）。
- [x] **设计留白决策（config 收敛）**：`Format::serialize` 用各格式默认 config（json pretty=2、csv delimiter="," include_header=true、xml root="root" indent=2、yaml indent=0、toml path=&[] depth=0）——已核实各命令的实际默认值一致。
- [x] **`FormatRegistry`**：`HashMap<String, Arc<dyn Format>>`，`new()` 注册 5 个，`get(&str)` + `names()`。

**落点偏离（vs 设计 §5.1）**：设计说放 `ash-core/src/format.rs`，实际放 `ash/auto-shell/src/cmd/format.rs`。理由：实现复用的 5 个解析器（含手写 csv/yaml/xml + `toml` crate）全在 auto-shell；把它们搬进 ash-core 需迁移 ~1900 行 + 引入 `toml` 依赖，超出 M0 范围。trait 仍是纯 `auto_val::Value` 接口；ash-core 的 lazy 层不需要它（Format 在 lazy 链两端，调用点在 shell.rs）。11 测试全绿（json/toml/yaml/xml/csv roundtrip + registry）。

**验收**：5 个 Format 实现各自 roundtrip/parse 单测通过；registry 能按名取到。

---

### 任务 M0.3 — 10 个格式命令内部改用 Format（用户接口不变）

**引用**：设计文档 §5.4。**v1 不删现有 10 个命令**，只把内部解析/序列化逻辑委托给 Format，消除 9 个走有损桥 `atom_to_pipeline_data`（`pipeline_convert.rs:35-50`）的 `unwrap_or_default()` 静默吞错。

- [x] **先写回归测试** `ash/auto-shell/tests/format_commands.rs`（6 个）：from_json/toml/yaml 文本解析 + from_json 外部流不空 + to_json 结构化序列化 + json roundtrip。先确立「当前行为」基线，改造后保持全绿 = 零行为变化。
- [x] **改造范围调整**：探勘发现实际有 53 个命令走有损桥（不只 10 个），且 `into_text`（29 处）自身用 `unwrap_or_default`。本轮聚焦 **5 个无参数格式命令** 的 run_atom 直接用 Format trait（消除有损桥双程转换）：`from_toml`/`from_xml`/`from_yaml`（into_text + Format.parse）+ `to_toml`/`to_yaml`（into_value + Format.serialize）。`from_json` 已是参照（M0.3 前就用 into_text）。
- [x] **保留 legacy run 路径**的命令：`from_csv`/`to_csv`（delimiter/header 参数）、`to_json`（pretty/compact）、`to_xml`（root/indent 参数）——它们有用户参数，Format 的固定默认 serialize 会丢失参数化能力。
- [x] 回归：auto-shell 774 全绿（+6 format_commands + 11 format trait）；`legacy_json_compat.rs`（028/007 信封，5）全绿。

**验收**：5 个格式命令行为向后兼容（6 集成测试守护）；外部命令 → from_json 不再静默空。csv/to_json/to_xml 等有参数命令保持原 legacy 路径（用户接口不变）。

**范围外（明确推迟）**：①`into_text` 的 `unwrap_or_default` 静默吞错（波及 29 处，另一计划）；②NDJSON 多行外部流 → from_json（trailing content，另一特性）；③剩余 48 个走有损桥的命令改造。

---

### M0 整体验收 ✅ 完成（2026-07-30）
- [x] Stream bug 修复测试绿（4 单元 + 2 端到端）+ 028/007 回归绿
- [x] Format trait + 5 实现 + registry 单测绿（11）
- [x] 格式命令回归绿（6 集成测试），行为不变
- [x] 全量回归：ash-core 356 + auto-shell 774 = **1130 全绿**（基线 1107 + 新增 23）

---

## M1：LazyNode + impl Iterator（2 周，~800 行）

**引用**：设计文档 §3.1-3.2。**放 `ash-core/src/pipeline/lazy.rs`**（新增）。

### 任务 M1.0 — 提升 helper 可见性（前置）✅ 完成
- [x] `operators.rs`：`get_field`/`compare`/`compare_order`/`as_f64` 提为 `pub`（LazyNode 复用，避免重复实现算子语义）；新增 `pub enum AggOp`（7 变体）。ash-core 356 全绿（纯可见性 + 新类型，无行为变化）。

### 任务 M1.1 — `LazyNode` enum + `impl Iterator` ✅ 完成
- [x] **先写流式性测试** `filter_yields_before_source_is_exhausted` + `take_short_circuits_without_consuming_all_source`——验证 lazy 本质（filter/take 在 source 未读完时产出）。
- [x] **实现 `LazyNode`**（设计 §3.1，7 变体）：`Source(IntoIter)` / `StreamSource(Box<dyn Iterator>)` / `Filter` / `Take` / `Select`（流式）/ `SortBy` / `Aggregate`（断流点）。
- [x] **新建 `AggOp`**（设计 §3.1）：`enum AggOp { Count, Uniq, GroupBy(String), Sum(String), Avg(String), Min(String), Max(String) }`（放 operators.rs，lazy 引用）。
- [x] **实现 `impl Iterator for LazyNode`**（设计 §3.2）：断流点用内部 `BreakState { Pending, Done(IntoIter) }`，首次 `next()` 触发 collect。
- [x] 复用 M1.0 提升后的 `get_field`/`compare`/`compare_order`/`as_f64`。

**实现偏差（vs 设计）**：①`Source` 用 `std::vec::IntoIter<Value>`（设计 §3.2 注释推荐），而非 `Source(Vec<Value>)`——因 enum 变体需自带游标才能实现 Iterator。②断流状态 enum 命名 `BreakState`（设计叫 `SortState`，但 sort 和 aggregate 共用，更通用的名字）。③`collect()` 对 aggregate 根返回标量（匹配 eager apply 语义），对行节点返回 Array。

10 测试全绿（流式性 ×2 + source/filter/select/sort/count/sum/uniq/stream 各一）；ash-core 366 全绿。

**验收**：流式性测试绿；每个算子的 lazy 行为单测绿（filter/take/select 流式产出；sort/agg 断流后产出）。

### 任务 M1.2 — `build_lazy` + 等价测试 ✅ 完成
- [x] **实现 `build_lazy(ops: &[PipelineOp], source: Value) -> LazyNode`**（设计 §3.4）：逐个把 PipelineOp 包成 LazyNode 层。11 个算子有直接 lazy 映射（Filter/Take/Select/SortBy + Count/Uniq/GroupBy/Sum/Avg/Min/Max→Aggregate）；5 个无 lazy 节点的（FilterAll/FilterAny/Map/SkipBack/Reverse）回退 eager `apply`（collect 当前 lazy 链 → eager 处理 → 继续）。`build_lazy` 保留 `operators::apply`（eager）不动，向后兼容。
- [x] **等价测试**（9 个）：`build_lazy([op], source).collect() == apply(op, source)` 逐算子——覆盖**全部 16 个 PipelineOp**（11 lazy + 5 eager 回退）+ 多段链 `filter|select|sort`。

19 测试全绿（10 M1.1 流式性 + 9 M1.2 等价）；ash-core 375 全绿（366+9）。`build_lazy` 暂留 `predicate_pushdown` 为 identity（M2.1 接通）。

**M1 整体验收**：流式性绿 + 16 算子 lazy/eager 等价绿 + 现有 1107 回归全绿。

---

## M2：谓词下推 + shell.rs 累积改造（1-2 周，~500 行，lazy 真正产生价值）

### 任务 M2.1 — 谓词下推 pass（v1 只两条保守规则，不跨 sort/agg）
- [ ] **测试**：`test_pushdown_filter_before_select`（规则 1：filter 在 select 前，字段在 select 前存在才交换）；`test_pushdown_merge_adjacent_filters`（规则 2：`.a > 1 | .b < 10` → `and`）；`test_pushdown_does_not_cross_sort`（保守：不下推跨断流点）。
- [ ] **实现 `predicate_pushdown(node: LazyNode) -> LazyNode`**（设计 §3.3 签名）。

### 任务 M2.2 — shell.rs 累积连续 DSL 段 → build_lazy（v2 策略）
- [ ] **测试** `tests/pipeline_dsl.rs`：`test_multi_dsl_stages_lazy`——`ls | filter .size > 10 | select .name | sort .name` 端到端，结果与 eager 一致（正确性），且通过计数器验证走了 lazy 路径。
- [ ] **实现 shell.rs 改造**：在 `execute_pipeline_with_auto` 循环里维护 `pending_ops: Vec<PipelineOp>`，遇到 DSL 段就 push 进去（不立即 apply），遇到非 DSL 段或 is_last 时 flush：`build_lazy(&pending_ops, source_val).collect()`。保留 M0.1 修的 Stream bug 分支作为 source 提取的前置。
- [ ] 回归：`pipeline_dsl.rs`（Plan 024 测试）+ `legacy_json_compat.rs`（5 个）全绿。

### 任务 M2.3 — 性能验证
- [ ] 写 benchmark/测试：构造 1 万行 source，对比 eager vs lazy 的峰值内存（设计目标 < eager 50%，主要在 take/early-exit 场景占优）。记录数据。

**M2 整体验收**：谓词下推两条规则 + 不跨断流点 + 多段 DSL 端到端正确 + 性能数据记录 + 全量回归绿。

---

## M3（可选）：ExternalStream → 真流式 StreamSource（1 周，~300 行）
- [ ] `ExternalStream::lines()` → `LazyNode::StreamSource` 桥接。
- [ ] `find ... | filter` 真流式（find 持续产出 + filter 边读边过滤）。
- [ ] 把 M0.1 的 bug 修复从「collect 成 Array」升级为「真流式」。
- **依赖**：M2 完成。**可推迟**——M0 已修复丢数据，M3 是性能优化。

---

## 依赖与并行性

```
M0.0(基线)✅ → M0.1(bug) ─┬→ M0.2(Format trait) → M0.3(10 命令)
                            └→ M1.0(get_field pub) → M1.1(LazyNode) → M1.2(build_lazy) → M2.1(下推) → M2.2(shell改造) → M2.3(性能) → M3(可选)
```

M0.1（bug 修复）与 M0.2（Format trait）**可并行**（互不依赖）。其余严格串行。

## 与其他 Plan 的接触点（改动时守护）

| 接触点 | 守护方式 |
|---|---|
| Plan 007 `pipeline_to_json`（`shell.rs:971`）— 现「信封」 | `legacy_json_compat.rs` 5 测试 |
| Plan 024 DSL（`pipeline_dsl.rs` 测试） | M2.2 改造后必跑 |
| Plan 028 Agent 引擎 | 本仓库已删除（委托 auto-ai），无需守护；`AshCommandTool`（029）走 `execute_for_agent` text 路径 |
| Plan 036 parity（80 case） | lazy 改造若动命令输出格式，跑 `parity.rs` |
| OS pipe 链（`spawn_external_chained`/`into_raw_stdout`）| **禁区**，lazy 只发生在数据进入 Ash 之后 |

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| lazy 语义改变用户可见行为（排序/去重时机） | 算子语义不变只改执行策略；M2.2 端到端等价测试守护 |
| 谓词下推误下推（跨断流点） | M2.1 v1 只两条保守规则 + 不跨 sort/agg 测试 |
| `get_field` 提 pub 影响范围 | M1.0 单独提交，先跑回归 |
| `to_*` config 收敛破坏 10 命令 | M0.3 逐个回归，用户接口不变 |
| auto-val Value API 不够用 | M1.0 提 `get_field` pub 已缓解 |

## 成功指标（设计 §6.5，已更新基线）

1. **M0**：外部命令 → DSL 段不丢数据（M0.1 测试）+ Format 5 实现 + 单测
2. **M1**：filter 在 Source 未读完时已产出（流式性测试 M1.1）
3. **M2**：`ls | filter .size > 10 | select .name | sort .name` 端到端 lazy；1 万行内存对比记录
4. **M3（可选）**：`find / | filter .name contains "log"` 真流式
5. **回归**：现有 **1107** 测试全过；from_json/csv 等行为不变
