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

### P1: 结构化命令 bash 兼容输出模式 ⭐最高优先级

**现状**:ash 的 `ls`/`grep`/`wc`/`ps` 等输出 ratatui 表格,与 bash 纯文本不一致。实测验证:
- `> grep apple f.txt` → 表格(带路径列),bash 是 `apple\napple`
- `> cat f | wc -l` → `lines: 3`,bash 是 `3`
- `> ls` → 五列表格,bash 是 `file1\nfile2`

当前 50 case 全用 AutoLang 模拟规避了这点,但**真正的 shell 命令 parity 缺失**。

**实施路径**(已精确调研,照抄 `--json` 三件套):
1. **shell.rs**:加 `bash_compat: bool` 字段(行 136 后)+ 构造初始化 + `set_bash_compat` setter(照抄 `set_json_output` 行 936)
2. **新建 `ash-core/src/cmd/bash_compat.rs`**:写 `format_atom_as_bash(atom) -> Option<String>`,按 `AtomType` 分发:
   - `FileList`/`FileEntry` → 每行一个 name(`ls` 默认)
   - `ProcessList`/`ProcessEntry` → 经典 `ps` 列
   - `MatchList` → `file:line:content`(grep -n)或纯行
   - `CountResult` → 纯数字 + `\n`(wc 风格)
   - `Path` → 原样字符串
   - 其他 → `None`(落 fallback `into_text`)
   - 复用 `format_value_for_table`(value_helpers.rs:164)提取字段,**不复用** `format_array_as_table`(带表头)
3. **shell.rs `format_output`**(行 885 后):注入分支 `if self.bash_compat { ... }`,在 json 之后、表格之前
4. **main.rs**:加 `--bash-compat` flag(照抄 `--json` 预扫描行 40 + skip 行 53 + 三处应用)。注意 `-c` 路径的 `execute_for_agent` 会 reset 字段,需扩签名或改 set+execute
5. **parity case**:加 `51_ls_bash`/`52_grep_bash`/`53_wc_bash`(用 `--bash-compat` 跑 ash,对比真实 bash)

**工作量**:核心 < 150 行,触及 3 文件(shell.rs、main.rs、新格式器)。**验收**:新增 case 通过 + 现有 50 case 不回归。

### P2: 真实 shell 命令 case(依赖 P1)

补真实命令版 case:`> ls`/`> ls -l`/`> grep pattern file`/`> wc -l`/`> ps`/`> find`。ash 版用 `--bash-compat` 模式。预计 10-15 个新 case。

### P3: fish/nu shell 变体覆盖

harness 的 `run_fish`/`run_nu` runner 和 best-effort WARNING 逻辑已就绪(parity.rs)。需为高价值 case 补 fish/nu 版本;无法对应的标 `skip_shells: [nu]`(需在 harness 加 skip 机制,读 `desc.md`)。

### P4: CI 集成

`parity_all_cases` 默认 warning 模式,需 `ASH_PARITY_STRICT=1` 才 fail。在 CI 加 `ASH_PARITY_STRICT=1 cargo test --test parity` 作门禁。需确保 CI 环境有 Git bash(Windows)或 bash(Linux)。

### P5: R4 交互式 REPL 回归验证

R4 修复(`print_command_output`)改了 `execute_script_content`/`execute_with_stdin` 输出路径。交互式 REPL 走不同路径(`Repl::run`),理论上不受影响,但未做交互模式回归。手动验证 `ash`(交互式)下 `echo hello`/`ls`/`cat file` 输出正常。

### 优先级建议
P1(核心缺口,解锁 P2)→ P2(真实命令 parity)→ P5(快速验证 R4 无副作用)→ P4(持续门禁)→ P3(锦上添花)
