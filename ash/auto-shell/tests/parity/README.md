# ash 脚本 Parity 测试

验证 ash 脚本与 bash/PowerShell 的输出一致性。同一逻辑用不同 shell 语言编写,执行后比较 stdout + exit-code 是否一致。

- 设计文档:`designs/037-script-parity-testsuite.md`
- 实施计划:`docs/superpowers/plans/2026-07-23-ash-script-parity-testsuite.md`
- 框架来源:Plan 036(50 case 骨架)

## 运行

```bash
# 前置:构建 ash 二进制(cargo test 会通过 CARGO_BIN_EXE_ash 自动构建)
cd ash && cargo test --test parity

# 严格模式:把分歧作为测试失败(用于 CI)
ASH_PARITY_STRICT=1 cargo test --test parity

# 从 bash 重新生成所有 expected.txt(golden)
cargo test --test parity -- bootstrap_expected --nocapture
```

**当前状态:50/50 通过**(`✓ All 50 parity cases passed`,strict 模式验证)。

## 用例编写规范

每个 ash 用例必须遵守(详见 designs/037 §3.1):

- **shell 命令以 `>` 前缀**:bash `echo x` → ash `> echo x`(无 `>` 的行被当作 AutoLang 代码)
- **命令捕获**:bash `x=$(cmd)` → ash `var x = > cmd`(注意 `.trim()` 尾部空行)
- **AutoLang 逻辑用原生语法**:`var` / `print()` / `if {}` / `for in` / `fn` / `while`
- **纯文本优先**:避免 wc/grep/head/tail/awk(ash 里结构化或缺失);计数用 AutoLang `for l in s.lines()` 循环

## 关键实现细节(避免回归)

- **`resolve_bash()`**:Windows 上 `Command::new("bash")` 在 `cargo test` 进程会解析到 WSL `System32\bash.exe`(能启动但无法执行 bash 脚本语法)。harness 用 `echo $BASH_VERSION` 探测,完整 Git bash 路径优先,`OnceLock` 缓存。
- **`print_command_output()`**(shell.rs):shell 行输出已带 `\n` 时用 `print!`,否则 `println!`,避免重复换行。
- **normalize**:去 ANSI / CRLF→LF / trim trailing / 绝对路径 → `<TMPDIR>` 占位。

## 已知差异(MVP)

**无。** 50 个 case 全部通过。

(如未来新增 case 出现分歧,在此记录格式:`NN_xxx` — 原因:...)

## MVP 验收

50/50 通过,超出原目标(≥25)。详见 designs/037 §4。未来扩展(结构化命令 bash 兼容模式、fish/nu、G 类错误处理)见 designs/037 §6。
