# Plan 007: MS1 — Agent 可调用（非交互 + 稳定 + JSON 输出）

- **日期**: 2026-06-26
- **状态**: ✅ 已完成（2026-07-02）
- **RoadMap**: MS1（`docs/roadmap.md`）
- **目标**: 让 AI 能 `ash -c "ls | sort -w size" --json` 一次性执行并拿到结构化 JSON 结果。

## 1. 背景与现状

### 好消息：非交互模式已存在
ash 已实现非交互执行（Plan 303/304），`main.rs` 支持：
- `ash -c "cmd"`：执行单条命令
- `ash script.ash`：执行脚本文件
- `ash -s`：从 stdin 读脚本

**所以 MS1 不是从零做非交互**，而是补齐"Agent 可用"的三个缺口。

### 三个缺口
1. **`--json` 未实现**：`-c` 现在直接 `println!("{}", 渲染字符串)`，Agent 拿到的是人读的表格文本，无法解析。需要把管道末端的 **AtomPipeline 序列化成 JSON**。
2. **退出码不完整**：只有成功 0 / 错误 1。POSIX 还要 2（用法错误）/126（不可执行）/127（未找到）。
3. **健壮性**：存在 `unwrap()`/`expect()`，corner case 可能 panic。Agent 调用要求"绝不崩"。

### 关键实现路径（已确认）
- `execute()`（shell.rs:243）返回 `Result<Option<String>>`，末端调 `format_output`（shell.rs:682）把 AtomPipeline 渲染成 String。
- **execute 内部保留了 AtomPipeline**（format_output 的入参），所以 `--json` 可以复用这条链路，只需末端换成 JSON 序列化。
- **`to_json` 命令已存在**（`cmd/commands/to_json.rs`），有手写的 `value_to_json` 函数——可复用来序列化 Atom 的 value。

## 2. 设计

### 行为契约

| # | 规则 |
|---|---|
| 1 | `ash -c "cmd"` 默认输出渲染文本（表格/记录），与现状一致（向后兼容）|
| 2 | `ash -c "cmd" --json` 输出 JSON：管道末端 Atom 的 value 序列化成 JSON 到 stdout |
| 3 | `--json` 时，文本类输出（Text/Empty）序列化成 JSON 字符串（`"hello\n"`）|
| 4 | `--json` 时，结构化输出（Table/Record/FileList/MatchList）序列化成 JSON 数组/对象 |
| 5 | stdout 纪律：`--json` 模式下，数据走 stdout，所有诊断/错误走 stderr |
| 6 | 退出码：0 成功 / 1 命令错误 / 2 用法错误 / 126 不可执行 / 127 未找到 |
| 7 | 无 panic：所有 corner case 返回错误（→ 非零退出码 + stderr）|

### `--json` 的 JSON 映射
AtomPipeline 末端 → JSON：
- `AtomPipeline::Atom(atom)` → `value_to_json(atom.value)`（复用 to_json.rs）
- `AtomPipeline::Text(s)` → `serde_json::json!(s)` 或手写 `"s"`
- `AtomPipeline::Empty` → `null`
- 其他 → 序列化底层 value

### 不在本期范围（YAGNI）
- `--format csv/table/text`（本期只 json，其他后续）
- 沙盒/权限（MS2）
- 脚本编程能力（MS3）
- 全量 panic 审计（本期聚焦高频路径的 unwrap，不追求 100%）

## 3. 实现架构

### 改动 A：main.rs 加 `--json` flag + 改 `-c` 输出逻辑
```rust
// 解析 --json 全局 flag
let json_mode = args.contains("--json");

// -c 分支：
match shell.execute_for_agent(command, json_mode) {
    Ok(output) => {
        if let Some(s) = output { println!("{}", s); }
    }
    Err(e) => { eprintln!("Error: {}", e); std::process::exit(exit_code_for(&e)); }
}
```

### 改动 B：shell.rs 新增 `execute_for_agent`
返回 `Result<Option<String>>`，但末端根据 `json_mode` 选择 format_output 或 json 序列化：
```rust
pub fn execute_for_agent(&mut self, input: &str, json_mode: bool) -> Result<Option<String>> {
    // 复用 execute_inner 的管道逻辑，但末端不调 format_output，
    // 而是拿到 AtomPipeline：
    //   json_mode=true → atom_to_json(pipeline) → String
    //   json_mode=false → format_output(pipeline)（现状）
}
```
> 注：execute_inner 末端调 format_output 后返回 String。需要让管道末端**返回 AtomPipeline**而非渲染后的 String。这是核心改动——execute_inner / execute_pipeline 的末端格式化要可配置。

### 改动 C：退出码映射
新增 `exit_code_for(err)` 把 miette 错误映射到 POSIX 退出码（先简化：用法错误→2，未找到→127，其余→1）。或让 shell 内部记录错误类型。

### 改动 D：复用 to_json 的 value_to_json
`to_json.rs::value_to_json(&Value) -> String` 已存在（手写序列化）。`--json` 末端用它把 Atom.value 序列化。

## 4. 测试策略

| 层级 | 测试 | 方式 |
|---|---|---|
| 单元 | `value_to_json` 对 Array/Obj/标量 的序列化 | 复用/扩展 to_json 测试 |
| 单元 | `atom_to_json(AtomPipeline)` 各变体 | 新增函数测试 |
| 集成 | `execute_for_agent("ls", true)` 返回 JSON 字符串 | Shell + 断言 JSON 合法 |
| 集成 | `execute_for_agent("ls", false)` 返回渲染文本（兼容） | 断言含表格 |
| CLI | 子进程 `ash -c "echo hi" --json` 输出 `"hi\n"` | std::process::Command |
| CLI | `ash -c "ls /no/such"` 退出码非 0，stderr 有诊断 | Command + exit code |
| 错误 | 用法错误 → 退出码 2 | Command |

### TDD 流程
1. RED：`atom_to_json` 单测（各 AtomPipeline 变体）→ 失败（函数不存在）
2. GREEN：实现 atom_to_json（复用 value_to_json）
3. RED：`execute_for_agent(..., true)` 集成测试 → 失败（方法不存在）
4. GREEN：改 execute_inner 末端可配置 + 新增 execute_for_agent
5. RED：CLI 子进程 `--json` 测试 → 失败（main 未加 flag）
6. GREEN：main.rs 加 --json + 退出码
7. 回归

## 5. 实施步骤

1. ✅ **确认 execute_inner 末端格式化接缝**（line 682 format_output 的调用点），设计"末端返回 AtomPipeline"的最小改动。
2. ✅ **TDD: atom_to_json**（复用 value_to_json）。
3. ✅ **TDD: execute_for_agent**（execute_inner 末端可配置）。
4. ✅ **TDD: main.rs --json flag + 退出码**。
5. ✅ **健壮性**：审计 -c 路径的高频 unwrap（main.rs + execute 路径）。
6. ✅ **全量回归**：cargo test（含现有交互测试不破坏）。
7. ✅ **手动验证**：`ash -c "ls | sort -w size" --json` 输出合法 JSON。
8. ✅ 提交 + push（commits `cd90d3a`、`8af40cb`）。

## 6. 验收标准

- [x] `ash -c "echo hi" --json` → 输出 `"hi\n"`（JSON 字符串）
- [x] `ash -c "ls" --json` → 输出 JSON 数组（文件列表）
- [x] `ash -c "ls | sort -w size" --json` → 结构化 JSON，含 name/size 字段
- [x] `ash -c "ls"`（无 --json）→ 渲染表格（向后兼容）
- [x] `ash -c "ls /no/such"` → 退出码非 0（exit=1），stderr 有诊断，stdout 空
- [x] `ash -c "show data.csv | grep alice" --json` → JSON 匹配结果
- [x] 全量 cargo test 通过，无回归（563 单测 + 全部集成测试）
- [x] -c 高频路径无 panic（0 个 unwrap/expect/panic/unreachable）

## 7. 风险

- **execute_inner 末端改造**（最大风险）：format_output 在多处调用，改成"末端返回 AtomPipeline"可能波及多个分支。需谨慎，保持交互模式（format_output）完全不变。
- **JSON 序列化保真**：to_json 的 value_to_json 是手写的，需确认它覆盖 Atom.value 的所有变体（特别是 Int/Float/Bool/Null/嵌套）。
- **退出码映射的简化**：本期不区分 126/127 的精确语义（"不可执行"vs"未找到"难区分），先用 2/1 两档，127/126 标注为"后续完善"。
- **stdout/stderr 纪律**：现有命令可能把诊断信息 println 到 stdout（而非 eprintln stderr）。本期聚焦 -c 路径的诊断走 stderr，全量清理 stdout 污染留后续。

## 8. 完成总结（2026-07-02）

MS1 全部验收通过，落地提交 `cd90d3a`、`8af40cb`：

- `--json` 支持 `-c` / `-s` / 脚本文件 三种入口；脚本模式下每条命令输出一行 NDJSON。
- `--json` 修复为**全局 flag**（可在命令行任意位置出现），原实现因 `-c` 分支提前 return 导致 `ash -c "x" --json` 不生效。
- Panic 审计：`-c` 全路径（main → execute_for_agent → execute → execute_inner → format_output/pipeline_to_json → value_to_json）**0 个 unwrap/expect/panic/unreachable**，无需加固。

### 已显式延后（YAGNI / §7 风险已声明）
| 项 | 原因 | 落点 |
|---|---|---|
| 退出码 126（不可执行）/ 127（未找到） | §7：与"未找到"难区分，本期用 1/2 两档 | 后续完善 |
| 全量 stdout 诊断清理 | §7：本期仅 -c 路径诊断走 stderr | 后续 |
| `--format csv/table/text` | §2 YAGNI | 后续 |
