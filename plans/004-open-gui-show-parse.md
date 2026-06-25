# Plan 004: `open` 改为 GUI 打开，解析命令改名 `show`

- **日期**: 2026-06-25
- **状态**: 待实施
- **目标**: 让 `open` 承担 auto-os 生态统一的"打开/启动应用"语义（用系统默认程序 GUI 打开文件），把 Plan 001 的文件解析命令改名为 `show`。

## 1. 背景与命名决策

### 语义冲突
Plan 001 用 `open` 实现"按扩展名把文件解析进管道"。但用户（项目维护者）指出：在整个 **auto-os 生态**（auto-shell + auto-ui + auto-man + ...）里，`open` 已确立**"打开/启动应用"**的统一语义：
- `auto-man open` = 用 IDE 打开项目
- ash 的 `open` 应 = 用系统默认程序打开文件（GUI）
- 这符合"完整 OS（带界面）"的整体设计——`open` 就是"打开应用"

因此 Plan 001 的解析命令需要改名，让 `open` 让位给 GUI 语义。**命名一致性（项目内部生态约定）优先于外部工具惯例**（Nushell 里 open=解析，但那是别的项目）。

### 命名核对（已查证）
- **`open`**：非 POSIX 命令（macOS/BSD 的 open 是平台扩展）；非 Nushell 内置（Nushell 的 open 是解析，但我们按生态约定覆盖它）
- **`show`**：非 POSIX、非 Nushell、非 fish 内置命令（仅作为子命令/flag 出现）→ **无撞名**，可安全使用

### 实现选择：`opener` crate（已核实依赖）
跨平台 GUI 打开采用 [`opener`](https://crates.io/crates/opener) crate。已核实依赖极轻：
```
opener v0.8.5
├── normpath v1.5.1
└── windows-sys / windows-link  (仅 Windows)
```
- 传递依赖仅 3 个 crate；`windows-sys`/`windows-link` 是 Windows 专属，**ash 已有**（sysinfo/crossterm/ratatui 依赖），故近乎零新增体积。
- `reveal` 默认 feature 不引入额外依赖。
- 比手写 `std::process::Command` 分平台更稳健（封装了 `ShellExecuteW`/`open`/`xdg-open` 的边界情况、URL 转义、Linux `gio open` 回退）。

## 2. 设计

### 两个命令的最终形态

| 命令 | 语义 | 实现 |
|---|---|---|
| `open <file>` | 用系统默认程序 GUI 打开文件（类 xdg-open/explorer） | `opener::open(path)` |
| `show [<file>] [--as <fmt>]` | 按 扩展名/`--as` 解析文件进管道（Plan 001 原行为） | 复用 `parse_json`/`parse_csv` |

### `open`（GUI 打开）行为契约
| # | 规则 |
|---|---|
| 1 | `open <file>` → 用系统默认程序打开该文件 |
| 2 | 文件不存在 → 报错 `open: <path>: No such file or directory` |
| 3 | 无参数 → 报错 `open: missing file argument` |
| 4 | 路径解析相对 `shell.pwd()`，支持 `~`（复用 resolve_path + expand_tilde） |
| 5 | 不接受管道输入（它是 GUI 启动器，非管道过滤器） |
| 6 | 成功 → 无管道输出（返回 Empty），失败 → 报错 |
| 7 | 多文件：本期单文件（YAGNI） |

### `show`（解析，原 open 改名）
完全保留 Plan 001 的 11 条行为规则，仅改命令名 `open` → `show`。包括：文件/管道双输入源、`--as` 格式覆盖、扩展名嗅探、错误处理。详见 Plan 001。

### 不在范围（YAGNI）
- `open` 不做 URL/目录特殊处理（opener 已能处理 URL 和目录，本期直接透传即可，不做额外封装）
- `show` 不新增能力（纯改名 + 迁移）

## 3. 实现架构

### 改动清单

#### (a) 新增 `open`（GUI）：重写 `cmd/commands/open.rs`
- 当前 open.rs 是解析命令（Plan 001）。**整文件重写**为 GUI 打开命令：
  - `use opener;` 调 `opener::open(&path_string)`
  - Signature：`open <file>`，单一 required positional
  - `run`：resolve_path → exists 检查 → `opener::open()` → 返回 Empty
  - `run_atom`：桥接（结果 Empty）

#### (b) 新建 `show`（解析）：新建 `cmd/commands/show.rs`
- 把 Plan 001 的解析逻辑从 open.rs **迁移**到 show.rs：
  - 结构体 `ShowCommand`（原 `OpenCommand`）
  - `name()` 返回 `"show"`
  - 保留 `Format`/`resolve_format`/`parse_text`/`extension_of`/`resolve_path` 等全部辅助函数
  - 保留 `--as` option、管道模式、所有测试（测试里 "open" → "show"）

#### (c) 依赖：`Cargo.toml` 加 `opener`
```toml
opener = "0.8"
```

#### (d) 注册：`shell.rs` + `cmd/commands/mod.rs`
- `mod.rs`：`pub mod open;`（保留）+ 新增 `pub mod show;`
- `shell.rs`：`OpenCommand`（新语义）+ `ShowCommand` 都注册

### `open::run` 伪代码
```rust
fn run(&self, args, _input, shell) -> Result<PipelineData> {
    let path = args.first().ok_or_else(|| miette!("open: missing file argument"))?;
    let resolved = resolve_path(path, shell);
    if !resolved.exists() {
        miette::bail!("open: {}: No such file or directory", path);
    }
    opener::open(&resolved).map_err(|e| miette::miette!("open: {}: {}", path, e))?;
    Ok(PipelineData::empty())  // 无管道输出
}
```

## 4. 测试策略

| 层级 | 测试 | 方式 |
|---|---|---|
| 单元 | `open` 文件不存在 → 报错 | execute("open /no/such") |
| 单元 | `open` 无参数 → 报错 | execute("open") |
| 集成 | `open` 成功打开 → 不报错（**不实际验证 GUI 弹窗**，只验 spawn 成功） | tempdir + 真实文件 |
| 单元 | `show data.csv` → 表格（迁移自 Plan 001） | tempdir + execute + strip_ansi |
| 单元 | `show --as json` / 管道模式 / 错误（迁移自 Plan 001，改名） | 迁移现有测试 |
| help | `open --help` / `show --help` | execute 断言 |

### 关于 GUI 打开的测试局限
`opener::open` 会真启动 GUI 程序，无法在 CI 里断言"窗口弹出"。测试只验证：
- 文件存在时 `opener::open` **不返回错误**（spawn 成功）
- 文件不存在/无参数时**报错**

### TDD 流程
1. RED：`open` 文件不存在报错测试 → 失败（open 当前是解析命令）
2. 重写 open.rs 为 GUI → 通过
3. RED：`show data.csv` 表格测试 → 失败（show 不存在）
4. 新建 show.rs 迁移 Plan 001 逻辑 → 通过
5. 迁移 Plan 001 全部测试到 show（改名 open→show）
6. 全量回归

## 5. 实施步骤

1. **加依赖**：`Cargo.toml` 加 `opener = "0.8"`。
2. **新建 show.rs**：把 Plan 001 的解析逻辑从 open.rs 迁移过去（OpenCommand→ShowCommand，name 改 show，测试里 open→show）。
3. **重写 open.rs**：改为 GUI 打开（opener crate）。
4. **注册**：mod.rs 加 show；shell.rs 注册 OpenCommand（新）+ ShowCommand。
5. **TDD**：open 错误用例 + show 迁移用例 + help。
6. **全量回归**：cargo test。
7. 提交 + push。

## 6. 验收标准

- [ ] `open <真实文件>` 用系统默认程序打开（手动验证 GUI 弹出）
- [ ] `open /no/such` → 报错
- [ ] `open`（无参数）→ 报错
- [ ] `show data.csv` → 表格（原 open 行为完整迁移）
- [ ] `show --as json` / 管道模式 / 错误处理 全部正常
- [ ] `open --help` / `show --help` 正常
- [ ] 全量 cargo test 通过，无回归
- [ ] ash-core / auto-shell 无新增编译警告（除既有）

## 7. 风险与备注

- **破坏性变更**：`open` 的语义变了。已有脚本若用 `open file.csv | ...` 解析，需改成 `show file.csv | ...`。属于预期内的命名调整，commit message 明确说明。
- **GUI 测试局限**：无法在自动化测试里验证 GUI 窗口，靠手动验收 + spawn 成功断言。
- **opener 平台覆盖**：opener 在 Linux 上会尝试 `xdg-open` → `gio open` 等回退；Windows 用 ShellExecuteW。项目主平台是 Windows，已验证依赖。
