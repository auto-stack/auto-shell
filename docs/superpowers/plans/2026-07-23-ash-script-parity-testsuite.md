# ash 脚本 Parity 测试套件(MVP)实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Plan 036 已建的 parity 框架上,落地 32 个纯文本场景的 ash↔bash parity 用例,达到 ≥25 个实测跑通。

**Architecture:** 沿用 `tests/parity.rs` 的子进程方案(run_ash 已是 `Command::new(ash_bin).arg(script)`),增强三点(R2 二进制定位、R3 exit-code 对比、normalize 路径占位),修正 01_echo,再批量填充 31 个 case。

**Tech Stack:** Rust 集成测试(`cargo test --test parity`),ash 子进程 + bash/pwsh 子进程对比。

**设计文档:** `designs/037-script-parity-testsuite.md`(已确认)

---

## 实测验证的 ash 能力矩阵(计划依据,务必遵守)

在写任何 case 前先明确——以下结论均已用 `ash/target/debug/ash` 实测验证(2026-07-23):

**纯文本输出(与 bash 一致,可用)**:
- `> echo "x"` — 纯文本(注意:`echo` 输出含一个尾部空行,normalize 处理)
- `print(x)` — AutoLang,纯文本
- `var x = ...` / 字符串拼接 `"a"+b` — 纯文本
- `> cat file` — 文件内容,纯文本
- `> cat f | sed -n 1,2p` — sed 范围,纯文本(`apple\nbanana`)
- `> echo x | sed s/a/X/g` — sed 替换,纯文本(`bXnXnX`)
- `> echo hello | tr a-z A-Z` — tr,纯文本(`HELLO`)
- `> cat f | sort` — sort,纯文本(`alpha\nalpha\nbeta...`)
- `> cat f | sort | uniq -c` — uniq,纯文本(`      2 alpha`,有前导空格)
- `> cut -c1 file` — cut,纯文本
- `exit(code)` — 传播为进程退出码(`exit(7)` → 进程退出码 7),`exit` 后的代码不执行
- `for l in s.lines()` AutoLang 计数 — 纯文本(`3`)

**结构化表格或缺失(不可用,会导致 KNOWN_FAIL)**:
- `> wc -l/-w/-c` → 结构化(`lines: 5` / `words: 5` / `bytes: 28`),bash 是 `5`
- `> grep x file` 或 `> cat f | grep x` → 结构化表格(`<stdin>   apple`)
- `> head -N file` / `> tail -N file` → `Error: Unknown flag: -2`(ash 不支持)
- `> ... | awk ...` → 报错(ash 内建 awk 不完整)

**结论**:涉及 wc/grep/head/tail/awk 的 case,ash 版必须改用 AutoLang 原生实现(循环计数、字符串匹配)或纯文本外部命令(sed/cut),否则必然结构化不 parity。

---

## 文件结构

```
ash/auto-shell/tests/
├── parity.rs                      # 修改:harness 增强(R2/R3/normalize 路径占位)
└── parity/
    ├── README.md                  # 新建:框架说明 + 用例索引 + 已知差异表
    └── cases/
        ├── 01_echo/{ash.ash,bash.sh,pwsh.ps1,expected.txt}   # 修正(加 > 前缀)
        ├── 02_var_print/...        # 新建 ×31
        ├── ... (32 个 case 目录)
        └── 32_text_pipeline/...
```

每个 case 目录含:`ash.ash`(守 `>` 规约)、`bash.sh`、`pwsh.ps1`、`expected.txt`(bootstrap 生成)。

---

## Task 1: harness 增强 — R2 二进制定位

**Files:**
- Modify: `ash/auto-shell/tests/parity.rs:71-93`(`run_ash` + `ash_binary_path`)

当前 `ash_binary_path()` 用硬路径 `target/debug/ash`,要求手动 build。改用 `env!("CARGO_BIN_EXE_ash")`(cargo 自动构建定位,二进制名确认为 `ash`,见 Cargo.toml:9-11)。

- [ ] **Step 1: 替换 `ash_binary_path` 实现**

把 `parity.rs:81-93` 的整个 `ash_binary_path` 函数替换为:

```rust
/// Locate the ash binary (cargo auto-builds it via CARGO_BIN_EXE_ash).
/// Falls back to ASH_TEST_BIN env override for custom builds.
fn ash_binary_path() -> PathBuf {
    if let Ok(b) = std::env::var("ASH_TEST_BIN") {
        return PathBuf::from(b);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_ash"))
}
```

- [ ] **Step 2: 确认 `run_ash` 签名不变(此 task 只改定位)**

`run_ash`(parity.rs:71-78)保持返回 `Option<String>`(本 task 不动 exit-code,Task 2 再改)。无需改动。

- [ ] **Step 3: 验证编译**

Run: `cd ash && cargo build --tests 2>&1 | tail -20`
Expected: 无编译错误(`CARGO_BIN_EXE_ash` 宏在集成测试中可用,因 `ash` 是同 crate 的 bin target)。

- [ ] **Step 4: 暂不提交**(Task 2-3 一起提交)

---

## Task 2: harness 增强 — R3 exit-code 对比

**Files:**
- Modify: `ash/auto-shell/tests/parity.rs:71-130`(runner 返回 exit-code)、`:203-280`(对比逻辑)

让 runner 返回 `(stdout, exit_code)`,对比逻辑加入 exit-code 比对。

- [ ] **Step 1: 改 `run_ash` 返回 `(String, i32)`**

替换 `parity.rs:69-78`(`run_ash` 函数):

```rust
/// Execute an ash script via subprocess. Returns (stdout, exit_code).
fn run_ash(script_path: &Path) -> Option<(String, i32)> {
    let bin = ash_binary_path();
    let output = Command::new(&bin)
        .arg(script_path)
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    ))
}
```

- [ ] **Step 2: 改 `run_bash`/`run_pwsh` 返回 `(String, i32)`**

把 `run_bash`(parity.rs:95-102)、`run_pwsh`(:104-112)、`run_fish`(:114-121)、`run_nu`(:123-130)四个函数,**每个**都改为返回 `Option<(String, i32)>`,在返回元组里加 `output.status.code().unwrap_or(-1)`。例如 `run_bash`:

```rust
fn run_bash(script_path: &Path) -> Option<(String, i32)> {
    let output = Command::new("bash")
        .arg(script_path)
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    ))
}
```

对 `run_pwsh`/`run_fish`/`run_nu` 做同样改动(各自保留自己的 Command 调用)。

- [ ] **Step 3: 改 `run_parity_case` 用新返回值 + 加 exit-code 对比**

替换 `parity.rs:203-235`(`run_parity_case` 的第 1-3 步,即 ash 运行 + expected 对比 + bash 对比部分):

```rust
fn run_parity_case(case: &ParityCase) -> Result<(), String> {
    // 1. Run ash
    let (ash_out, ash_code) = run_ash(&case.ash_script).unwrap_or((String::new(), -1));
    let ash_norm = normalize(&ash_out);

    // 2. Compare against expected.txt (golden) if present
    if let Some(expected) = &case.expected {
        let exp_norm = normalize(expected);
        if ash_norm != exp_norm {
            return Err(format!(
                "ash output != expected\n\
                 --- ash (normalized) ---\n{}\n\
                 --- expected (normalized) ---\n{}\n",
                ash_norm, exp_norm
            ));
        }
    }

    // 3. Compare against bash if present and bash is available
    if let Some(bash_path) = &case.bash_script {
        if command_exists("bash") {
            let (bash_out, bash_code) = run_bash(bash_path).unwrap_or((String::new(), -1));
            let bash_norm = normalize(&bash_out);
            if ash_norm != bash_norm {
                return Err(format!(
                    "ash output != bash output\n\
                     --- ash (normalized) ---\n{}\n\
                     --- bash (normalized) ---\n{}\n",
                    ash_norm, bash_norm
                ));
            }
            // exit-code parity (R3): compare against bash exit code.
            if ash_code != bash_code {
                return Err(format!(
                    "ash exit-code != bash exit-code: {} != {}",
                    ash_code, bash_code
                ));
            }
        }
    }
```

(后续 pwsh/fish/nu best-effort WARNING 部分保持不变,但它们的 `run_*` 返回值已是元组,需同步解构——见 Step 4)

- [ ] **Step 4: 修正 pwsh/fish/nu best-effort 部分的解构**

替换 `parity.rs:237-277`(pwsh/fish/nu best-effort 段)里每个 `let xxx_out = run_xxx(...).unwrap_or_default();` 为解构元组。例如:

```rust
    // 4. PowerShell comparison (best-effort)
    let pwsh_path = case.dir.join("pwsh.ps1");
    if pwsh_path.exists() && command_exists("pwsh") {
        let (pwsh_out, _) = run_pwsh(&pwsh_path).unwrap_or_default();
        let pwsh_norm = normalize(&pwsh_out);
        if ash_norm != pwsh_norm {
            eprintln!(
                "WARNING: ash != pwsh for {} (best-effort, not failing):\n\
                 ash:  {}\npwsh: {}",
                case.name, ash_norm, pwsh_norm
            );
        }
    }
```

注意 `unwrap_or_default()` 对 `(String,i32)` 返回 `("", 0)`。对 fish、nu 做同样改动。

- [ ] **Step 5: 修正 `bootstrap_expected` 的解构**

替换 `parity.rs:319-338`(`bootstrap_expected`)里的 `let bash_out = run_bash(...)`:

```rust
            if command_exists("bash") {
                let (bash_out, _) = run_bash(bash_path).unwrap_or_default();
                let normalized = normalize(&bash_out);
```

- [ ] **Step 6: 验证编译**

Run: `cd ash && cargo build --tests 2>&1 | tail -20`
Expected: 无错误。如有 "expected struct/ tuple mismatch" 错误,检查是否漏改某个 `run_*` 调用点的解构。

- [ ] **Step 7: 暂不提交**(Task 3 一起提交)

---

## Task 3: normalize 增强 — 绝对路径占位

**Files:**
- Modify: `ash/auto-shell/tests/parity.rs:23-63`(`normalize`)

加绝对路径 → `<TMPDIR>` 占位(规避临时目录差异)。实测显示 ash 的结构化输出含 `\\?\C:\Users\...\Temp\...` 路径,纯文本 case 虽较少出现,但仍需防护(尤其文件类 case 的 stderr 混入或 error 信息)。

- [ ] **Step 1: 在 `normalize` 末尾(return 前)加路径占位**

在 `parity.rs` 的 `normalize` 函数里,`s = s.trim_matches('\n').to_string();` 之后、`s`(return)之前,插入:

```rust
    // Replace absolute paths with <TMPDIR> placeholder (temp dir differences).
    // Matches Windows (\\?\C:\... or C:\...) and Unix (/tmp/...) paths.
    // Best-effort: uses std::env::temp_dir() prefix.
    let tmp = std::env::temp_dir();
    let tmp_str = tmp.to_string_lossy().replace('\\', "/");
    let tmp_win = tmp.to_string_lossy();
    s = s.replace(&*tmp_win, "<TMPDIR>");
    // Also normalize backslash-style \\?\ prefixes and forward-slash variants
    s = s.replace("\\\\?\\", "");
    s = s.replace(&tmp_str, "<TMPDIR>");
```

- [ ] **Step 2: 验证编译 + 运行现有 01_echo(此时 01_echo 仍是错的,预期 fail)**

Run: `cd ash && cargo build --tests 2>&1 | tail -5`
Expected: 编译通过。

- [ ] **Step 3: 提交 harness 增强**

```bash
git add ash/auto-shell/tests/parity.rs
git commit -m "feat(037): parity harness enhancements — CARGO_BIN_EXE_ash, exit-code compare, path normalize

R2: binary location via env!(CARGO_BIN_EXE_ash) (cargo auto-builds).
R3: runners return (stdout, exit_code); exit-code parity enforced.
normalize: absolute temp paths -> <TMPDIR> placeholder.

Refs designs/037. Part of plan 036/037."
```

---

## Task 4: 修正 01_echo + 建 README

**Files:**
- Modify: `ash/auto-shell/tests/parity/cases/01_echo/ash.ash`(加 `>` 前缀)
- Create: `ash/auto-shell/tests/parity/README.md`

`01_echo/ash.ash` 当前写成 `echo "hello"`(无 `>`),被 ash 当 AutoLang 代码吞掉。实测确认 `> echo "hello"` 正常输出。

- [ ] **Step 1: 修正 `01_echo/ash.ash`**

写入(注意 ash 的 echo 输出含尾部空行,但 normalize 会 trim,所以 expected.txt 仍是 `hello\nworld`):

```ash
> echo "hello"
> echo "world"
```

- [ ] **Step 2: 重建 `01_echo/expected.txt`**

Run: `cd ash/auto-shell/tests/parity/cases/01_echo && bash bash.sh > expected.txt`
然后检查 `cat expected.txt` 应为:
```
hello
world
```
(末尾一个换行,bash echo 默认行为)

- [ ] **Step 3: 创建 `tests/parity/README.md`**

```markdown
# ash 脚本 Parity 测试

验证 ash 脚本与 bash/PowerShell 的输出一致性。详见 `designs/037-script-parity-testsuite.md`。

## 运行

```bash
# 前置:构建 ash 二进制
cd ash && cargo build

# 运行全部 parity 用例
cargo test --test parity

# 从 bash 重新生成所有 expected.txt(golden)
cargo test --test parity -- bootstrap_expected --nocapture
```

## 用例编写规范

每个 ash 用例必须遵守(详见 designs/037 §3.1):
- **shell 命令以 `>` 前缀**:bash `echo x` → ash `> echo x`
- **命令捕获**:bash `x=$(cmd)` → ash `var x = > cmd`(注意 `.trim()` 尾部空行)
- **AutoLang 逻辑用原生语法**:`var`/`print`/`if{}`/`for in`/`fn`
- **纯文本优先**:避免 wc/grep/head/tail/awk(ash 里结构化或缺失);计数用 `for l in s.lines()` AutoLang 循环

## 已知差异(MVP)

以下用例因 ash 当前能力限制标记 KNOWN_FAIL(非回归):
- (实施中填入,如:`NN_xxx` — 原因:wc 结构化输出)

## MVP 验收

32 个用例建立,≥25 个实测跑通。详见 designs/037 §4。
```

- [ ] **Step 4: 验证 01_echo 实测跑通**

Run: `cd ash/auto-shell && cargo test --test parity -- parity_all_cases --nocapture 2>&1 | tail -20`
Expected: `01_echo` 通过(无 `❌ 01_echo`)。若整组测试因后续未建的 case 报错属正常,但 01_echo 本身不应在 failures 列表。

- [ ] **Step 5: 提交**

```bash
git add ash/auto-shell/tests/parity/cases/01_echo/ash.ash \
        ash/auto-shell/tests/parity/cases/01_echo/expected.txt \
        ash/auto-shell/tests/parity/README.md
git commit -m "fix(037): correct 01_echo ash script (> prefix) + parity README

01_echo was written bash-style without > prefix, so echo was treated
as AutoLang code and produced no output. Now > echo works.

Refs designs/037."
```

---

## Task 5: A 类用例 — 基础命令与 IO(02-06)

**Files:**
- Create: `tests/parity/cases/{02_var_print,03_cmd_capture,04_pipe,05_redirect,06_exit_code}/{ash.ash,bash.sh,pwsh.ps1}`

每个 case:先写 bash.sh,跑生成 expected.txt;再写 ash.ash;实测对齐。以下代码均已实测验证。

- [ ] **Step 1: `02_var_print` — 变量 + print**

`cases/02_var_print/ash.ash`:
```ash
var x = "hello"
var y = "world"
print(x + " " + y)
```
`cases/02_var_print/bash.sh`:
```bash
#!/bin/bash
x="hello"
y="world"
echo "$x $y"
```
`cases/02_var_print/pwsh.ps1`:
```powershell
$x = "hello"
$y = "world"
Write-Output "$x $y"
```
生成 expected:`cd cases/02_var_print && bash bash.sh > expected.txt`(应为 `hello world`)

- [ ] **Step 2: `03_cmd_capture` — 命令输出捕获**

`cases/03_cmd_capture/ash.ash`:
```ash
var x = > echo captured
print(x.trim())
```
`cases/03_cmd_capture/bash.sh`:
```bash
#!/bin/bash
x=$(echo captured)
echo "$x"
```
`cases/03_cmd_capture/pwsh.ps1`:
```powershell
$x = "captured"
Write-Output $x
```
生成 expected(`captured`)。

- [ ] **Step 3: `04_pipe` — 管道(用纯文本命令,规避 grep/wc 结构化)**

注意:实测 `cat|grep` 和 `wc -l` 都结构化。改用 sort 计数(纯文本)。
`cases/04_pipe/ash.ash`:
```ash
var data = > printf "b\na\nc\na\nb\n"
var sorted = data.lines()
for l in sorted {
    print(l)
}
```
等等——更稳妥地直接用纯文本管道 sort。实测 `> cat f | sort` 是纯文本。用文件:
`cases/04_pipe/ash.ash`:
```ash
> echo "banana" > pipe_in.txt
> echo "apple" >> pipe_in.txt
> echo "cherry" >> pipe_in.txt
> cat pipe_in.txt | sort
```
`cases/04_pipe/bash.sh`:
```bash
#!/bin/bash
echo "banana" > pipe_in.txt
echo "apple" >> pipe_in.txt
echo "cherry" >> pipe_in.txt
cat pipe_in.txt | sort
```
`cases/04_pipe/pwsh.ps1`:
```powershell
"banana","apple","cherry" | Sort-Object
```
生成 expected(`apple\nbanana\ncherry`)。注意:此 case 会在工作目录留 `pipe_in.txt`,但 parity 只比 stdout,不影响。

- [ ] **Step 4: `05_redirect` — 重定向写+读**

`cases/05_redirect/ash.ash`:
```ash
> echo "first line" > redir.txt
> echo "second line" >> redir.txt
> cat redir.txt
```
`cases/05_redirect/bash.sh`:
```bash
#!/bin/bash
echo "first line" > redir.txt
echo "second line" >> redir.txt
cat redir.txt
```
`cases/05_redirect/pwsh.ps1`:
```powershell
"first line","second line" | Set-Content redir.txt
Get-Content redir.txt
```
生成 expected(`first line\nsecond line`)。

- [ ] **Step 5: `06_exit_code` — exit 传播**

`cases/06_exit_code/ash.ash`:
```ash
print("before exit")
exit(42)
print("after exit")
```
`cases/06_exit_code/bash.sh`:
```bash
#!/bin/bash
echo "before exit"
exit 42
echo "after exit"
```
`cases/06_exit_code/pwsh.ps1`:
```powershell
Write-Output "before exit"
exit 42
Write-Output "after exit"
```
生成 expected(`before exit`)。注意:此 case 的 exit-code 对比(R3)会验证 ash=42=bash。

- [ ] **Step 6: 实测验证 02-06 全部跑通**

Run: `cd ash/auto-shell && cargo test --test parity -- parity_all_cases --nocapture 2>&1 | tail -30`
Expected: 01-06 全部不在 failures 列表。若某个 fail,看是 stdout 还是 exit-code,据实测矩阵调整。

- [ ] **Step 7: 提交 A 类**

```bash
git add ash/auto-shell/tests/parity/cases/0[2-6]_*/
git commit -m "feat(037): parity cases 02-06 (A-class: var/capture/pipe/redirect/exit)

All verified passing ash vs bash (stdout + exit-code). Refs designs/037."
```

---

## Task 6: B 类用例 — 字符串操作(07-12)

**Files:** `cases/{07_str_concat,08_str_len,09_str_sub,10_str_replace,11_str_case,12_str_split}/`

字符串操作优先用 AutoLang 原生方法(实测 `.len()` 等可用),避免 sed/tr 的边界差异。

- [ ] **Step 1: `07_str_concat`**

`cases/07_str_concat/ash.ash`:
```ash
var a = "foo"
var b = "bar"
print(a + b)
print(a + "-" + b)
```
`cases/07_str_concat/bash.sh`:
```bash
#!/bin/bash
a="foo"; b="bar"
echo "${a}${b}"
echo "${a}-${b}"
```
`cases/07_str_concat/pwsh.ps1`:
```powershell
$a="foo"; $b="bar"
Write-Output "$a$b"
Write-Output "$a-$b"
```
expected(`foobar\nfoo-bar`)。

- [ ] **Step 2: `08_str_len`**

`cases/08_str_len/ash.ash`:
```ash
var s = "hello"
print(s.len())
```
注意:`.len()` 返回的是字节数还是字符数需实测确认。bash `${#s}` 是字节数。
`cases/08_str_len/bash.sh`:
```bash
#!/bin/bash
s="hello"
echo "${#s}"
```
`cases/08_str_len/pwsh.ps1`:
```powershell
$s="hello"
Write-Output $s.Length
```
expected(`5`)。**若 ash `.len()` 与 bash 不一致,标 KNOWN_FAIL 并记录。**

- [ ] **Step 3: `09_str_sub`**

用 sed/cut 规避 AutoLang sub 边界。实测 `cut` 纯文本可用。
`cases/09_str_sub/ash.ash`:
```ash
var s = "hello world"
> echo $s | cut -c1-5
```
注意:`$s` 在 `>` 行里会插值(ash shell-bridge 支持 `$var`)。
`cases/09_str_sub/bash.sh`:
```bash
#!/bin/bash
s="hello world"
echo "$s" | cut -c1-5
```
`cases/09_str_sub/pwsh.ps1`:
```powershell
$s="hello world"
Write-Output $s.Substring(0,5)
```
expected(`hello`)。**若 `$s` 在 `>` 行不插值,改用 `print(s.sub(0,5))` AutoLang 方式并实测。**

- [ ] **Step 4: `10_str_replace`**

`cases/10_str_replace/ash.ash`:
```ash
> echo banana | sed s/a/o/g
```
`cases/10_str_replace/bash.sh`:
```bash
#!/bin/bash
echo banana | sed 's/a/o/g'
```
`cases/10_str_replace/pwsh.ps1`:
```powershell
"banana" -replace "a","o"
```
expected(`bonono`)。实测 `sed s/a/o/g` 纯文本输出 `bXnXnX`(当 X=o 时为 bonono)。

- [ ] **Step 5: `11_str_case`**

`cases/11_str_case/ash.ash`:
```ash
> echo hello | tr a-z A-Z
```
`cases/11_str_case/bash.sh`:
```bash
#!/bin/bash
echo hello | tr a-z A-Z
```
`cases/11_str_case/pwsh.ps1`:
```powershell
"hello".ToUpper()
```
expected(`HELLO`)。

- [ ] **Step 6: `12_str_split`**

用 cut 取字段(纯文本)。
`cases/12_str_split/ash.ash`:
```ash
> echo "a,b,c" | cut -d, -f2
```
`cases/12_str_split/bash.sh`:
```bash
#!/bin/bash
echo "a,b,c" | cut -d, -f2
```
`cases/12_str_split/pwsh.ps1`:
```powershell
"a,b,c".Split(",")[1]
```
expected(`b`)。

- [ ] **Step 7: 实测验证 + 生成所有 expected**

```bash
cd ash/auto-shell/tests/parity/cases
for d in 0[7-9]_* 1[0-2]_*; do
  (cd "$d" && bash bash.sh > expected.txt)
done
cd ../../../../..
cd ash/auto-shell && cargo test --test parity -- parity_all_cases --nocapture 2>&1 | tail -30
```
Expected: 07-12 中大部分通过。08(len)、09(sub)可能 KNOWN_FAIL,记录到 README。

- [ ] **Step 8: 提交 B 类**

```bash
git add ash/auto-shell/tests/parity/cases/0[789]_* ash/auto-shell/tests/parity/cases/1[0-2]_* ash/auto-shell/tests/parity/README.md
git commit -m "feat(037): parity cases 07-12 (B-class: string ops)

Refs designs/037. KNOWN_FAIL recorded in README for len/sub if divergent."
```

---

## Task 7: C 类用例 — 条件与循环(13-20)

**Files:** `cases/{13_if_else,14_if_elif,15_for_list,16_for_range,17_while,18_break,19_continue,20_nested_loop}/`

全部用 AutoLang 原生控制流(实测 `if`/`for in`/`while`/`break`/`continue` 可用)。

- [ ] **Step 1: `13_if_else`**

`cases/13_if_else/ash.ash`:
```ash
var n = 5
if n > 3 {
    print("big")
} else {
    print("small")
}
```
`cases/13_if_else/bash.sh`:
```bash
#!/bin/bash
n=5
if [ "$n" -gt 3 ]; then echo "big"; else echo "small"; fi
```
`cases/13_if_elif`/pwsh:
```powershell
$n=5
if ($n -gt 3) { Write-Output "big" } else { Write-Output "small" }
```
expected(`big`)。

- [ ] **Step 2: `14_if_elif`**

`cases/14_if_elif/ash.ash`:
```ash
var score = 75
if score >= 90 {
    print("A")
} else {
    if score >= 60 {
        print("B")
    } else {
        print("C")
    }
}
```
(AutoLang 嵌套 if 模拟 elif;若 ash 支持 `elif` 语法则用之)
`cases/14_if_elif/bash.sh`:
```bash
#!/bin/bash
score=75
if [ "$score" -ge 90 ]; then echo "A"
elif [ "$score" -ge 60 ]; then echo "B"
else echo "C"; fi
```
`cases/14_if_elif/pwsh.ps1`:
```powershell
$score=75
if ($score -ge 90) { Write-Output "A" }
elseif ($score -ge 60) { Write-Output "B" }
else { Write-Output "C" }
```
expected(`B`)。

- [ ] **Step 3: `15_for_list`**

`cases/15_for_list/ash.ash`:
```ash
for item in ["apple", "banana", "cherry"] {
    print(item)
}
```
`cases/15_for_list/bash.sh`:
```bash
#!/bin/bash
for item in apple banana cherry; do echo "$item"; done
```
`cases/15_for_list/pwsh.ps1`:
```powershell
foreach ($item in "apple","banana","cherry") { Write-Output $item }
```
expected(`apple\nbanana\ncherry`)。

- [ ] **Step 4: `16_for_range`**

`cases/16_for_range/ash.ash`:
```ash
var i = 0
while i < 3 {
    print(i)
    i = i + 1
}
```
(用 while 模拟 range,规避 AutoLang range 语法不确定性;实测 while+i+1 已验证可用)
`cases/16_for_range/bash.sh`:
```bash
#!/bin/bash
for i in 0 1 2; do echo "$i"; done
```
`cases/16_for_range/pwsh.ps1`:
```powershell
0..2 | ForEach-Object { Write-Output $_ }
```
expected(`0\n1\n2`)。

- [ ] **Step 5: `17_while`**

`cases/17_while/ash.ash`:
```ash
var count = 0
var sum = 0
while count < 5 {
    sum = sum + count
    count = count + 1
}
print(sum)
```
`cases/17_while/bash.sh`:
```bash
#!/bin/bash
count=0; sum=0
while [ "$count" -lt 5 ]; do sum=$((sum+count)); count=$((count+1)); done
echo "$sum"
```
`cases/17_while/pwsh.ps1`:
```powershell
$count=0; $sum=0
while ($count -lt 5) { $sum+=$count; $count++ }
Write-Output $sum
```
expected(`10`,即 0+1+2+3+4)。**注意:此处算术用 AutoLang 整数加法,非 .to_uint(),规避 VM Bug 1。**

- [ ] **Step 6: `18_break`**

`cases/18_break/ash.ash`:
```ash
for n in [1, 2, 3, 4, 5] {
    if n == 3 {
        break
    }
    print(n)
}
```
`cases/18_break/bash.sh`:
```bash
#!/bin/bash
for n in 1 2 3 4 5; do
  if [ "$n" -eq 3 ]; then break; fi
  echo "$n"
done
```
`cases/18_break/pwsh.ps1`:
```powershell
foreach ($n in 1..5) { if ($n -eq 3) { break }; Write-Output $n }
```
expected(`1\n2`)。

- [ ] **Step 7: `19_continue`**

`cases/19_continue/ash.ash`:
```ash
for n in [1, 2, 3, 4, 5] {
    if n == 3 {
        continue
    }
    print(n)
}
```
`cases/19_continue/bash.sh`:
```bash
#!/bin/bash
for n in 1 2 3 4 5; do
  if [ "$n" -eq 3 ]; then continue; fi
  echo "$n"
done
```
`cases/19_continue/pwsh.ps1`:
```powershell
foreach ($n in 1..5) { if ($n -eq 3) { continue }; Write-Output $n }
```
expected(`1\n2\n4\n5`)。

- [ ] **Step 8: `20_nested_loop`**

`cases/20_nested_loop/ash.ash`:
```ash
for i in [1, 2] {
    for j in [1, 2] {
        print(i + "," + j)
    }
}
```
`cases/20_nested_loop/bash.sh`:
```bash
#!/bin/bash
for i in 1 2; do for j in 1 2; do echo "$i,$j"; done; done
```
`cases/20_nested_loop/pwsh.ps1`:
```powershell
foreach ($i in 1..2) { foreach ($j in 1..2) { Write-Output "$i,$j" } }
```
expected(`1,1\n1,2\n2,1\n2,2`)。**注意:`i + "," + j` 涉及整数转字符串拼接,需实测确认 AutoLang 行为;若整数不能直接拼接,改为 `> echo $i,$j` 走 shell。**

- [ ] **Step 9: 实测验证 + 生成 expected + 提交**

```bash
cd ash/auto-shell/tests/parity/cases
for d in 1[3-9]_* 20_*; do (cd "$d" && bash bash.sh > expected.txt); done
cd ../../../../..
cd ash/auto-shell && cargo test --test parity -- parity_all_cases --nocapture 2>&1 | tail -40
```
根据 fail 情况调整(尤其 20 整数拼接)。把 KNOWN_FAIL 更新到 README。
```bash
git add ash/auto-shell/tests/parity/cases/1[3-9]_* ash/auto-shell/tests/parity/cases/20_* ash/auto-shell/tests/parity/README.md
git commit -m "feat(037): parity cases 13-20 (C-class: conditionals & loops)

Refs designs/037. AutoLang-native control flow."
```

---

## Task 8: D 类用例 — 函数(21-24)

**Files:** `cases/{21_func_def,22_func_args,23_func_return,24_recursion}/`

用 AutoLang `fn`/`return`(实测可用)。

- [ ] **Step 1: `21_func_def`**

`cases/21_func_def/ash.ash`:
```ash
fn greet() {
    print("hello from function")
}
greet()
```
`cases/21_func_def/bash.sh`:
```bash
#!/bin/bash
greet() { echo "hello from function"; }
greet
```
`cases/21_func_def/pwsh.ps1`:
```powershell
function greet { Write-Output "hello from function" }
greet
```
expected(`hello from function`)。

- [ ] **Step 2: `22_func_args`**

`cases/22_func_args/ash.ash`:
```ash
fn add(a, b) {
    print(a + b)
}
add(3, 4)
```
`cases/22_func_args/bash.sh`:
```bash
#!/bin/bash
add() { echo $(($1 + $2)); }
add 3 4
```
`cases/22_func_args/pwsh.ps1`:
```powershell
function add($a,$b) { Write-Output ($a + $b) }
add 3 4
```
expected(`7`)。整数加法,规避 .to_uint()。

- [ ] **Step 3: `23_func_return`**

`cases/23_func_return/ash.ash`:
```ash
fn square(x) {
    return x * x
}
var r = square(6)
print(r)
```
`cases/23_func_return/bash.sh`:
```bash
#!/bin/bash
square() { echo $(($1 * $1)); }
r=$(square 6)
echo "$r"
```
`cases/23_func_return/pwsh.ps1`:
```powershell
function square($x) { return $x * $x }
$r = square 6
Write-Output $r
```
expected(`36`)。

- [ ] **Step 4: `24_recursion` — 阶乘**

`cases/24_recursion/ash.ash`:
```ash
fn fact(n) {
    if n <= 1 {
        return 1
    }
    return n * fact(n - 1)
}
print(fact(5))
```
`cases/24_recursion/bash.sh`:
```bash
#!/bin/bash
fact() {
  if [ "$1" -le 1 ]; then echo 1; else
    local prev=$(fact $(($1 - 1)))
    echo $(($1 * prev))
  fi
}
echo "$(fact 5)"
```
`cases/24_recursion/pwsh.ps1`:
```powershell
function fact($n) { if ($n -le 1) { return 1 } else { return $n * (fact ($n-1)) } }
Write-Output (fact 5)
```
expected(`120`)。

- [ ] **Step 5: 实测验证 + 生成 expected + 提交**

```bash
cd ash/auto-shell/tests/parity/cases
for d in 2[1-4]_*; do (cd "$d" && bash bash.sh > expected.txt); done
cd ../../../../..
cd ash/auto-shell && cargo test --test parity -- parity_all_cases --nocapture 2>&1 | tail -30
```
```bash
git add ash/auto-shell/tests/parity/cases/2[1-4]_* ash/auto-shell/tests/parity/README.md
git commit -m "feat(037): parity cases 21-24 (D-class: functions & recursion)

Refs designs/037."
```

---

## Task 9: E 类用例 — 文件操作(25-28)

**Files:** `cases/{25_file_write_read,26_file_append,27_file_exists,28_file_count_lines}/`

文件操作用 `>` shell 行(echo/cat/重定向)。**28 计数行用 AutoLang `.lines()`,规避 wc 结构化。**

- [ ] **Step 1: `25_file_write_read`**

`cases/25_file_write_read/ash.ash`:
```ash
> echo "line one" > fw.txt
> echo "line two" >> fw.txt
> cat fw.txt
```
`cases/25_file_write_read/bash.sh`:
```bash
#!/bin/bash
echo "line one" > fw.txt
echo "line two" >> fw.txt
cat fw.txt
```
`cases/25_file_write_read/pwsh.ps1`:
```powershell
"line one","line two" | Set-Content fw.txt
Get-Content fw.txt
```
expected(`line one\nline two`)。

- [ ] **Step 2: `26_file_append`**

`cases/26_file_append/ash.ash`:
```ash
> echo "first" > app.txt
> echo "second" >> app.txt
> echo "third" >> app.txt
> cat app.txt
```
`cases/26_file_append/bash.sh`:
```bash
#!/bin/bash
echo "first" > app.txt
echo "second" >> app.txt
echo "third" >> app.txt
cat app.txt
```
`cases/26_file_append/pwsh.ps1`:
```powershell
"first" | Set-Content app.txt
"second","third" | Add-Content app.txt
Get-Content app.txt
```
expected(`first\nsecond\nthird`)。

- [ ] **Step 3: `27_file_exists`**

用 AutoLang + system_status 检测(实测 `system_status()` 返回 exit code 可用)。
`cases/27_file_exists/ash.ash`:
```ash
> echo "x" > exists.txt
> test -f exists.txt
if system_status() == 0 {
    print("exists")
} else {
    print("missing")
}
```
`cases/27_file_exists/bash.sh`:
```bash
#!/bin/bash
echo "x" > exists.txt
if [ -f exists.txt ]; then echo "exists"; else echo "missing"; fi
```
`cases/27_file_exists/pwsh.ps1`:
```powershell
"x" | Set-Content exists.txt
if (Test-Path exists.txt) { Write-Output "exists" } else { Write-Output "missing" }
```
expected(`exists`)。**注意:`> test -f` 后 `system_status()` 是否反映 test 的退出码需实测;若不工作,改用 AutoLang 捕获 `var r = > test -f exists.txt` + 判断。**

- [ ] **Step 4: `28_file_count_lines` — AutoLang 计数(规避 wc)**

`cases/28_file_count_lines/ash.ash`:
```ash
> echo "a" > lc.txt
> echo "b" >> lc.txt
> echo "c" >> lc.txt
var content = > cat lc.txt
var count = 0
for l in content.lines() {
    count = count + 1
}
print(count)
```
`cases/28_file_count_lines/bash.sh`:
```bash
#!/bin/bash
echo "a" > lc.txt
echo "b" >> lc.txt
echo "c" >> lc.txt
wc -l < lc.txt
```
`cases/28_file_count_lines/pwsh.ps1`:
```powershell
"a","b","c" | Set-Content lc.txt
Write-Output (Get-Content lc.txt).Count
```
expected(`3`)。**注意:bash `wc -l < lc.txt` 输出 `3`(无文件名);ash 用 AutoLang 计数得 `3`。但注意 `.lines()` 对末尾换行的计数可能与 wc 差 1(末尾换行后空行),实测确认。若差 1,调整 ash 逻辑或标 KNOWN_FAIL。**

- [ ] **Step 5: 实测验证 + 生成 expected + 提交**

```bash
cd ash/auto-shell/tests/parity/cases
for d in 2[5-8]_*; do (cd "$d" && bash bash.sh > expected.txt); done
cd ../../../../..
cd ash/auto-shell && cargo test --test parity -- parity_all_cases --nocapture 2>&1 | tail -30
```
```bash
git add ash/auto-shell/tests/parity/cases/2[5-8]_* ash/auto-shell/tests/parity/README.md
git commit -m "feat(037): parity cases 25-28 (E-class: file ops)

Refs designs/037. Line-count uses AutoLang loop (wc is structured in ash)."
```

---

## Task 10: F 类用例 — 文本数据处理(29-32)

**Files:** `cases/{29_grep,30_sort_uniq,31_head_tail,32_text_pipeline}/`

F 类是 ash 能力最受限的(grep/head/tail/wc/awk 结构化或缺失)。按实测矩阵,**用纯文本命令(sort/uniq/cut/sed)和 AutoLang 原生** 实现等价逻辑。

- [ ] **Step 1: `29_grep` — 用 AutoLang 过滤(规避 grep 结构化)**

`cases/29_grep/ash.ash`:
```ash
> echo "apple" > gl.txt
> echo "banana" >> gl.txt
> echo "apricot" >> gl.txt
> echo "cherry" >> gl.txt
var content = > cat gl.txt
for l in content.lines() {
    if l.find("ap") >= 0 {
        print(l)
    }
}
```
`cases/29_grep/bash.sh`:
```bash
#!/bin/bash
echo "apple" > gl.txt
echo "banana" >> gl.txt
echo "apricot" >> gl.txt
echo "cherry" >> gl.txt
grep "ap" gl.txt
```
`cases/29_grep/pwsh.ps1`:
```powershell
"apple","banana","apricot","cherry" | Set-Content gl.txt
Select-String "ap" gl.txt | ForEach-Object { $_.Line }
```
expected(`apple\napricot`)。**注意:`.find()` 返回 -1 表示未找到(实测),`>= 0` 判断存在性。**

- [ ] **Step 2: `30_sort_uniq` — 用纯文本 sort|uniq**

实测 `sort|uniq -c` 纯文本可用(`      2 alpha`)。但 `-c` 带计数格式有前导空格。用不带计数的纯去重更稳。
`cases/30_sort_uniq/ash.ash`:
```ash
> echo "banana" > su.txt
> echo "apple" >> su.txt
> echo "banana" >> su.txt
> echo "apple" >> su.txt
> cat su.txt | sort | uniq
```
`cases/30_sort_uniq/bash.sh`:
```bash
#!/bin/bash
echo "banana" > su.txt
echo "apple" >> su.txt
echo "banana" >> su.txt
echo "apple" >> su.txt
cat su.txt | sort | uniq
```
`cases/30_sort_uniq/pwsh.ps1`:
```powershell
"banana","apple","banana","apple" | Sort-Object -Unique
```
expected(`apple\nbanana`)。

- [ ] **Step 3: `31_head_tail` — 用 sed -n(规避 head/tail 缺失)**

实测 `head -N`/`tail -N` 报错,但 `sed -n 1,2p` 纯文本可用。
`cases/31_head_tail/ash.ash`:
```ash
> echo "a" > ht.txt
> echo "b" >> ht.txt
> echo "c" >> ht.txt
> cat ht.txt | sed -n 1,2p
```
`cases/31_head_tail/bash.sh`:
```bash
#!/bin/bash
echo "a" > ht.txt
echo "b" >> ht.txt
echo "c" >> ht.txt
head -n 2 ht.txt
```
`cases/31_head_tail/pwsh.ps1`:
```powershell
"a","b","c" | Set-Content ht.txt
Get-Content ht.txt | Select-Object -First 2
```
expected(`a\nb`)。

- [ ] **Step 4: `32_text_pipeline` — 综合(用 AutoLang + sort)**

日志分析:统计某关键字出现次数 + 排序。用 AutoLang 计数 + sort。
`cases/32_text_pipeline/ash.ash`:
```ash
> echo "ERROR disk" > log.txt
> echo "INFO cpu" >> log.txt
> echo "ERROR cpu" >> log.txt
> echo "ERROR disk" >> log.txt
var content = > cat log.txt
var err_count = 0
for l in content.lines() {
    if l.find("ERROR") >= 0 {
        err_count = err_count + 1
    }
}
print("errors: " + err_count)
```
`cases/32_text_pipeline/bash.sh`:
```bash
#!/bin/bash
echo "ERROR disk" > log.txt
echo "INFO cpu" >> log.txt
echo "ERROR cpu" >> log.txt
echo "ERROR disk" >> log.txt
echo "errors: $(grep -c ERROR log.txt)"
```
`cases/32_text_pipeline/pwsh.ps1`:
```powershell
"ERROR disk","INFO cpu","ERROR cpu","ERROR disk" | Set-Content log.txt
$errs = (Select-String "ERROR" log.txt).Count
Write-Output "errors: $errs"
```
expected(`errors: 3`)。**注意:`"errors: " + err_count` 字符串+整数拼接需实测;若失败用 `> echo "errors: $err_count"` 走 shell 插值。**

- [ ] **Step 5: 实测验证 + 生成 expected + 提交**

```bash
cd ash/auto-shell/tests/parity/cases
for d in 29_* 3[0-2]_*; do (cd "$d" && bash bash.sh > expected.txt); done
cd ../../../../..
cd ash/auto-shell && cargo test --test parity -- parity_all_cases --nocapture 2>&1 | tail -30
```
```bash
git add ash/auto-shell/tests/parity/cases/29_* ash/auto-shell/tests/parity/cases/3[0-2]_* ash/auto-shell/tests/parity/README.md
git commit -m "feat(037): parity cases 29-32 (F-class: text processing)

Refs designs/037. grep/head/wc replaced with AutoLang native + sed/sort
(ash structured-output limitation)."
```

---

## Task 11: 全量回归 + KNOWN_FAIL 记录 + 最终提交

- [ ] **Step 1: 全量运行,统计 pass/fail**

Run: `cd ash/auto-shell && cargo test --test parity -- parity_all_cases --nocapture 2>&1 | tail -60`
统计:passed 数、failures 列表。目标 ≥25 通过。

- [ ] **Step 2: 对每个 fail 的 case,实测 ash 单独输出,判定原因**

对每个失败 case:
```bash
ASH=ash/target/debug/ash
cd cases/NN_xxx && "$ASH" ash.ash; echo "[ash exit=$?]"; echo "---expected---"; cat expected.txt
```
分类原因:(a) 结构化输出 → 标 KNOWN_FAIL;(b) 语法错误 → 尝试修正;(c) 算术 VM bug → 标 KNOWN_FAIL。

- [ ] **Step 3: 更新 README 已知差异表**

把所有 KNOWN_FAIL case 填入 `tests/parity/README.md` 的"已知差异(MVP)"段,格式:
```
- `08_str_len` — ash `.len()` 语义与 bash `${#}` 不同(待确认)
- `NN_xxx` — 原因:...
```

- [ ] **Step 4: 确认验收**

- 32 个 case 目录全部存在(`ls cases/ | wc -l` ≥ 32)
- `cargo test --test parity` 通过(已知差异已记录,不阻塞)
- ≥25 个 case 在 failures 列表外

- [ ] **Step 5: 最终提交**

```bash
git add ash/auto-shell/tests/parity/
git commit -m "docs(037): finalize parity suite — KNOWN_FAIL table + MVP acceptance

MVP: 32 cases, N passing (>=25), M KNOWN_FAIL recorded.
Refs designs/037 §4. Future: structured-command bash-compat mode,
fish/nu, full coverage after VM bug fixes."
```

- [ ] **Step 6: 更新 spec 状态**

把 `designs/037-script-parity-testsuite.md` 顶部"状态:设计已确认,待实施"改为"状态:MVP 已实施(N/32 通过)"。

---

## 自检清单(计划完成后回顾)

**Spec 覆盖**:
- [x] R2(CARGO_BIN_EXE_ash)→ Task 1
- [x] R3(exit-code)→ Task 2
- [x] normalize 路径占位 → Task 3
- [x] 修正 01_echo → Task 4
- [x] 32 case(A-F 六类)→ Task 5-10
- [x] ≥25 跑通 + KNOWN_FAIL 记录 → Task 11
- [x] README + 已知差异表 → Task 4/11

**实测依据**:所有 ash case 代码基于 2026-07-23 实测矩阵(echo/print/var/cat/sort/uniq/cut/sed/tr/exit 纯文本;wc/grep/head/tail/awk 结构化或缺失)。

---

## ⚠️ MVP 实施结果与计划偏差(2026-07-23 实施后记录)

本计划的 Task 1-11 是按"从零写 32 个 case"设计的,但**实施时发现仓库已有 Plan 036 建好的 50 个 case**(commit `e9285f3`/`9f62063`),覆盖全部 A-G 七大类。实际实施路径因此改变:

| 计划设想 | 实际情况 |
|---------|---------|
| 写 32 个新 case | 仓库已有 50 个 case(全 A-G 类) |
| 预期 ≥25 跑通,其余 KNOWN_FAIL | **50/50 全部跑通**(0 KNOWN_FAIL) |
| 预期 ash 用 `>` shell 命令写 | 实际 50 case 全用 **AutoLang 原生实现**(规避结构化命令) |
| 只修 R1/R2/R3 | 实际额外修了 **R4(重复换行)+ WSL bash 误选**,这才是 37 分歧的根因 |

**已完成的 commit**:`b72cf8b`(harness)、`6fbe0ff`(R4+WSL)、`b68f3a7`(docs)。Task 1-3 + Task 4 的 01_echo 修正 + R4 + WSL 修复均已落地。Task 5-11(批量写 case)因仓库已有而**无需执行**。

**关键洞察**:50/50 全过是**真实的**,但它测的是"AutoLang 逻辑等价性",**不是"shell 命令 parity"**。真正的 shell 命令(`> grep`/`> wc`/`> ls`)在 ash 里仍输出结构化表格,与 bash 不一致——这是下一段(后续工作 P1)要解决的。

---

## 后续工作(Phase 2+)

按优先级排序。每项含:现状、实施路径(已调研)、工作量评估。

### P1: 结构化命令 bash 兼容输出模式 ⭐最高优先级

**现状**:ash 的 `ls`/`grep`/`wc`/`ps` 等输出 ratatui 表格,与 bash 纯文本不一致。实测验证(2026-07-23):
- `> grep apple f.txt` → 表格(带 `\\?\C:\...` 路径列),bash 是 `apple\napple`
- `> cat f | wc -l` → `lines: 3`,bash 是 `3`
- `> ls` → name/permissions/type/size/modified 五列表格,bash 是 `file1\nfile2`

当前 50 case 全用 AutoLang 模拟规避了这点,但**真正的 shell 命令 parity 缺失**。

**实施路径**(已精确调研,照抄 `--json` 三件套):
- **Step 1**(shell.rs):加 `bash_compat: bool` 字段(行 136 后)+ 构造初始化 + `set_bash_compat` setter(照抄 `set_json_output` 行 936)
- **Step 2**(新建 `ash-core/src/cmd/bash_compat.rs` 或 value_helpers.rs):写 `format_atom_as_bash(atom) -> Option<String>`,按 `AtomType` 分发:
  - `FileList`/`FileEntry` → 每行一个 name(`ls` 默认)
  - `ProcessList`/`ProcessEntry` → 经典 `ps` 列
  - `MatchList` → `file:line:content`(grep -n)或纯行
  - `CountResult` → 纯数字 + `\n`(wc 风格)
  - `Path` → 原样字符串
  - 其他 → `None`(落 fallback `into_text`)
  - 复用 `format_value_for_table`(value_helpers.rs:164)提取字段,**不复用** `format_array_as_table`(带表头)
- **Step 3**(shell.rs `format_output` 行 885 后):注入分支 `if self.bash_compat { ... }`,在 json 之后、表格之前
- **Step 4**(main.rs):加 `--bash-compat` flag(照抄 `--json` 预扫描行 40 + skip 行 53 + 三处应用)。**注意 `-c` 路径的 `execute_for_agent` 会 reset 字段**,需扩签名或改 set+execute
- **Step 5**:加 parity case `51_ls_bash`/`52_grep_bash`/`53_wc_bash`(用 `--bash-compat` 跑 ash,对比真实 bash ls/grep/wc)

**工作量**:核心 < 150 行,触及 3 文件(shell.rs、main.rs、新格式器)。风险:`-c` 路径 reset 问题。

**验收**:新增的 `51-53` case 通过(strict 模式),且现有 50 case 不回归。

### P2: 真实 shell 命令 case(依赖 P1)

**现状**:当前 50 case 全是 AutoLang 模拟。P1 完成后,可补**真实 shell 命令版** case:
- `> ls` / `> ls -l` / `> ls -a`
- `> grep pattern file` / `> grep -c` / `> grep -n`
- `> wc -l/-w/-c`
- `> ps` / `> ps aux`
- `> find . -name x`

**实施**:每个 case 的 ash 版用 `--bash-compat` 模式(或脚本内 `set_bash_compat`),bash 版用真实命令。预计 10-15 个新 case。

### P3: fish/nu shell 变体覆盖

**现状**:harness 已有 `run_fish`/`run_nu` runner 和 best-effort WARNING 逻辑(parity.rs),但 case 目录里 `fish.fish`/`nu.nu` 多为空或简单。fish/nu 语法与 bash 差异大,某些场景无法精确对应。

**实施**:为高价值 case 补 fish/nu 版本;无法对应的标 `skip_shells: [nu]`(需在 harness 加 skip 机制,读 `desc.md`)。

### P4: CI 集成

**现状**:`parity_all_cases` 默认是 warning 模式(不 fail),需 `ASH_PARITY_STRICT=1` 才 fail。CI 未集成。

**实施**:在 CI 配置加 `ASH_PARITY_STRICT=1 cargo test --test parity`,让 parity 回归成为 CI 门禁。需确保 CI 环境有 Git bash(Windows)或 bash(Linux)。

### P5: 交互式 REPL 的 R4 回归验证

**现状**:R4 修复(`print_command_output`)改了 `execute_script_content` 和 `execute_with_stdin` 的输出路径。交互式 REPL 走不同路径(`Repl::run`),理论上不受影响,但**未做交互模式回归测试**。

**实施**:手动验证 `ash`(交互式)下 `echo hello`、`ls`、`cat file` 输出正常(无多余空行、表格渲染正常)。

---

## 后续工作优先级建议

1. **P1(结构化命令 bash 兼容模式)** — 核心能力缺口,解锁 P2,工作量可控(<150 行)
2. **P2(真实 shell 命令 case)** — 依赖 P1,补齐"shell 命令 parity"的真正验证
3. **P5(REPL 回归)** — 快速验证 R4 无副作用,应尽早做
4. **P4(CI 集成)** — 让 parity 成为持续门禁
5. **P3(fish/nu)** — 锦上添花,优先级最低

