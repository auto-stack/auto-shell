# Plan 036: ash 脚本 Parity 测试框架

> **日期**: 2026-07-23
> **状态**: ✅ MVP 已完成 — 50/50 用例通过(strict 模式验证);后续工作见文末"Phase 2+"
> **目标**: 建立 ash 脚本与 bash/PowerShell/fish/nu 的 parity 测试，验证跨 shell 行为一致性
> **范围**: 50 个用例，Rust 集成测试框架，4 个参照 shell

## 愿景

ash 的脚本能力（AutoLang）目标是替代 bash/fish/nu/pwsh 各自的脚本。为了证明这一点，需要系统的 parity 测试——**同一逻辑用不同 shell 语言写，执行后比较输出是否一致**。

## 参照的 auto-lang parity 体系

auto-lang 有三套测试：
- **System A**（per-transpiler golden）：Auto 源码 → 转译输出，byte 比较
- **System B**（conformance）：Auto 源码 → AutoVM 执行，与 golden stdout 比较
- **System C**（parity workspace）：同一测试经三种后端执行，TAP 比较

ash 的 parity 参照 System C 的思路，但参照物不是"同一语言的多后端"，而是"同一逻辑的多 shell 语言"。

## 文件结构

```
ash/auto-shell/tests/parity/
├── README.md                    # 框架说明 + 用例索引
├── cases/                       # 50 个用例
│   ├── 01_echo/
│   │   ├── desc.md              # 描述
│   │   ├── ash.ash              # ash 版
│   │   ├── bash.sh              # bash 版
│   │   ├── pwsh.ps1             # PowerShell 版（可选）
│   │   ├── fish.fish            # fish 版（可选）
│   │   ├── nu.nu                # nushell 版（可选）
│   │   └── expected.txt         # golden output（bash 生成）
│   └── ...
└── (harness 在 tests/parity.rs 集成测试文件里)
```

## 用例分类（50 个）

### A. 基础命令（10 个）
01. echo 输出 | 02. 变量赋值与引用 | 03. 命令替换 | 04. 管道 | 05. 重定向
06. 退出码 | 07. 环境变量 | 08. 多命令串联(&&) | 09. 多命令串联(||) | 10. 分组命令

### B. 字符串操作（8 个）
11. 字符串拼接 | 12. 字符串长度 | 13. 子串提取 | 14. 字符串替换
15. 大小写转换 | 16. 分割字符串 | 17. 去空白 | 18. 字符串包含

### C. 条件与循环（10 个）
19. if-else | 20. if-elif-else | 21. for 遍历列表 | 22. for 范围
23. while 循环 | 24. break | 25. continue | 26. 嵌套循环
27. 文件存在测试 | 28. 字符串比较测试

### D. 函数（5 个）
29. 函数定义与调用 | 30. 函数参数 | 31. 函数返回值 | 32. 递归 | 33. 局部变量

### E. 文件操作（8 个）
34. 创建文件 | 35. 读取文件 | 36. 追加写入 | 37. 文件存在检查
38. 文件大小 | 39. 行数统计 | 40. 文件复制 | 41. 目录创建

### F. 数据处理（5 个）
42. grep 搜索 | 43. sort 排序 | 44. uniq 去重 | 45. head/tail | 46. wc 统计

### G. 错误处理（4 个）
47. try-catch vs trap | 48. 命令失败处理 | 49. 空输入处理 | 50. 数学运算

## Harness 设计

### 执行流程
```
对每个用例:
  1. Shell::execute(ash_content) → ash stdout
  2. Command("bash").arg(bash_script) → bash stdout
  3. normalize(ash_stdout) vs normalize(bash_stdout)
  4. assert_eq!
  5. pwsh/fish/nu（存在则比较，不存在跳过）
```

### 归一化规则
- 去 ANSI 颜色码
- CRLF → LF
- 去 trailing whitespace
- 绝对路径 → `<TMPDIR>`

### 跳过策略
- shell 未安装 → 跳过（不 fail）
- 用例 desc.md 可标 `skip_shells: [nu]`

## 实施步骤

1. harness.rs + normalize.rs
2. 3 个示范用例验证框架
3. 批量 50 个用例
4. CI 集成

---

## ✅ MVP 实施结果(2026-07-23)

50/50 用例全部通过(`cargo test --test parity` strict 模式 `ASH_PARITY_STRICT=1` 验证)。实施中发现并修复了 5 个阻塞(均非预期):

| # | 阻塞 | 修复 |
|---|------|------|
| R1 | `01_echo/ash.ash` 缺 `>` 前缀(shell 命令必须 `>` 开头) | 加 `>` 前缀 |
| R2 | 二进制定位用硬路径 `target/debug/ash` | 改用 `env!("CARGO_BIN_EXE_ash")` |
| R3 | exit-code 未参与对比 | runner 返回 `(stdout, exit_code)` + 对比 |
| R4 | shell 行输出每命令多一个尾部空行(`println!` 重复换行) | 新增 `print_command_output()`(shell.rs) |
| WSL | `run_bash` 在 cargo test 下解析到 WSL `System32\bash.exe`(能启动但无法执行 bash 脚本) | `resolve_bash()` 用 `echo $BASH_VERSION` 探测,Git bash 路径优先 |

**关键洞察**:50/50 全过是真实的,但当前 case 测的是**"AutoLang 逻辑等价性"**(42_grep/46_wc 等用 AutoLang 循环模拟,而非真实 `> grep`/`> wc`)。真正的 shell 命令在 ash 里仍输出结构化表格——这是 Phase 2(P1)要解决的。

相关 commit:`b72cf8b`(harness)、`6fbe0ff`(R4+WSL)、`b68f3a7`(docs)。

---

## Phase 2+: 后续工作

按优先级排序。每项含现状、已调研的实施路径、工作量。

### P1: 结构化命令 bash 兼容输出模式 ⭐最高优先级 — ✅ 已完成(2026-07-23)

**现状(已完成)**:ash 的 `ls`/`grep`/`wc`/`ps` 等输出 ratatui 表格,与 bash 纯文本不一致。现已新增 `--bash-compat` flag,让结构化命令输出 bash 风格纯文本。

**已实施**(commit 见下):
1. **shell.rs**:加 `bash_compat: bool` 字段 + 构造初始化 + `set_bash_compat` setter(照抄 `set_json_output`)
2. **ash-core/src/cmd/value_helpers.rs**:新增 `format_atom_as_bash(atom_type, value) -> Option<String>`,按 AtomType 分发(FileList→每行 name、MatchList→text 行/`ln:text`、CountResult→纯数字、ProcessList→`PID NAME`、Path→原样;其他→None)。含 6 个单测
3. **shell.rs `format_output`**:注入 `if self.bash_compat { ... }` 分支(json 之后、表格之前)
4. **main.rs**:加 `--bash-compat` flag(预扫描 + skip + `-s`/script 路径 set)
5. **execute_for_agent**:签名扩展为 `(input, json_mode, bash_compat)`,内部 set 并 reset
6. **parity harness**:`run_ash` 支持 `bash_compat` 参数;`ParityCase` 加 `bash_compat` 字段,case 目录放 `bash_compat` 空标记文件即启用
7. **新 case**:`51_ls_bash`/`52_grep_bash`/`53_wc_bash`(真实 `> ls`/`> grep`/`> wc` 命令,对比 bash)

**验收**:`cargo test --test parity`(strict)→ **53/53 全过**(原 50 + 新增 3)。value_helpers 单测 12/12 通过。execute_for_agent 单测 4/4 通过。

**实施中发现的相关 bug(已修复)**:
- ~~ash `> ls <glob>` 只取第一个匹配~~ → **已修复**:`ls` 现遍历全部位置参数(ls.rs `collect_ls_value`),`ls *.txt`/`ls a.txt b.txt` 列出所有文件
- ~~ash `>` 重定向捕获 `echo` 输出时多一个空行~~ → **已修复**:`apply_output_redirect` 改为仅当 output 不以 `\n` 结尾时才补换行(与 R4 同源;`echo hi > f` 现写入 `hi\n`,与 bash 一致)

### P2: 真实 shell 命令 case(依赖 P1)— ✅ 已完成(2026-07-24)

补真实命令版 case,用 `--bash-compat` 模式(case 目录放 `bash_compat` 标记)。已新增 9 个真实命令 case(51-59):
- 51_ls_bash(ls 单文件)、52_grep_bash(grep)、53_wc_bash(wc -w 管道)
- 54_grep_n(grep -n 带行号)、55_grep_c(grep -c 计数)、56_grep_i(grep -i 忽略大小写)
- 57_wc_l(wc -l 管道)、58_wc_c(wc -c 字节)、59_sort(sort 排序)

**验收**:`cargo test --test parity`(strict)→ **59/59 全过**(原 50 AutoLang + 9 真实命令)。

**探测发现不可 parity(留待后续)**:`ls -l`(ash bash-compat 未实现长格式)、`ls -a`(ash 不含 `.`/`..`)、`uniq`(ash 管道末端输出空,疑似 bug)。

### P3: fish/nu shell 变体覆盖

harness 的 `run_fish`/`run_nu` runner 和 best-effort WARNING 逻辑已就绪(parity.rs)。需为高价值 case 补 fish/nu 版本;无法对应的标 `skip_shells: [nu]`(需在 harness 加 skip 机制,读 `desc.md`)。

### P4: CI 集成

`parity_all_cases` 默认 warning 模式,需 `ASH_PARITY_STRICT=1` 才 fail。在 CI 加 `ASH_PARITY_STRICT=1 cargo test --test parity` 作门禁。需确保 CI 环境有 Git bash(Windows)或 bash(Linux)。

### P5: R4 交互式 REPL 回归验证

R4 修复(`print_command_output`)改了 `execute_script_content`/`execute_with_stdin` 输出路径。交互式 REPL 走不同路径(`Repl::run`),理论上不受影响,但未做交互模式回归。手动验证 `ash`(交互式)下 `echo hello`/`ls`/`cat file` 输出正常。

### 优先级建议
P1(核心缺口,解锁 P2)→ P2(真实命令 parity)→ P5(快速验证 R4 无副作用)→ P4(持续门禁)→ P3(锦上添花)
