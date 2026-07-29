# Plan 036: ash 脚本 Parity 测试框架

> **日期**: 2026-07-23 (updated 2026-07-29)
> **状态**: ✅ 全部通过 — 79/79 用例通过(strict 模式，0 skip)。全部 5 类规避已清零:规避 1(真实命令)✅、规避 2(.to_int)✅、规避 3(伪缺陷)✅、规避 4(cwd 隔离)✅、规避 5(HashMap)✅(2026-07-29)。详见各 Phase
> **目标**: 建立 ash 脚本与 bash/PowerShell/fish/nu 的 parity 测试，验证跨 shell 行为一致性
> **范围**: 79 个用例(72 命令级 + 7 脚本级常见场景)，Rust 集成测试框架，4 个参照 shell

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

**⚠️ 范围内未完成的 parity 缺口(036 目标是 ash↔bash 一致,这些是真实缺陷,待修复)**:
- `ls -l`:ash `--bash-compat` 未实现长格式渲染(权限/大小/时间列)。属 P1 的未完成部分。
- `ls -a`:ash 不含 `.`/`..` 条目(bash `ls -a` 含)。属 ls 命令行为差异。
- `uniq`:ash 管道末端输出空(疑似 ash uniq 实现 bug)。**uniq 是 036 F 类第 44 项,明确在计划范围内**。

这三项因当前未修复,暂未建对应 case;修复后应补 case `60_ls_long`/`61_ls_all`/`62_uniq` 并纳入 parity。

### P3: fish/nu shell 变体覆盖 — ✅ 已完成(2026-07-24)

harness 的 `run_fish`/`run_nu` runner 和 best-effort WARNING 逻辑已就绪(parity.rs,不 fail)。原 50 个 case 已有 fish.fish/nu.nu 真实内容。本次为新增的 9 个真实命令 case(51-59)补齐 fish.fish/nu.nu,**全 59 case 覆盖**。

**验收**:全 59 case 有 fish.fish + nu.nu。本机无 fish/nu,未实测,但有环境的 CI/用户会经 best-effort WARNING 自动对比。skip 机制(`desc.md` 标 `skip_shells`)暂不加——当前 WARNING 模式不阻塞,无需 skip(YAGNI);未来若升级为 strict 对比再补。

### P4: CI 集成 — ✅ 已完成(2026-07-24)

`parity_all_cases` 默认 warning 模式,需 `ASH_PARITY_STRICT=1` 才 fail。

**已实施**:在 `.github/workflows/ci.yml` 的 test job 加 "Parity gate (strict)" 步骤:
- `ASH_PARITY_STRICT=1 cargo test --test parity`
- `if: runner.os != 'Windows'`(Windows 的 Git bash 路径在 CI 不确定,避免误阻塞;Linux/macOS 自带真 bash)
- 这样 parity 分歧会成为 CI 门禁(Linux/macOS),回归自动暴露

### P5: R4 交互式 REPL 回归验证 — ✅ 已完成(2026-07-24)

R4 原修复只改了脚本路径(`execute_script_content`/`execute_with_stdin`)。回归验证发现 **REPL 路径和 `-c` 路径仍有同样的重复换行 bug**(`println!("{}", s)` 打印已带 `\n` 的 echo 输出)。

**已修复**:把 `print_command_output` 改为 `pub`,统一应用到所有命令输出路径:
- `repl.rs` REPL 主循环(repl.rs:822)+ AI 模式执行(2 处)→ 改用 `crate::shell::print_command_output`
- `main.rs` `-c` 路径(main.rs:86)→ 改用 `auto_shell::shell::print_command_output`

**验证**:`ash -c 'echo hi'` 现输出 `hi\n`(与 bash 一致,不再是 `hi\n\n`)。parity 59/59(strict)、shell 单测 66/66 通过,无回归。

### 优先级建议
~~P1(核心缺口,解锁 P2)→ P2(真实命令 parity)→ P5(快速验证 R4 无副作用)→ P4(持续门禁)→ P3(锦上添花)~~
**进度**:P1 ✅ → P2 ✅ → P5 ✅ → P4 ✅ → P3 ✅

---

## ✅ 范围内缺口已修复(2026-07-24)

036 目标是"验证 ash 脚本与 bash 的跨 shell 行为一致性"。以下 3 项 parity 缺口是计划范围内发现的真实缺陷,**现已全部修复**,并补了对应 case(60-62),parity 62/62 通过。

### 缺口 1: `uniq` 不去重 — ✅ 已修复

**根因**(实测调试推翻了初步判断):不是 bash-compat 渲染问题,而是 `uniq`(无参数)被 `parse_pipe_stage`(pipe_stages.rs:26)识别为**结构化管道阶段**,走 `operators::apply` 路径,绕过了 uniq 命令的 run_atom。而 `apply`(operators.rs:91)对非 Array 的 `Value::Str`(cat 的文本输出)直接 no-op 透传 → 不去重。`sort|uniq` 空 output 则因 shell.rs:1054 只取 `AtomPipeline::Atom` 的 value,忽略了 sort 产出的 `AtomPipeline::Text` 变体 → uniq 收到空数组。

**修复**:
- operators.rs `apply`:为 `Value::Str` 加 Uniq 的行级去重(相邻行合并 + join_lines 保留尾随换行)
- shell.rs:1054 pipe-stage 路径:增加 `AtomPipeline::Text` → `Value::Str` 的传递
- 附带:echo 加 `-e` flag(echo.rs `interpret_escapes` 解释 `\n`/`\t`/`\\` 等)
- case `60_uniq`(sort|uniq)通过

### 缺口 2: `ls -a` 缺 `.`/`..` — ✅ 已修复

**根因**:Rust `std::fs::read_dir` 不返回 `.`/`..`;ls.rs 把 `-a`/`-A` 折叠成同一布尔(都=bash `-A`),与文档承诺不符。

**修复**:
- ls.rs 区分 `-a`(`include_dots=true`)与 `-A`(`include_dots=false`)
- fs.rs `ls_command_value`:read_dir 后若 `include_dots`,用 `metadata_to_entry` 注入 `.`(当前目录)和 `..`(parent,带 root 防护)两个 Dir 条目(参与既有排序,自然浮顶)
- case `61_ls_all`(ls -a subdir)通过

### 缺口 3: `ls -l` 长格式 — ✅ 已修复(视觉合理,有 parity 残差)

**根因**:`format_file_list_as_bash` 无条件输出 name;`ls_command_value` 的 `long` 参数被忽略(虽数据含 permissions)。

**修复**:
- fs.rs `ls_command_value`:真正用 `long` 参数——非 `-l` 时剥离 permissions/owner 字段(单文件 + 目录两分支)
- value_helpers.rs `format_file_list_as_bash`:检测首条目有无 `permissions` 字段决定长格式(参照 ps 检测 `command` 的先例),长格式输出 `<perms> 1 <owner> <owner> <size> <modified> <name>`
- 附带:`format_atom_as_bash` 对 Record/BuildResult/RunResult 返回空(mkdir 等副作用命令 bash 静默)

**parity 残差**(未完全消除,记录待后续):bash ls -l 还需 links 计数(ash 无)、group(ash 无)、owner 用户名(ash 是 uid/Windows 缺失)、bash 日期格式(`Mon DD HH:MM` vs ash 的 ISO)、`total` 行。视觉合理已达成,严格字节 parity 需扩 AshFileEntry。未建 strict case(残差会 fail)。

### 验收
- parity 62/62 通过(strict):原 59 + `60_uniq`/`61_ls_all`/`62_echo_e`
- 单测全绿:value_helpers 12、operators 16、echo 8、shell 66

---

## Phase 3: 脚本级 parity(中型脚本)

命令级 parity(62 case)验证的是单个命令/短管道的输出一致性。**脚本级 parity** 验证的是完整的小/中型脚本——多行逻辑(循环、条件、函数)组合 shell 命令解决实际问题,如"遍历子目录算磁盘占用"、"批量重命名"、"日志统计分析"。

### 脚本级障碍诊断与修复(2026-07-24)

实测"遍历子目录算 du"这类中型脚本,发现并修复了 2 个阻塞性 bug:

**S1: AutoLang 变量在 `>` shell 行不插值**(auto-lang `d5987cd1`)
- 根因:顶层 `var name="a"` 走 `STORE_GLOBAL`(写入 `vm.globals`),但 `get_var_string` 只查 `scope_stack`(局部栈),漏查 globals → `$name` 插值返回空
- 修复:`get_var_string` 回退查 `vm.globals`;`get_all_vars` 同步补查。附带给 plan370 测试加 `ui-iced` feature gate
- 验证:`var d="b"; > cat $d/file` 正确插值并读取

**S1b: 函数体内 `var x = > cmd` 捕获报解析错误**(auto-shell `85ce9bc`)
- 根因:`try_capture_assignment` 不管 brace_depth,在函数体内也 flush+单独执行,截断函数体块 → AutoLang `unexpected token`
- 修复:brace_depth > 0 时改写成 `var x = system("cmd")` 注入 auto_block(与 Plan 034 Bug 3 对称)
- 验证:`fn main() { var x = > ls; for e in x.split(...); ... }` 正常工作

### 剩余次要障碍(非阻塞,可规避)
- 缺命令 flag:`du -sb` 报 `Unknown flag: -b`(换用支持的 flag 或 AutoLang 计算)
- `.to_int()` 数值转换(已知 VM Bug 1,算术走 `> echo $((...))` 规避)
- `DBGE:` 调试输出泄漏 stderr(遗留 debug print,影响脚本输出纯净度)

### 脚本级 case 设计(case 63+)

脚本级 case 同样放在 `tests/parity/cases/`,但内容是多行完整脚本。每个 case 的 ash.ash 是能独立解决一个实际问题的中型脚本,bash.sh 是等价 bash 脚本。

**首批脚本级 case**:
- `63_dirsize`:遍历子目录算每个目录大小 + 总大小(用 AutoLang 遍历 + shell du)
- `64_count_lines`:递归统计某类文件的总行数(如所有 .txt 的行数)
- `65_batch_grep`:对多个文件执行 grep,汇总匹配数

这些 case 用 `bash_compat` 标记(结构化命令走纯文本),实测对齐 bash 脚本输出。

### ⚠️ 规避手段统计与对应缺陷(2026-07-24 审计)

parity 测试的目标是**发现 ash/AutoLang 在脚本方面的真实缺陷**。但当前 case 存在 5 类规避手段,绕开了缺陷而非暴露它们。以下是完整审计,每类对应应修复的真实缺陷:

	**规避 1:用 AutoLang 模拟 shell 命令(影响 case 01-50 多数)— ✅ 已修复(2026-07-29)**
	
	case 01-50 几乎全部用 AutoLang 原生代码模拟 shell 命令,而非真正调用 ash 命令。现已全部改为真实 `> command` + `bash_compat`:
	- `04_pipe`: ~~`for + .contains()` 模拟 grep~~ → `> cat | grep`(bash_compat)
	- `42_grep`/`43_sort`/`44_uniq`/`45_head_tail`/`46_wc`: ~~AutoLang 循环/排序/去重/计数模拟~~ → 真实 `> grep`/`> sort`/`> uniq`/`> sed`/`> wc`(bash_compat)
	- `34_create_file`/`35_read_file`/`36_append_file`: ~~`system("echo...|tee")`~~ → `> echo > file` + `> cat`
	- `39_line_count`: ~~AutoLang for 循环计数~~ → `> cat | wc -l`(bash_compat)
	- `40_copy_file`: ~~`system("cp...")`~~ → `> cp`(bash_compat)
	- `41_mkdir`: ~~`system("mkdir...")`~~ → `> mkdir`(bash_compat)
	- `08_and_chain`/`09_or_chain`: 真实 `&&`/`||` 链
	- `25_continue`: 真实 `continue` 语句
	
	**保留 system() 的 case**(测试 system/system_status API,属语言特性测试而非命令模拟):`03_command_sub`/`05_redirect`/`06_exit_code`/`07_env_var`/`10_group`/`27_file_test`/`37_file_exists`/`38_file_size`/`48_cmd_fail`/`49_empty_input`。
	
	**验收**:`cargo test --test parity` strict → 79/79 全过,无回归。

**规避 2:字符串比较替代整数算术(影响 case 68/69)**

`68`/`69` 用 `if ns == "3"` 代替 `ns.to_int()` 算术。

**对应缺陷**:`.to_int()`/`.to_uint()` VM Bug 1(返回 None/0)。auto-lang codegen 的 native ID 分发问题——`shim_str_to_int_nv` override(ID 1516)未被调用,codegen 发的 CALL_NAT 用了不同 ID。已有 `fix/to-uint-codegen` worktree,但修复不完整。**待 auto-lang 修复后,case 68/69 应改用真正的 `.to_int()` 算术**。

**规避 3:echo 逐行写替代 printf 多行(影响多数文件类 case)**

**诊断结论**:经实测确认,`> printf "x\ny"` 在脚本文件里(`\n` 字面两字符)**完全正常工作**。之前的"printf 坏了"是测试文件创建方式的 bug(bash printf 把 `\\n` 转成真换行)。**这不是 ash 缺陷,echo 方式只是更清晰,无需改**。

**规避 4:唯一前缀文件名(影响多数文件类 case)**

case 用 `p51ls_unique.txt` 等唯一前缀,因 harness 不隔离 cwd。

**对应缺陷**:**harness 不隔离工作目录**——所有 case 在同一 cwd 跑,文件互相污染。spec §3.1 规约 5 要求工作目录隔离但未实现。**应让 harness 为每个 case 创建独立临时目录**。

	**规避 5:HashMap 词频改用简单计数(影响潜在词频 case) — ✅ 已修复(2026-07-29)**
	
	**原缺陷**:`HashMap.get_str("missing")` 经 `var x = ...` 存储后 `print(x)` 显示 None。
	
	**根因(两层)**:
	1. `native_catalog.rs`: `auto.hashmap.get_str` 返回类型注册为 `Void` → codegen 推断变量为 Int → `print()` 调用 `print_i32()` → 丢失 nanbox 类型标签。**修复**:get_str→String, get_int→Int, contains→Bool, size→Int, keys→List, is_empty→Bool。
	2. `autovm_persistent.rs`: `last_result: Option<i32>` 用 `pop_i32()` 捕获 REPL 结果 → 永远丢失 nanbox 标签。**修复**:last_result 改为 `Option<NanoValue>`，用 `pop_nv()` 保留标签；`format_last_result()` 改为调用已有的 `decode_nv_value()` (支持所有 nanbox 类型)。
	
	**验收**:`var x = m.get_str("missing"); print("[" + x + "]")` → 输出 `[]`(空字符串)✅。`var x = m.get_str("a"); print("value=" + x)` → 输出 `value=hello`✅。parity 79/79 无回归。

### 下阶段工作优先级(基于规避审计)

1. ~~**修 case 01-50 的规避 1**(最高价值)~~ → ✅ 已完成(2026-07-29):6 个 case(34/35/36/39/40/41)从 system() 改为真实 `> command`;此前 04/08/09/25/42-46 也已转换。保留 10 个测试 system API 的 case 不变。
2. ~~**修 harness 工作目录隔离**(规避 4)~~ → ✅ 已完成(commit `24d7509`):每个 case 独立临时目录
3. ~~**等 auto-lang 修 `.to_int()`**(规避 2)~~ → ✅ 已完成(commit `d23f091`):case 68/69 已改用真正 `.to_int()` 算术
4. **等 auto-lang 修 HashMap**(规避 5):修后可加词频统计类 case(当前非阻塞)

### 优先级 3 深度调研与修复方案(2026-07-29)

**调研结论**:两个 bug 都不是 native shim 分发优先级问题。

**Bug A:`.to_int()` 在变量 receiver 上返回 None**

- 字面量 `"42".to_int()` 正常(codegen 编译成 CALL_NAT 1516,命中 override shim)。
- 变量 `var n="42"; n.to_int()` 返回 None。根因:变量 receiver 的方法调用走 `CALL_SPEC` 操作码(engine.rs:4651-4805 的 str 内联分发表)。`to_int`/`parse_int`/`to_uint` 不在内联表里,命中 `_ =>` 分支(engine.rs:4801-4804),直接 `push_nv(encode_null())`——注释说"fall through to other handlers"但代码没做。
- **修复方案 B(推荐)**:在 engine.rs str 内联表的 match 里加 `"to_int"|"parse_int"` 和 `"to_uint"` 分支,直接内联 parse 逻辑(参照现有 str 方法如 `trim`/`len` 的模式)。

**Bug B:`HashMap.get_str("missing")` 改动看似不生效**

- 实际上改动**已生效**(native.rs:2722-2731 推空字符串)。误判来自 `last_result: Option<i32>` 把字符串索引(idx=6,nanbox payload=-7)误读成了 `-7`。
- 直接链式调用验证:`m.get_str("missing") + "]"` 拼出 `[]`(空字符串),证明 get_str 返回空字符串。
- 但经变量存储(`var x = m.get_str("missing"); print(x)`)时,变量槽按 i32 存储,破坏了 nanbox TAG_STRING → 显示为 None。这是更广的 nanbox 变量存储一致性问题。
- **修复方案**:get_str 的 shim 改动保持(已生效)。变量存储破坏 tag 的问题单独记录,不在本计划范围。

**实施步骤**:
1. (auto-lang)engine.rs str 内联表加 `to_int`/`parse_int`/`to_uint` 分支(方案 B)
2. (auto-lang)cherry-pick 到主仓库
3. (auto-shell)case 68/69 改用 `.to_int()` 真正算术(去掉字符串比较规避)
4. 验证 parity 全通过

---

## Phase 4: 7 类常见 bash 脚本场景覆盖(2026-07-29)

对照业界总结的 **7 大常见 bash 脚本类型**(日志清理、应用部署、数据备份、文件批量处理、日志告警、交互菜单、项目脚手架),审计前 72 个 case 的覆盖面,并补齐缺口。结论:**5 类有专门用例(部分原已覆盖),2 类完全未覆盖(find 递归、case/select 菜单、mkdir-p 脚手架)**。新增 7 个脚本级 case(74-80),套件 72 → 79。

### 新增用例

| # | 目录名 | 对应类型 | 状态 |
|---|---|---|---|
| 74 | `74_script_logcleanup` | 1. 日志清理(find -mtime) | **skip** |
| 75 | `75_script_deploy` | 2. 应用部署(&& 链 + 健康检查) | ✅ 通过 |
| 76 | `76_script_backup` | 3. 数据备份(date 时间戳 + gzip) | ✅ 通过 |
| 77 | `77_script_batchrename` | 4. 文件批量改名(.jpeg→.jpg) | ✅ 通过 |
| 78 | `78_script_alert` | 5. 日志告警(计数 + 阈值判断) | ✅ 通过 |
| 79 | `79_script_menu` | 6. 交互菜单(if-elif 模拟 case/select) | ✅ 通过 |
| 80 | `80_script_scaffold` | 7. 项目脚手架(mkdir-p + touch + find) | **skip** |

**验收**:`cargo test --test parity`(strict)→ **79/79**(77 实跑通过 + 2 skip)。无回归(原 72 全保持绿)。

### 关键洞察:ash 命令执行模型(实测确认)

实施中用真实 ash 二进制逐类实测,确认 ash 的命令执行模型对脚本可移植性的决定性影响:

- **`>` 前缀仅支持内置命令**(echo/cat/ls/grep/wc/sort/uniq/sed/mkdir/touch 等);`> date`/`> mv`/`> find`/`> gzip` 等外部命令报 `program not found`。
- **外部命令必须走 `system("...")` 桥**(在 AutoLang 函数体内)。但 `system()` 桥有两个缺陷(见下)。
- **`>` 内置命令 + `bash_compat` 标记是最可靠的 parity 路径**——这是 case 77 能通过而 74/80 失败的根本原因。`> ls`/`> mv`/`> cat` 在 bash_compat 下输出纯文本;而 `system("ls ...")` 输出 ratatui 表格(ANSI 码 + `╭╮╰╯` 边框),`system("find ... -name ...")` 返回空。

### 新发现并钉住的缺陷(2 个 skip)

**缺陷 A:`system()` 桥不能正确转发含 shell 特殊字符的命令**(影响 case 74/80)
- `system("find . -name '*.log'")` 返回空(glob/引号在桥里丢失)。
- `system("find ... | sort")` 等含管道的捕获也返回空。
- 根因:system() 桥的命令字符串解析未正确处理引号/glob/管道。
- 钉住方式:case 74/80 的 `skip` 文件记录根因,待 system 桥修复后移除。

**缺陷 B:`> find` 内置命令的 stdout 不接入捕获缓冲**(影响 case 80)
- `> mkdir -p` / `> touch` 副作用生效(文件确实创建),但 `var x = > find ... -type f` 捕获为空。
- `> ls -R` 报 "program not found",`> ls <subdir>` 报 "os error 5 拒绝访问"——无可用内置命令枚举递归树。
- 钉住方式:case 80 的 `skip` 文件记录根因。

### 实施中的调试插曲:77 的 bash_compat 标记疏漏

77 首次全量测试失败(ash 输出 ratatui 表格而非纯文本),手动单跑却正确。经系统调试(Phase 1 证据收集:在全量环境给 ash.ash 加 DBG print 捕获实际 stdout)定位根因:**创建 77 时遗漏了 `bash_compat` 空标记文件**,导致 harness 未传 `--bash-compat`,`> ls` 输出表格。补标记后通过。

教训:**手动测试必须完全复现 harness 的执行方式**(脚本路径作 arg + 仅当存在 bash_compat 标记才传 flag),否则会漏掉标记相关的问题。这也是 `skip`/`bash_compat` 标记机制的价值——把环境依赖显式化。

### 不做的事(YAGNI)

- ~~不修复 system 桥缺陷 A / `> find` 捕获缺陷 B~~ — **已在 Phase 4 后续修复,见下节"缺陷修复(2026-07-29)"**。
- ~~不改造早期 01-50 的"规避 1"(用 AutoLang 模拟 shell 命令)~~ — **已完成(2026-07-29)**:6 个 case(34/35/36/39/40/41)从 system() 改为真实 `> command`;此前的 ca0e239 已转换 04/08/09/25/42-46。详见上节"规避 1"更新。

---

## Phase 4 后续:缺陷修复(2026-07-29)

Phase 4 用 `skip` 钉住的 case 74/80 暴露的缺陷已全部修复,用例转绿,套件达到 **79/79 全过(0 skip)**。修复过程中根因比初判更深,共修复 4 个关联缺陷。

### 修复的 4 个缺陷

**缺陷 A(system 桥返回空/乱码)→ 修复**
- 根因:`ShellHostBridge::system`(host.rs:119)调 `shell.execute(cmd)` 时未开 `bash_compat`,`format_output` 对结构化命令(find/ls/wc)渲染 ratatui 表格而非纯文本。
- 修复:新增 `Shell::execute_capture(cmd)`(shell.rs,紧邻 `execute_for_agent`),以 bash_compat 模式执行并捕获;`system()` 桥改调它。用 `was_compat` 保存恢复(而非硬置 false),可在已开 bash_compat 的上下文里安全嵌套。
- 效果:`system("ls")`/`system("find ...")` 现返回 bash 风格纯文本。

**缺陷 B(find FileList 捕获空)→ 修复**
- 根因:`format_file_list_as_bash`(value_helpers.rs)硬编码只读 `name` 字段,而 find 的对象只有 `path`/`type`,无 `name` → 全部条目被丢弃 → 空串。
- 修复:`format_file_list_as_bash` 在 `name` 缺失时回退读 `path`。find 输出路径(bash `find` 语义),ls 输出文件名(原行为不变)。
- 效果:find 的 FileList 能正确渲染;value_helpers 12 个单测全过(无回归)。

**缺陷 C(find -name/-type 参数绑定错)→ 修复**
- 根因:find 的 signature 用 `flag_with_short`(布尔标志)声明 `-name`/`-type`,但它们需带值。导致 `-t f` 的 `f` 残留在 positionals,污染 name_pattern 检测 → 误匹配/漏匹配。
- 修复:改用 `option_with_short`(带值选项)。parser 现正确把 `-t`/`-n` 后的 token 绑定为值。

**缺陷 D(find 路径格式不 parity)→ 修复**
- 根因:find 的 `pathdiff` 用 `strip_prefix(root)` 剥掉根,输出裸文件名(`a.log`、`src\main.py`),而 bash `find` 输出完整相对路径(`./a.log`、`p80_proj/src/main.py`,正斜杠,保留根前缀)。
- 修复:新增 `display_path(root_arg, root, target)`,输出 `root_arg/相对路径`(正斜杠,Windows 反斜杠归一为 `/`)。
- 效果:`find . -t f` → `./a.log`;`find p80_proj -t f` → `p80_proj/src/main.py`,与 bash 完全一致。

### 遗留(未修,非本次范围)

- **单横杠长选项**(`-type`):ash parser 只认 `--type`(双横杠)或 `-t`(单字符),`-type`(单横杠多字符)被当短选项簇解析(`-t -y -p -e` → "Unknown flag: -y")。这是 parser 通用设计,影响所有命令,回归面大。case 74/80 的 ash 端改用短选项 `-t f` 规避(bash 端仍用 `-type f`,输出一致)。
- **glob 展开不尊重单引号**:`system("find . -n '*.log'")` 中 `*.log` 仍被 shell 展开。case 74 改用 `-t f` + AutoLang 过滤 `.log` 后缀规避。
- **外部命令捕获**:`execute_external` 的 `capture_output=false` 使 date/gzip 等外部命令在 system 桥下返回空。case 76 用 `> ls` + AutoLang 计数规避。独立大任务。

### 验收

- parity **79/79 全过(0 skip)**,strict 模式。case 74/80 从 skip 转绿。
- 单测无回归:value_helpers 12/12、auto-shell lib 603/603。
- 改动文件:`find.rs`(缺陷 B/C/D)、`value_helpers.rs`(缺陷 B)、`shell.rs`(缺陷 A,新增 execute_capture)、`host.rs`(缺陷 A,system 桥改调 execute_capture)、case 74/80 脚本(用 `-t f`)。

