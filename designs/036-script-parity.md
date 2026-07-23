# 设计文档 036: ash 脚本 Parity 测试套件(MVP)

> **日期**: 2026-07-23
> **状态**: ✅ MVP 已实施 — 50/50 case 通过(strict 模式验证),超出验收目标(原 ≥25)
> **关系**: 细化并扩展 [plans/036-script-parity.md](../plans/036-script-parity.md);本套件在 Plan 036 已建的框架骨架上落地
> **目标**: 为 ash 的"跨平台脚本替代 bash/powershell"能力建立可执行、可验证的 parity 测试套件

---

## 1. 背景与目标

ash (AutoShell) 的脚本能力(AutoLang + `>` shell-bridge)目标是**替代 bash/fish/nu/powershell/cmd 各自的脚本**,提供跨平台一致的脚本语言。为证明这一点,需要系统的 **parity 测试**:同一逻辑用不同 shell 语言编写,执行后比较输出是否一致。

参照 auto-lang 的三套 parity 体系(producer/consumer/python,基于 TAP 三方对比),ash 的 parity 模型本质不同——ash 直接执行 `.ash` 脚本,与 bash/powershell 的 stdout/exit-code 对比,无 transpiler 参与。

**本套件的 MVP 目标**:
- 建立 32 个纯文本输出场景的 parity 用例
- 验证 ash 与 bash 的 stdout + exit-code 一致性
- powershell 走"功能/内容一致"放宽判定(因 ash 跨平台,bash/pwsh 输出本就不完全一致)
- 形成可扩展的骨架,为后续扩充奠定基础

---

## 2. 现状调研结论(已实测验证)

### 2.1 已有资产(Plan 036 已建)

- `ash/auto-shell/tests/parity.rs` — harness 骨架,**方案 A(子进程)已实现**:
  - `run_ash`(parity.rs:71)用 `Command::new(bin).arg(script_path)` 子进程执行
  - `ash_binary_path()`(parity.rs:81)定位 `target/debug/ash`,支持 `ASH_TEST_BIN` 覆盖
  - `run_bash`/`run_pwsh`/`run_fish`/`run_nu` 跨 shell runner(对称子进程模型)
  - `normalize()`(parity.rs:23)去 ANSI / CRLF→LF / trim trailing
  - `discover_cases()` / `run_parity_case()` / `bootstrap_expected()` 测试入口
- `ash/auto-shell/tests/parity/cases/01_echo/` — 唯一示范 case(纯文本)

**结论:harness 骨架基本就绪,B1(stdout 捕获)阻塞其实已解决。** 最初调研读到的"run_ash 返回空串"是陈旧副本,当前版本已是子进程方案。

### 2.2 真实阻塞(5 个,均已定位并修复)

| # | 阻塞 | 根因(实测) | 修复 |
|---|------|------------|------|
| **R1** | `01_echo/ash.ash` 跑不出输出 | ash 脚本模型:shell 命令**必须以 `>` 前缀**(shell.rs:2330 `if trimmed.starts_with('>')`)。现有 case 写成纯 bash 风格 `echo "hello"`(无 `>`),被当作 AutoLang 代码处理,`echo` 不是合法语句故输出被吞。 | 用例编写规范问题,非代码 bug。见 §3.1 规约。 |
| **R2** | 二进制定位脆弱 | `parity.rs:81` 用硬路径 `target/debug/ash`,要求手动 `cargo build`。 | 改用 `env!("CARGO_BIN_EXE_ash")`(cargo 自动构建定位)。 |
| **R3** | exit-code 未参与对比 | `run_ash` 只回 stdout,`run_parity_case` 只比 stdout。 | runner 返回 `(stdout, exit_code)`,对比逻辑加 exit-code 比对。 |
| **R4**(实施中发现) | shell 行输出每命令多一个尾部空行 | `execute_script_content` 用 `println!("{}", output)` 打印,而 `echo` 等命令输出已带 `\n`,导致重复换行。影响几乎所有用 `> echo`/`> cat` 的 case。 | 新增 `print_command_output()`(shell.rs):输出已带 `\n` 用 `print!`,否则 `println!`。 |
| **WSL bash**(实施中发现) | `run_bash` 在 `cargo test` 下返回垃圾/空 | Windows 上 `Command::new("bash")` 在测试进程解析到 **WSL `System32\bash.exe`**,能启动(exit 0 ≠ 127)但无法执行 bash 脚本语法(输出垃圾)。旧逻辑用 `code != 127` 选候选,在第一个(WSL)就返回了垃圾。 | `resolve_bash()`:用 `echo $BASH_VERSION` 探测,Windows 上完整 Git bash 路径优先,`OnceLock` 缓存;gate 改为 `resolve_bash().is_some()`。 |

**R4 + WSL bash 是 37 个分歧的真正根因**(非 echo 语法、非 VM bug)。两者修复后 **50/50 全过**。

### 2.3 关于结构化命令输出(B2)

ash 的结构化命令(`ls`/`ps`/`sys` 等)输出是 ratatui 表格(atom/batom),与 bash 的纯文本 `ls` 不一致。**本轮 MVP 不做"bash 兼容输出模式"**,case 全部选用纯文本输出场景——实测确认 `print()`/`> echo`/`> cat`/`> grep`/`> sort`/`> wc`/变量/算术等都走纯文本路径,天然与 bash 一致。结构化命令留作后续独立子任务。

### 2.4 已知 VM bug(影响部分 case)

来自 designs/034 附录 B(2026-07-23),5 个已知 VM bug 中影响本套件的主要是:
- **Bug 1**: `.to_uint()` 在算术中返回垃圾值(auto-lang codegen bug)——影响数值计算类 case。规避:算术优先走 `> echo $((...))` shell 路径,而非 AutoLang `.to_uint()`。
- Bug 2/3/4 已在 Plan 034 修复(`> cmd` in fn body、var 捕获、`$@` 传播)。
- Bug 5: `cat f | from_json` 失败——影响 jq-like 类,本轮不涉及。

受影响的 case 标 `KNOWN_FAIL`,记入 README 已知差异表,不阻塞套件建立。

---

## 3. 设计

### 3.1 ash 脚本编写规范(每个 ash.ash 必须遵守)

实测验证的 ash 脚本执行模型(shell.rs:2282 `execute_script_content`),规约如下:

**规约 1:shell 命令必须以 `>` 前缀**
| bash | ash |
|------|-----|
| `echo "hello"` | `> echo "hello"` |
| `cat file` | `> cat file` |
| `grep foo f.txt` | `> grep foo f.txt` |

不带 `>` 的行被当作 AutoLang 代码。

**规约 2:命令输出捕获用 `var x = > cmd`**
- bash: `x=$(echo hi)` → ash: `var x = > echo hi`(捕获后含尾部空行,需 `.trim()`)
- bash: `` x=`echo hi` `` → ash: `var x = > echo hi`

**规约 3:纯 AutoLang 逻辑用原生语法**
- 变量:`var name = "value"`(无 `$`)
- 输出:`print("...")`(非 `echo`)
- 拼接:`"a" + b + "c"`
- 条件:`if cond { } else { }`
- 循环:`for x in coll { }`、`while cond { }`
- 函数:`fn name(params) { return v }`
- 算术:`var n = 1 + 2`(纯 AutoLang)或 `> echo $((1+2))`(走 shell,规避 Bug 1)

**规约 4:管道与重定向走 shell 行**
- bash: `cat f | grep x | wc -l` → ash: `> cat f | grep x | wc -l`
- bash: `echo hi > file` → ash: `> echo hi > file`

**规约 5:工作目录隔离**
每个 case 在独立的临时工作目录里跑(ash 和 bash 同一个 tmp dir),避免 case 间污染、避免相对路径歧义。harness 负责创建+清理。

**规约 6:跨平台可移植**
- bash 版可在 Unix 上跑;ash 版**必须 Windows + Unix 都能跑**(ash 跨平台的核心价值)。
- 避免硬编码 `/tmp`;用相对路径或 harness 注入的工作目录。

### 3.2 等价判定标准(parity 定义)

按用户决策:
- **ash vs bash**:stdout 精确一致(经 `normalize`)+ exit-code 精确一致。最严格。
- **ash vs powershell**:功能与内容一致即可,不要求 stdout 完全一致(因 ash 跨平台,bash/pwsh 输出本就不一致)。走 best-effort WARNING(不 fail),与现有 `run_parity_case` pwsh 逻辑一致。
- **stderr 不参与**(各 shell 诊断信息不同是正常的)。

`normalize` 规则(parity.rs:23 已实现):去 ANSI → CRLF→LF → trim trailing whitespace per line → trim 首尾换行。本轮新增:**绝对路径 → `<TMPDIR>` 占位**(规避临时目录差异)。

### 3.3 case 分类清单(MVP 32 个,纯文本输出)

全部选用纯文本输出场景(排除 ls/ps 等结构化命令)。每个 case 目录结构:
```
tests/parity/cases/<NN_name>/
├── desc.md       # 用例描述 + skip 标注(如 skip_shells: [nu])
├── ash.ash       # ash 版(守 §3.1 规约)
├── bash.sh       # bash 版(golden 基准)
├── pwsh.ps1      # powershell 版(放宽判定)
└── expected.txt  # golden output(由 bootstrap_expected 从 bash 生成)
```

**A. 基础命令与 IO(6)**
1. `01_echo` — echo 多行输出(修正现有,加 `>` 前缀)
2. `02_var_print` — 变量定义 + print 输出
3. `03_cmd_capture` — 命令输出捕获(var x = > cmd)
4. `04_pipe` — 管道(cat | grep | wc)
5. `05_redirect` — 重定向写文件 + 读回
6. `06_exit_code` — exit(code) + 进程退出码一致

**B. 字符串操作(6)**
7. `07_str_concat` — 字符串拼接
8. `08_str_len` — 字符串长度(.len() / ${#x})
9. `09_str_sub` — 子串提取
10. `10_str_replace` — 字符串替换
11. `11_str_case` — 大小写转换(走 shell `tr` 或 awk)
12. `12_str_split` — 分割字符串(走 shell IFS/cut)

**C. 条件与循环(8)**
13. `13_if_else` — if/else 分支
14. `14_if_elif` — if/elif/else 多分支
15. `15_for_list` — for 遍历列表
16. `16_for_range` — for 范围(range / seq)
17. `17_while` — while 循环计数
18. `18_break` — break 中断
19. `19_continue` — continue 跳过
20. `20_nested_loop` — 嵌套循环

**D. 函数(4)**
21. `21_func_def` — 函数定义与调用
22. `22_func_args` — 函数参数
23. `23_func_return` — 函数返回值
24. `24_recursion` — 递归(阶乘)

**E. 文件操作(4)**
25. `25_file_write_read` — 写文件 + 读回
26. `26_file_append` — 追加写入
27. `27_file_exists` — 文件存在检查
28. `28_file_count_lines` — 统计行数(wc -l)

**F. 文本数据处理(4)**
29. `29_grep` — grep 过滤
30. `30_sort_uniq` — sort | uniq 去重统计
31. `31_head_tail` — head/tail 截取
32. `32_text_pipeline` — 综合管道(日志分析:grep+sort+uniq+head)

**本轮明确排除**(避免 B2/VM bug):
- ls/ps/sys 结构化表格输出(需 bash 兼容模式,本轮不做)
- `.to_uint()` 相关算术(VM Bug 1;算术优先走 `> echo $((...))` 规避)
- `use.py`/`use auto.*` 库消费(超出"脚本能力"范围)
- 复杂 heredoc 多行注入(易触发边界 bug)

---

## 4. MVP 验收标准

> **注:这是 MVP 验收标准。** 未来需要扩充到更多用例(覆盖 ls/ps 等结构化命令、fish/nu shell 变体、错误处理 G 类等),且目标覆盖全部用例跑通(届时需先修复挡路的 VM bug)。本 MVP 套件为后续扩充奠定可扩展骨架。

**✅ 实施结果(2026-07-23):全部达成,且超出预期。**

- [x] harness 增强:R2(`CARGO_BIN_EXE_ash` 定位)+ R3(exit-code 对比)+ normalize 路径占位
- [x] `01_echo` 修正为守 `>` 规约,实测跑通
- [x] **50 个 case 全部通过**(Plan 036 已建全套,本计划验证并修复使其全过;原计划 32 个的目标被实际 50 个超越)
- [x] **50/50 实测跑通**(ash vs bash stdout + exit-code 一致,strict 模式 `ASH_PARITY_STRICT=1` 验证)
- [x] ~~其余标 KNOWN_FAIL~~ — 无需,全部通过,0 个 KNOWN_FAIL
- [x] `cargo test --test parity` 通过(`✓ All 50 parity cases passed`)

**实施中额外修复的 2 个关键阻塞**(详见 §2.2):
- **R4**:ash shell 行输出重复换行 bug(`print_command_output` 修复)
- **WSL bash 误选**:`run_bash` 在 cargo test 下解析到 WSL bash 导致 37 个假分歧(`resolve_bash` 探测修复)

---

## 5. 实施步骤(概要,详见后续 writing-plans)

1. **harness 增强**(R2 + R3):改 `parity.rs` 二进制定位 + runner 返回 exit-code + 对比逻辑
2. **normalize 增强**:加绝对路径 → `<TMPDIR>` 占位
3. **修正 01_echo** + 建测试目录 README/索引
4. **批量写 case**:A→F 六类,每类先写 bash.sh 跑通生成 expected,再写 ash.ash 实测对齐
5. **逐个验证**:实测 ash 子进程输出,对齐 normalize 后的 expected;跑不通的标 KNOWN_FAIL
6. **全量回归**:`cargo test --test parity`,确保通过

---

## 6. 未来扩展(超出本 MVP)

> 实施后更新(2026-07-23):本 MVP 50/50 全过,但测的是"AutoLang 逻辑等价性"而非"shell 命令 parity"。后续工作的详细实施路径已写入实施计划 `docs/superpowers/plans/2026-07-23-ash-script-parity-testsuite.md` 的"后续工作(Phase 2+)"章节。优先级:P1 > P2 > P5 > P4 > P3。

- **P1:结构化命令 bash 兼容输出模式**(最高优先级,已调研实施路径)— `ls`/`grep`/`wc`/`ps` 等仍输出 ratatui 表格。新增 `--bash-compat` flag(照抄 `--json` 三件套)+ per-AtomType 经典格式器(FileList→每行 name、CountResult→纯数字)。核心 < 150 行,触及 shell.rs/main.rs/新格式器。
- **P2:真实 shell 命令 case**(依赖 P1)— 补 `> ls`/`> grep`/`> wc` 等真实命令版 case(当前 50 case 全用 AutoLang 模拟)。
- **P5:R4 交互式 REPL 回归验证** — 验证 `print_command_output` 修复对交互模式无副作用。
- **P4:CI 集成** — `ASH_PARITY_STRICT=1 cargo test --test parity` 作为 CI 门禁。
- **P3:fish/nu shell 变体** — harness runner 已就绪,需补 case 文件 + skip 机制。
- **(G 类错误处理已覆盖)**:47_try_catch/48_cmd_fail/49_empty_input 已在 50 case 内,全过。
