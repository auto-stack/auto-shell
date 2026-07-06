# Plan 001: `open` 命令（JSON/CSV 文件解析进管道）

- **日期**: 2026-06-25
- **状态**: ✅ 已完成（2026-06-25）
- **目标**: 新增 `open` 命令，按文件扩展名把 JSON/CSV 解析成结构化数据进管道，末尾自动渲染成表格；未知格式回退纯文本。

## 1. 背景与现状

ash **已实现** `from_json` / `from_csv`（以及 to_*），均为手写解析器，零 serde 依赖。解析结果为 `auto_val::Value`（JSON）或 `Value::Array<Obj>`（CSV），与 `ls` 输出同构，能被现有 `render_table_with` 自动渲染成表格。

**唯一缺口**：这些命令只吃管道里的文本，不认文件路径。用户必须先 `cat f.csv | from_csv`。

**关键发现**（决定实现方式）：
- `from_json::parse_json(&str) -> Result<Value>` 是 `pub fn`（`from_json.rs:270`）
- `from_csv::parse_csv(&str, &str, bool) -> Result<Array>` 是 `pub fn`（`from_csv.rs:68`）

两者都是公开自由函数，`open` 可直接调用，**无需走 registry、无需重构、零重复代码**。

## 2. 行为契约（11 条规则）

**命令**：`open`
**用法**：
```
Usage: open [<file>] [--as <format>]

Open a file (or pipeline text) and parse it into the pipeline.

Arguments:
  <file>            Path to the file to open (optional; if omitted, reads pipeline text)

Options:
  --as <format>     Force a format: json | csv | text  (default: infer from file extension)
```

| # | 规则 |
|---|---|
| 1 | 有 `<file>` → 读该文件（相对 `shell.pwd()`，`~` 已支持，复用 `resolve_path`） |
| 2 | 无文件 + 管道文本输入 → 处理管道文本（`cat f.csv \| open --as csv` 模式） |
| 3 | 无文件 + 无管道输入 → 报错 `open: no input (provide a file or pipe text)` |
| 4 | 格式判定优先级：`--as` 指定 > 文件扩展名（`.json`/`.csv`）> 默认纯文本 |
| 5 | `.json` → 文本喂给 `parse_json`，复用现有解析 |
| 6 | `.csv` → 文本喂给 `parse_csv`，**用默认参数**（逗号分隔、有表头）。不透传 flag；高级需求用 `from_csv` 显式命令 |
| 7 | 未知/无扩展名 → 纯文本（等价 `cat`） |
| 8 | 管道输入：`open` 接受管道文本输入（见规则 2）；`<file>` 与管道输入同时存在时，**`<file>` 优先**，管道输入被忽略 |
| 9 | 文件不存在 → 报错 `open: <path>: No such file or directory`，不动管道 |
| 10 | 多文件 → 本期不支持（报错），保持 YAGNI |
| 11 | `--as` 与文件扩展名冲突（如 `open f.csv --as json`）→ **`--as` 优先**，按 JSON 解析 |

**核心约束**：`open` 自身不写任何解析逻辑，全部转发给现有的 `parse_json` / `parse_csv`。

**显式不在本期范围（YAGNI）**：
- 不改 `Atom` / `AtomPipeline` / `batom`（不沿管道传文件元数据）
- 不动 registry 查找机制
- 不动 `from_*` 命令
- 不做写出（已有 `to_*` + 重定向）
- 不做多文件、不透传 from_csv 的 flag、不处理二进制文件

## 3. 实现架构

### 模块结构
- 新文件 `ash/auto-shell/src/cmd/commands/open.rs`，实现 `OpenCommand`
- `cmd/commands/mod.rs` 加 `pub mod open;` + `pub use open::OpenCommand;`
- `shell.rs` 的 `Shell::new()` 注册一行 `reg.register(Box::new(OpenCommand));`

### Signature
```rust
Signature::new("open", "Open a file and parse it by extension")
    .optional("file", "Path to the file to open")
    .option_with_short("as", 'a', "Force format: json | csv | text")
```
> 注：需先确认 `Signature` 是否支持 `option_with_short`（探查报告 cmd.rs:67-247 提到有）。若 `--as` 需带值，用 `.option()`/`.option_with_short()`；若短选项 `-a` 冲突则省略短选项。

### `open::run` 逻辑（伪代码）
```rust
fn run(&self, args: &ParsedArgs, input: PipelineData, shell: &mut Shell) -> Result<PipelineData> {
    // 1. 取输入文本
    let (text, ext_hint) = if let Some(path) = args.positionals.first() {
        let resolved = resolve_path(path, shell);     // 复用 cat 的范式
        if !resolved.exists() { bail!("open: {}: No such file or directory", path); }
        (read_to_string(&resolved)?, extension_of(&resolved))
    } else {
        // 无文件参数 → 从管道取文本
        match input {
            PipelineData::Text(s) => (s, None),
            PipelineData::Value(Value::Str(s)) => (s.to_string(), None),
            _ => bail!("open: no input (provide a file or pipe text)"),
        }
    };

    // 2. 判定格式
    let fmt = resolve_format(args, ext_hint);   // --as > ext > Text

    // 3. 解析
    match fmt {
        Format::Json => Ok(PipelineData::from_value(parse_json(&text)?)),
        Format::Csv  => Ok(PipelineData::from_value(Value::Array(parse_csv(&text, ",", true)?))),
        Format::Text => Ok(PipelineData::from_text(text)),
    }
}
```

### 辅助函数（建议作为 `open.rs` 内私有函数，便于单测）
- `resolve_format(args: &ParsedArgs, ext_hint: Option<&str>) -> Format` —— 格式判定（规则 4/11）
- `extension_of(path: &Path) -> Option<String>` —— 取小写扩展名
- `enum Format { Json, Csv, Text }`

### 路径解析
复用 `from_csv`/`cat` 已有的范式。需确认 `resolve_path` 的确切位置与签名（探查提到 cat.rs:42-47 用了 `resolve_path(arg, shell)` + `shell.pwd()`）。若 `resolve_path` 未导出，参照 cat 的实现就地写一个等价的（相对 `shell.pwd()`，先 `expand_tilde`）。

## 4. 测试策略

| 层级 | 测试 | 方式 |
|---|---|---|
| 单元 | `resolve_format`：扩展名判定、`--as` 覆盖扩展名、未知扩展名→Text、无扩展名→Text | 纯函数直接测 |
| 单元 | open 读 JSON 文件 → `Value::Obj`/`Value::Array` | tempdir 建文件 + `OpenCommand.run` |
| 单元 | open 读 CSV 文件 → `Value::Array<Obj>`，断言行数/列 | tempdir + read |
| 单元 | open 未知扩展名 → `PipelineData::Text` | tempdir |
| 单元 | open 管道模式（无文件，`--as csv`）吃文本 → 解析 | 喂 `PipelineData::Text` |
| 单元 | 错误：文件不存在 bail；无输入 bail；多文件 bail | bail 断言 |
| 集成 | `execute("open t.csv")` 末尾渲染出非空表格 | `shell.execute` + 断言输出含表头 |
| 集成 | `execute("open --help")` 正常输出 | 断言 help 文本 |

### TDD 流程
1. **RED**：先写 `resolve_format` 单测（期望行为）→ 失败（函数不存在）
2. **GREEN**：实现 `resolve_format` → 通过
3. **RED**：写 open 读 CSV 文件单测 → 失败（OpenCommand 不存在）
4. **GREEN**：实现 `OpenCommand::run` + 注册 → 通过
5. 逐个补全其余测试，红→绿循环
6. 全量 `cargo test --lib` 确认无回归

### 临时文件处理
优先用 `tempfile` crate（若已是依赖）；否则在 `std::env::temp_dir()` 下建唯一前缀文件，测试结束 `std::fs::remove_file` 清理（参照之前 cd 测试的清理范式）。

## 5. 实施步骤（按顺序）

1. **确认依赖与签名细节**：检查 `Signature::option_with_short` 是否存在、`resolve_path` 是否导出、`tempfile` 是否已是依赖。
2. **TDD: resolve_format 单元** → 实现枚举 + 判定函数。
3. **TDD: OpenCommand 骨架** → `open.rs` 文件 + `Signature` + `run`（先支持文件模式 + 扩展名判定）。
4. **TDD: 管道模式** → 补规则 2/3。
5. **TDD: --as 选项** → 补规则 4/11。
6. **TDD: 错误处理** → 补规则 9/10。
7. **注册命令**：`mod.rs` + `shell.rs`。
8. **集成测试**：`execute("open ...")` 渲染验证 + help。
9. **全量回归**：`cargo test --lib` 全过。
10. **手动验证**：构建后 `open <真实 csv/json 文件>` 看表格输出。
11. 提交 + push。

## 6. 验收标准

- [ ] `open data.csv` 显示表格（表头 + 行）
- [ ] `open data.json`（对象数组）显示表格
- [ ] `open data.json`（单对象）显示记录
- [ ] `open data.txt` 显示纯文本
- [ ] `open` 无参数无管道 → 报错
- [ ] `open nonexistent.csv` → 报错
- [ ] `open f.csv --as json` → 按 JSON 解析
- [ ] `cat f.csv | open --as csv` → 表格（管道模式）
- [ ] `open --help` → 正常帮助
- [ ] 全量 `cargo test --lib` 通过，无回归
