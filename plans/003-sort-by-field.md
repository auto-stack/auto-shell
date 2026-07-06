# Plan 003: 扩展 `sort` 命令——字段排序（`-k` 列号 + `-w` 字段名）

- **日期**: 2026-06-25
- **状态**: ✅ 已完成（2026-06-25）
- **目标**: 让 `sort` 支持按字段排序，使 `open t.csv | sort -w age` 能对表格按列排序。

## 1. 背景与现状

### 触发场景
用户执行 `open tmp/test.csv | sort .Age`，期望按 Age 列排序，但**没有效果**。

### 根因
ash 的 `sort`（`ash/auto-shell/src/cmd/commands/sort.rs`）是一个**纯文本行排序器**：
- 只按整行排序（sort.rs:80 `text.lines().collect()`）
- positional 参数是 `<file>`（当文件路径读），不是排序字段——`.Age` 被当文件路径/忽略
- `run_atom`（sort.rs:52-55）把结构化输入**强制拍扁成文本**，丢弃字段信息

### POSIX 对照
ash 的 sort **不是完整的 POSIX sort**。对照 [POSIX sort 规范](https://pubs.opengroup.org/onlinepubs/9699919799.orig/utilities/sort.html)：

| POSIX 选项 | 含义 | ash 现状 |
|---|---|---|
| `-r` | reverse | ✅ |
| `-n` | numeric | ✅ |
| `-u` | unique | ✅ |
| `-f` | ignore-case | ✅ |
| **`-k keydef`** | **按字段排序** | ❌ **缺失（POSIX 核心）** |
| **`-t char`** | **字段分隔符** | ❌ **缺失** |

POSIX sort 的核心能力就是 `-k`（按字段排序）。补上它不是"扩展"，而是**补齐标准缺失**。

### 已核查事实
- ash **没有**任何"按字段排序"命令（`sort_by` 仅出现在 du/ps/help 内部局部排序）
- `-w` **不被 POSIX sort 占用**（POSIX 选项：-m/-o/-c/-b/-d/-f/-i/-n/-r/-t/-u/-k，无 -w）
- `Signature::option_with_short` 存在（cmd.rs:171），`ParsedArgs::get_option` 取值（parser.rs:29）——支持 `-w age` 这类带值 option

## 2. 设计

### 设计原则：双轨语义
扩展 `sort`（不新增命令），同时支持文本和结构化输入的字段排序：

| Flag | 输入类型 | 语义 | 示例 |
|---|---|---|---|
| `-k <列号>` | 文本 | POSIX：按第 N 列排序 | `sort -k 2 -t , file.csv` |
| `-w <字段名>` | 结构化 (Array<Obj>) | ash 扩展：按对象字段名排序 | `open t.csv \| sort -w age` |
| 无 -k/-w | 文本/结构化 | 现有行为（整行排序） | `sort file` |

### 核心改动：sort 必须能保留结构化输入
当前 `run_atom` 把 Array<Obj> 拍扁成文本（sort.rs:52-55），这是 `-w` 的最大障碍。改动：
- 当输入是结构化 `Value::Array` 且指定了 `-w` 时，**不拍扁**，对数组元素按字段排序后，返回**结构化** `PipelineData::Value`（保持 Array<Obj>，末尾自动渲染成表格）。
- 无 `-w` 时维持现有文本行为（向后兼容）。

### 行为契约

| # | 规则 |
|---|---|
| 1 | `-w <字段名>`：输入必须是 `Array<Obj>`；按各对象的该字段排序，输出保持 `Array<Obj>` |
| 2 | `-k <列号>`：输入为文本；按指定列（1-based）排序；配合 `-t` 指定分隔符（默认空白） |
| 3 | `-w` 与 `-k` 互斥：同时指定报错 `sort: -w and -k are mutually exclusive` |
| 4 | `-w` + 非 Array 输入：报错 `sort -w requires a list of records` |
| 5 | `-w` + 字段不存在：该字段的元素排到最后（或报错，二选一——见决策点） |
| 6 | `-r`（reverse）对 `-w`/`-k` 排序结果同样生效 |
| 7 | `-n`（numeric）对 `-w`/`-k` 同样生效：数字字段按数值排，否则按字符串 |
| 8 | 无 `-w`/`-k` 时：完全维持现有行为（文本行排序）——**零回归** |
| 9 | `-t <分隔符>`：仅对 `-k`（文本模式）生效，指定字段分隔符（默认空白序列） |

### 决策点（写实现时定，影响小）
- 规则 5（`-w` 字段不存在）：选择"缺失字段排到末尾"（比报错更宽容，符合 Nushell）。在 plan 实施时明确。

### 不在本次范围（YAGNI）
- 不实现 POSIX `-k` 的完整 keydef 语法（`-k 2.3,4` 起止字符）——本期 `-k <列号>` 只支持单列号
- 不支持多字段排序（`-w a -w b`）
- 不动其他命令（where/select/get）

## 3. 实现架构

### Signature 改动
```rust
Signature::new("sort", "Sort lines or records by field")
    .optional("file", "File to sort (default: stdin)")
    .flag_with_short("reverse", 'r', "Reverse sort order")
    .flag_with_short("numeric", 'n', "Numeric sort")
    .flag_with_short("unique", 'u', "Remove duplicate lines")
    .flag_with_short("ignore-case", 'f', "Fold lower case to upper case")
    .option_with_short("key", 'k', "Sort by column NUMBER (1-based, text mode)")
    .option_with_short("field-separator", 't', "Field separator char for -k (default: whitespace)")
    .option_with_short("with", 'w', "Sort records by FIELD name (structured mode)")
```

### `run` 逻辑（伪代码）
```rust
fn run(&self, args, input, shell) -> Result<PipelineData> {
    let key = args.get_option("key");      // -k 列号
    let with = args.get_option("with");    // -w 字段名
    let reverse = args.has_flag("reverse");
    let numeric = args.has_flag("numeric");
    let sep = args.get_option("field-separator");  // -t

    // rule 3: 互斥
    if key.is_some() && with.is_some() {
        bail!("sort: -w and -k are mutually exclusive");
    }

    if let Some(field) = with {
        // rule 1: 结构化字段排序
        let arr = expect_array_of_obj(&input)?;   // rule 4
        let sorted = sort_array_by_field(arr, field, reverse, numeric)?;
        Ok(PipelineData::from_value(Value::Array(sorted)))  // 保持结构化
    } else {
        // 文本模式（现有 -k 或整行排序）
        let text = get_text_or_file(args, input, shell)?;
        if let Some(k) = key {
            let col = k.parse::<usize>()?;
            Ok(PipelineData::from_text(sort_by_column(&text, col, sep, reverse, numeric, unique, ignore_case)))
        } else {
            // 现有整行排序（零改动）
            Ok(PipelineData::from_text(sort_lines(&text, reverse, numeric, unique, ignore_case)))
        }
    }
}
```

### `run_atom` 改动（关键）
当前 run_atom 强制拍扁。改为：
- 结构化输入 + `-w`：直接在 AtomPipeline 层操作，返回结构化 Atom（类型标签由 `pipeline_data_to_atom` 自动推断为 Table）
- 否则：维持现有桥接（拍扁成文本）

最简实现：`run_atom` 走通用桥接（`atom_to_pipeline_data` → `run` → `pipeline_data_to_atom`），让 `run` 的结构化输出自动获得正确类型标签。即**去掉**当前 run_atom 里手动包 `Atom::Text` 的逻辑，改用通用桥接（和 from_csv/open 一致）。

### 新增辅助函数（私有，便于单测）
- `sort_array_by_field(arr: Array, field: &str, reverse: bool, numeric: bool) -> Result<Array>` —— 结构化字段排序
- `sort_by_column(text: &str, col: usize, sep: Option<&str>, ...) -> String` —— 文本列排序
- `field_sort_value(v: &Value, numeric: bool) -> SortKey` —— 提取可比较的排序键（数字/字符串）

## 4. 测试策略

| 层级 | 测试 | 方式 |
|---|---|---|
| 单元 | `sort_array_by_field`：按数字字段升/降序 | 构造 Array<Obj> 直接测 |
| 单元 | `sort_array_by_field`：按字符串字段排序 | 直接测 |
| 单元 | `sort_array_by_field`：缺失字段的元素排末尾（规则 5） | 直接测 |
| 单元 | `sort_by_column`：按第2列排序，逗号分隔 | 直接测 |
| 单元 | `sort_by_column`：列号越界处理 | 直接测 |
| 集成 | `open t.csv \| sort -w age` → 表格按 age 排序 | tempdir + execute + strip_ansi |
| 集成 | `sort -k 2 -t , file.csv` → 文本按列排序 | tempdir + execute |
| 回归 | 现有 `sort file`、`sort -n`、`sort -r`、`sort -u` 全部不变 | 现有测试 |
| 错误 | `-w` 与 `-k` 同时指定报错 | execute |
| 错误 | `-w` + 文本输入报错 | execute |

### TDD 流程
1. RED：`sort_array_by_field` 单测（期望升序）→ 失败（函数不存在）
2. GREEN：实现结构化字段排序
3. RED：`sort_by_column` 单测 → 失败
4. GREEN：实现文本列排序
5. RED：集成 `sort -w age` → 失败（run_atom 拍扁）
6. GREEN：改 run_atom 用通用桥接 → 通过
7. 回归：现有 sort 测试全过

## 5. 实施步骤

1. **TDD RED**：写 `sort_array_by_field` 单测（数字/字符串字段）。
2. **TDD GREEN**：实现结构化字段排序函数。
3. **TDD RED**：写 `sort_by_column` 单测（文本列排序）。
4. **TDD GREEN**：实现文本列排序函数。
5. **改 Signature**：加 `-k`/`-t`/`-w` option。
6. **改 run**：分支处理 `-w`（结构化）/ `-k`（文本列）/ 整行（现有）。
7. **改 run_atom**：去掉手动拍扁，用通用桥接（让结构化输出保留）。
8. **TDD 集成**：`open t.csv | sort -w age` 端到端。
9. **错误处理**：互斥、类型不符。
10. **全量回归**：cargo test。
11. 提交 + push。

## 6. 验收标准

- [ ] `open tmp/test.csv | sort -w age` → 表格按 age 升序（25,28,30,35,40）
- [ ] `open tmp/test.csv | sort -w age -r` → 降序
- [ ] `open tmp/test.csv | sort -w name` → 按名字字母序
- [ ] `sort -k 2 -t , tmp/test.csv` → 文本按第2列排序
- [ ] `sort -w age -k 2`（同时指定）→ 报错
- [ ] `echo "x" | sort -w age`（文本+字段名）→ 报错
- [ ] 现有 `sort`、`sort -n`、`sort -r`、`sort -u`、`sort -f` 行为不变
- [ ] 全量 cargo test 通过，无回归

## 7. 风险与备注

- **核心风险**：改 `run_atom` 去掉手动拍扁，可能影响现有"sort 在管道末尾输出文本"的预期。需确认：无 `-w` 时仍走文本路径（run 返回 Text，桥接后是 Atom::Text），渲染不变。
- **向后兼容**：无 `-w`/`-k` 时走完全相同的现有代码路径，零行为变更。
- **POSIX 完整性**：本期 `-k` 只支持单列号，不实现完整 keydef（`-k 2.3,4`）。这是有意的范围控制，后续可补。
