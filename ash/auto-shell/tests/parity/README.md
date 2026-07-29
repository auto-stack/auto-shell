# ash 脚本 Parity 测试

验证 ash 脚本与 bash/PowerShell 的输出一致性。同一逻辑用不同 shell 语言编写,执行后比较 stdout + exit-code 是否一致。

- 设计文档:`designs/036-script-parity.md`
- 实施计划:`docs/plans/036-script-parity.md`
- 框架来源:Plan 036(50 case 骨架,扩展至 79 case)

## 运行

```bash
# 前置:构建 ash 二进制(cargo test 会通过 CARGO_BIN_EXE_ash 自动构建)
cd ash && cargo test --test parity

# 严格模式:把分歧作为测试失败(用于 CI)
ASH_PARITY_STRICT=1 cargo test --test parity

# 从 bash 重新生成所有 expected.txt(golden)
cargo test --test parity -- bootstrap_expected --nocapture
```

**当前状态:79/79 通过(0 skip)**(`✓ All 79 parity cases passed`,strict 模式验证)。原 74/80 钉住的 find/system 桥缺陷已修复(见 docs/plans/036 "Phase 4 后续:缺陷修复")。

## 用例编写规范

每个 ash 用例必须遵守(详见 designs/036 §3.1):

- **shell 命令以 `>` 前缀**:bash `echo x` → ash `> echo x`(无 `>` 的行被当作 AutoLang 代码)
- **命令捕获**:bash `x=$(cmd)` → ash `var x = > cmd`(注意 `.trim()` 尾部空行)
- **AutoLang 逻辑用原生语法**:`var` / `print()` / `if {}` / `for in` / `fn` / `while`
- **纯文本优先**:避免 wc/grep/head/tail/awk(ash 里结构化或缺失);计数用 AutoLang `for l in s.lines()` 循环

### 标记文件(每个 case 目录下)

- **`bash_compat`**(空文件):用了 `> ls`/`> grep`/`> wc` 等结构化命令时**必须加**,让 harness 给 ash 传 `--bash-compat`,输出 bash 风格纯文本而非 ratatui 表格。漏加会导致 ash 输出表格、parity 失败(见 docs/plans/036 Phase 4 的 77 调试插曲)。
- **`skip`**(内容=跳过原因):已知缺陷导致 ash 跑不通时加,harness 跳过该 case 并打印原因。缺陷修复后**删除此文件**即自动纳入回归。
- `pwsh.ps1`/`fish.fish`/`nu.nu`(可选,best-effort WARNING,不 fail)。

## 关键实现细节(避免回归)

- **`resolve_bash()`**:Windows 上 `Command::new("bash")` 在 `cargo test` 进程会解析到 WSL `System32\bash.exe`(能启动但无法执行 bash 脚本语法)。harness 用 `echo $BASH_VERSION` 探测,完整 Git bash 路径优先,`OnceLock` 缓存。
- **`print_command_output()`**(shell.rs):shell 行输出已带 `\n` 时用 `print!`,否则 `println!`,避免重复换行。
- **normalize**:去 ANSI / CRLF→LF / trim trailing / 绝对路径 → `<TMPDIR>` 占位。
- **cwd 隔离**:每个 case 在独立 temp 目录 `%TEMP%/ash_parity_<name>` 跑,ash 与 bash 各用全新目录,避免文件污染。

## 已知差异(用 skip 钉住的缺陷)

**无。** 79 个 case 全部通过。

原由 case 74/80 钉住的 4 个 find/system 桥缺陷已全部修复(`execute_capture` 让 system 桥返回纯文本;find 的 `-name`/`-type` 改为带值 option;find 路径格式与 bash 对齐;FileList 渲染回退 `path` 字段)。详见 docs/plans/036 "Phase 4 后续:缺陷修复"。

遗留的非阻塞项(未影响任何用例,记录待后续):
- 单横杠长选项(`-type`):ash parser 只认 `--type`/`-t`,case 用短选项 `-t` 规避。
- 外部命令(date/gzip)在 system 桥下捕获为空(`capture_output=false`):独立任务。

## 验收

79/79 通过(0 skip),超出原 MVP 目标(≥25)。详见 designs/036 §4、docs/plans/036 各 Phase。未来扩展(fish/nu、更多结构化命令)见 designs/036 §6。
