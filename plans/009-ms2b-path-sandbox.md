# Plan 009: MS2-B — 路径沙盒（--sandbox + 中央 path resolver + 命令重构）

- **日期**: 2026-07-02
- **状态**: 待实施
- **RoadMap**: MS2（`docs/roadmap.md` §Milestone 2）
- **依赖**: Plan 008（共享 `SecurityPolicy` 结构，沙盒字段挂在这里）

- **目标**: `ash -c "rm -rf /" --sandbox /tmp` 被拦截；所有文件操作（读/写/cd）限制在 `--sandbox <dir>` 内；符号链接穿透到沙盒外被拒绝。同时抽出中央 path resolver，把散落在 ~20 个命令里的 `is_absolute / cwd.join` 重复代码统一。

> **与 Plan 008 的关系**：008 做"命令级"拦截（拦进程 spawn / 网络 / 危险模式 / 审计）。本 plan 做"文件级"拦截——限制命令能访问的**路径范围**。两者都挂在 `SecurityPolicy` 上，但本 plan 还要做一次**路径解析层重构**（最大工作量），这是 MS2 风险最高的部分。

## 1. 背景与现状

### 调研结论（2026-07-02 实读代码）

**核心问题：路径解析完全分散，无中央 resolver。**

ash 里**每个文件命令自己解析路径、自己直接调 `std::fs`**，没有任何集中拦截点。要把文件操作限制在沙盒内，必须先统一路径解析。重复的 idiom 有**三个变体**：

**变体 1 — fs.rs（legacy builtin，`current_dir: &Path` 参数）**，6 处：
```rust
// fs.rs:482 (mkdir), 499 (rm), 524+530 (mv), 547+553 (cp)
let target = if path.is_absolute() {
    path.to_path_buf()
} else {
    current_dir.join(path)
};
```

**变体 2 — 新式注册命令（`shell.pwd()` 查询）**，多处：
```rust
// touch.rs:101, cat.rs:84, head.rs:87, ln.rs:43+49, cp.rs:50+56, rm.rs:48, mkdir.rs:46
let path = if Path::new(arg).is_absolute() {
    arg.into()
} else {
    shell.pwd().join(arg)
};
```

**变体 3 — 不解析直接读**（最危险，沙盒能绕过）：
```rust
// grep.rs:91/272, find.rs, glob.rs, show.rs, tail.rs, cut.rs, diff.rs ...
let content = std::fs::read_to_string(path).into_diagnostic()?;  // path 未经 resolve！
```

**直接碰 `std::fs` 的命令**（18 个）：cat, cut, diff, du, file, find, fmt, glob, grep, head, open, paste, show, sort, split, tail, tee, touch + legacy fs.rs（mkdir/rm/mv/cp）+ ln。

**已有 canonicalize**：只有 `change_dir`（shell.rs:962）做了 `canonicalize()`。其他命令都不 canonicalize，**符号链接穿透完全可能**。

**外部命令的 cwd**：`execute_external`（shell.rs:569）和 `execute_external_with_redirect`（shell.rs:621）都 `.current_dir(&self.current_dir)` 把 shell cwd 传给子进程——子进程不受沙盒约束（外部进程能任意访问文件系统），这点要文档明确。

### 为什么必须先重构路径解析

没有中央 resolver，沙盒只能在**每个**命令里重复检查（20+ 处，易漏易错）。抽出 `Shell::resolve_path(arg) -> Result<PathBuf>` 后：
1. 沙盒检查集中在一个函数
2. canonicalize 集中在一处（修掉符号链接穿透）
3. 20+ 处重复 idiom 统一成一行调用

### 不在本期范围（YAGNI）

- **外部子进程的文件访问沙盒**：外部进程（git/curl/系统工具）一旦 spawn，ash 无法约束其 fs 访问（OS 级需 chroot/seccomp，远超本期）。→ 文档明确：`--sandbox` 只约束 **ash 内置/注册命令**的文件操作；要完全隔离外部进程，结合 Plan 008 的 `--no-exec` 或用容器。RoadMap §MS2 的验收 `ash -c "rm -rf /" --sandbox /tmp` 中 `rm` 走的是 ash 内置（registry/fs.rs），所以能拦。
- **写时沙盒（copy-on-write overlay）**：nushell 风格的"虚拟文件系统"。→ 远超本期，留后续。
- **Windows 符号链接/reparse point 的完全覆盖**：本期做 `canonicalize()`（Windows 上解析 reparse point），但 Windows 的 junction/UNC 边界 case 不追求 100%。

## 2. 设计

### 行为契约

| # | 规则 |
|---|------|
| 1 | 无 `--sandbox`：行为与现状完全一致（resolver 仍 canonicalize，但不做边界检查），向后兼容 |
| 2 | `--sandbox <dir>`：所有 ash 命令的文件操作（读/写/cd）的目标路径必须在 `<dir>` 内 |
| 3 | 相对路径：先 join 到 cwd，再 canonicalize，再检查是否在沙盒内 |
| 4 | 绝对路径：直接 canonicalize，检查是否在沙盒内 |
| 5 | 符号链接穿透：canonicalize 后若落在沙盒外 → 拒绝（`Err` + stderr 诊断 + 非零退出码）|
| 6 | `cd <dir>`：目标必须在沙盒内，否则拒绝（cwd 不能离开沙盒）|
| 7 | `--sandbox` 隐含 `--read-only` 关闭？否：沙盒内仍可写（除非叠加 `--read-only`）。沙盒管"在哪写"，read-only 管"能不能写"|
| 8 | 重定向目标（`> file`，shell.rs:581 `apply_output_redirect`）：也要过沙盒检查 |
| 9 | 沙盒边界用 canonicalize 后的路径前缀比较（`sandbox.canonicalize()` 是前缀）|
| 10 | 所有拒绝走 `Result<Err>`（不 panic）|

### 中央 path resolver

```rust
impl Shell {
    /// Plan 009: 统一路径解析 + 沙盒边界检查。
    /// - 相对路径 join 到 cwd
    /// - canonicalize（解析符号链接）
    /// - 若 --sandbox 开启，检查最终路径是否在沙盒内
    /// 返回 canonicalize 后的绝对路径；越界返回 Err。
    pub fn resolve_path(&self, arg: &str) -> Result<PathBuf> {
        let p = std::path::Path::new(arg);
        let joined = if p.is_absolute() { p.to_path_buf() }
                     else { self.current_dir.join(p) };
        // canonicalize 解析符号链接；路径可能不存在（创建场景），用父目录 canonicalize 兜底
        let canonical = canonicalize_or_parent(&joined)?;
        if let Some(ref sandbox) = self.policy.sandbox_dir {
            let sandbox_canon = sandbox.canonicalize().into_diagnostic()
                .map_err(|e| miette!("sandbox: invalid --sandbox {}: {}", sandbox.display(), e))?;
            if !canonical.starts_with(&sandbox_canon) {
                miette::bail!(
                    "sandbox: {} is outside sandbox {}",
                    canonical.display(), sandbox_canon.display()
                );
            }
        }
        Ok(canonical)
    }
}
```

**canonicalize 不存在路径的处理**（关键 corner case）：`touch newfile` 时文件还不存在，`canonicalize()` 会失败。用 `canonicalize_or_parent`：先试 canonicalize 整路径，失败则 canonicalize 父目录 + 拼文件名。这样新建文件也能检查"目标目录是否在沙盒内"。

### 沙盒如何拦截 —— 分层

```
命令路径
  │
  ├─① 注册命令（cat/grep/rm/...）的 run() handler
  │     · 把内部的 std::fs::xxx(path) 前的 path 改成 shell.resolve_path(arg)?
  │     · resolve_path 自带沙盒检查 → 越界即 Err
  │
  ├─② legacy builtin（fs.rs mkdir/rm/mv/cp）
  │     · 同上，改成走 resolve_path
  │     · 问题：fs.rs 函数签名是 (path, current_dir, ...) 不拿 Shell
  │     · 方案 A：改签名传 &Shell（波及 builtin.rs dispatch）
  │     · 方案 B（推荐）：fs.rs 函数接收一个 Fn(&str)->PathBuf 的 resolver 闭包
  │
  ├─③ cd（shell.rs:958 change_dir）
  │     · canonicalize 后检查沙盒边界
  │
  ├─④ 重定向目标（shell.rs:581 apply_output_redirect）
  │     · open 前过 resolve_path
  │
  └─⑤ 外部命令：不拦（文档声明，靠 --no-exec 或容器隔离）
```

### `SecurityPolicy` 扩展（Plan 008 的结构加字段）

```rust
pub struct SecurityPolicy {
    // ... Plan 008 字段 ...
    pub sandbox_dir: Option<PathBuf>,   // Plan 009: 路径沙盒根
    pub read_only: bool,                 // Plan 008 标记；Plan 009 补全路径级写拦截
}
```

`read_only` 路径级拦截：`resolve_path` 加 `for_write: bool` 参数；`read_only` 时所有 `for_write=true` 的 resolve 返回 Err。命令调用时标明读/写：
```rust
pub fn resolve_path(&self, arg: &str, for_write: bool) -> Result<PathBuf> { ... }
// rm/mv/cp/mkdir/touch/ln/tee → for_write=true
// cat/head/grep/show/ls/find → for_write=false
```

## 3. 实现架构

### 改动 A：`Shell::resolve_path` + `canonicalize_or_parent`

`shell.rs`：
- 新增 `resolve_path(&self, arg: &str, for_write: bool) -> Result<PathBuf>`（上述设计）
- 新增 `canonicalize_or_parent(path: &Path) -> Result<PathBuf>` 辅助函数
- 单元测试：相对/绝对路径、符号链接穿透、不存在路径（父目录兜底）、沙盒内外

### 改动 B：注册命令改用 resolve_path（逐个迁移）

按读写分类，**逐命令迁移**（每个命令一个 commit，便于回归）：

**写命令**（for_write=true，受 read_only + sandbox 约束）：
| 命令 | 文件 | 当前 resolve 处 | 迁移动作 |
|------|------|----------------|---------|
| touch | touch.rs:101 | `resolve_touch_path` | 改调 `shell.resolve_path(arg, true)` |
| rm（新） | rm.rs:48 | `pwd().join` | 改调 resolve_path(arg, true) |
| rm（legacy） | fs.rs:499 | `current_dir.join` | 见改动 C |
| mv（新+legacy） | mv.rs:44 / fs.rs:523 | pwd().join / current_dir.join | resolve_path(arg, true) 双路径 |
| cp（新+legacy） | cp.rs:50 / fs.rs:546 | 同上 | resolve_path(arg, ...) |
| mkdir（新+legacy） | mkdir.rs:46 / fs.rs:482 | 同上 | resolve_path(arg, true) |
| ln | ln.rs:43+49 | pwd().join | resolve_path(arg, true) |
| tee | tee.rs | std::fs::write | resolve_path(arg, true) |

**读命令**（for_write=false，只受 sandbox 约束）：
| 命令 | 文件 | 迁移动作 |
|------|------|---------|
| cat | cat.rs:84 | resolve_path(arg, false) |
| head | head.rs:87 | resolve_path(arg, false) |
| tail | tail.rs | resolve_path(arg, false) |
| grep | grep.rs:91/272 | resolve_path(arg, false) |
| show | show.rs | resolve_path(arg, false) |
| find | find.rs | resolve_path(arg, false)（根路径）|
| glob | glob.rs | resolve_path(arg, false) |
| cut/diff/du/file/fmt/paste/sort/split | 各自文件 | resolve_path(arg, false) |
| open | open.rs | resolve_path(arg, false) |
| ls | ls.rs（经 fs.rs:13 ls_command） | resolve_path(arg, false) |

> **迁移策略**：不做"大爆炸"重构。先建 resolve_path（改动 A）+ 测试，然后按命令迁移，每批跑回归。读命令优先（风险低），写命令后做（read_only 拦截在这批验证）。

### 改动 C：legacy fs.rs 函数接 resolver

fs.rs 的 `mkdir_command`/`rm_command`/`mv_command`/`cp_command` 签名是 `(path, current_dir, ...)`，不拿 Shell。两个方案：

**方案 B（推荐）**：加一个 resolver 参数：
```rust
pub fn rm_command(
    path: &Path, current_dir: &Path, recursive: bool,
    resolver: &dyn Fn(&str) -> Result<PathBuf>,  // ← 新增
) -> Result<String> {
    let target = resolver(&path.to_string_lossy())?;  // 替代 is_absolute/join idiom
    // ... 原 std::fs 逻辑用 target ...
}
```
调用点（builtin.rs:25 `execute_builtin`）传入闭包 `&|arg| shell.resolve_path(arg, true)`。

> 这样 fs.rs 不依赖 Shell（保持 ash-core/lib 纯净），又获得了沙盒能力。ls_command（读）同理传 for_write=false 的 resolver。

### 改动 D：cd 沙盒检查 + 重定向检查

- `change_dir`（shell.rs:958）：canonicalize 后加沙盒边界检查（复用 resolve_path 的检查逻辑，或直接调 `resolve_path(path, false)`）。
- `apply_output_redirect`（shell.rs:581）：`File::create(path)` 前调 resolve_path(path, true)。

### 改动 E：main.rs 加 `--sandbox <dir>` flag

main.rs while 循环加：
```rust
"--sandbox" => {
    i += 1;
    if i >= args.len() { eprintln!("ash --sandbox: requires argument"); std::process::exit(2); }
    policy.sandbox_dir = Some(PathBuf::from(&args[i]));
}
```
config `[security]` 段加 `sandbox = /path/to/dir`。

## 4. 测试策略

| 层级 | 测试 | 方式 |
|---|---|---|
| 单元 | `resolve_path` 相对/绝对/canonicalize | tempdir + Shell |
| 单元 | `resolve_path` 沙盒内放行 / 越界拒绝 | tempdir sandbox |
| 单元 | 符号链接穿透（沙盒内链接 → 沙盒外目标）拒绝 | tempdir + symlink |
| 单元 | 不存在路径（touch 新文件）父目录兜底 | tempdir |
| 单元 | read_only：写命令 resolve for_write=true 拒绝 | Shell + policy |
| 集成 | `rm` 在沙盒内成功 / 沙盒外拒绝 | Shell + tempdir |
| 集成 | `cat` 读沙盒外文件拒绝 | Shell + tempdir |
| 集成 | `cd` 跳出沙盒拒绝 | Shell |
| 集成 | 重定向 `> ../out` 越界拒绝 | Shell |
| CLI | `ash -c "rm -rf /" --sandbox /tmp` 被拦截 | 子进程 + tempdir |
| CLI | `ash -c "touch f" --sandbox . --read-only` 被拒（read-only）| 子进程 |
| 回归 | 无 --sandbox 时全量 cargo test 通过 | 全量 |

### TDD 流程

1. RED：`resolve_path` 单测（各 case）→ 失败（函数不存在）
2. GREEN：实现 resolve_path + canonicalize_or_parent + 沙盒检查
3. RED：迁移一个读命令（cat）→ 沙盒外读拒绝 → 失败（cat 未走 resolve_path）
4. GREEN：cat 改走 resolve_path；回归
5. 逐命令迁移（每批 RED→GREEN→回归）：读命令批 → 写命令批 → cd → 重定向
6. RED：read_only 路径级拦截（touch for_write=true）→ 失败
7. GREEN：resolve_path for_write 参数 + 写命令传 true
8. RED：`--sandbox` CLI 测试 → 失败
9. GREEN：main.rs 加 flag + config
10. 全量回归 + 手动验证

## 5. 实施步骤

1. `Shell::resolve_path` + `canonicalize_or_parent` + 单测（沙盒边界 / 符号链接 / 不存在路径）。
2. `SecurityPolicy` 加 `sandbox_dir` 字段 + config `[security] sandbox` 读取。
3. 迁移**读命令**批（cat/head/grep/show/tail/find/glob/ls + 其他读命令）：每个改走 `resolve_path(arg, false)`，逐个回归。
4. 迁移**写命令**批（touch/rm/mv/cp/mkdir/ln/tee）：改走 `resolve_path(arg, true)`。
5. legacy fs.rs 函数加 resolver 参数（方案 B）；builtin.rs dispatch 传闭包。
6. `change_dir` 加沙盒检查；`apply_output_redirect` 加沙盒检查。
7. `resolve_path` 加 `for_write` 参数；read_only 路径级拦截；写命令标 for_write=true。
8. `main.rs` 加 `--sandbox <dir>` flag；config 集成。
9. 全量回归（cargo test）+ 手动验证验收标准。
10. 提交 + push。

> **建议分多个 commit**：resolve_path 基础设施 / 读命令迁移 / 写命令迁移 / cd+重定向 / read_only / CLI flag。便于回归定位。

## 6. 验收标准

- [ ] `ash -c "rm -rf /" --sandbox /tmp` 被拦截（rm 走内置，路径越界）
- [ ] `ash -c "touch /tmp/f" --sandbox /tmp` 成功（沙盒内）
- [ ] `ash -c "touch /etc/x" --sandbox /tmp` 被拒（越界）
- [ ] `ash -c "cat /etc/passwd" --sandbox /tmp` 被拒（读越界）
- [ ] `ash -c "cd .." --sandbox /tmp/sub` 被拒（cd 跳出沙盒）
- [ ] 符号链接穿透：沙盒内 `ln -s /etc/passwd link && cat link` 被拒
- [ ] `ash -c "echo hi > out" --sandbox /tmp` 重定向在沙盒内成功；`> ../out` 越界被拒
- [ ] `ash -c "touch f" --read-only` 被拒（路径级写拦截，补全 Plan 008 的命令名级）
- [ ] 无 `--sandbox` / `--read-only` 时，全量 cargo test 通过，行为零变化
- [ ] 外部命令（`git status`）在 `--sandbox` 下不崩（文档声明不约束外部进程，但能正常 spawn 或被 `--no-exec` 拦）

## 7. 风险

- **迁移面大（18+ 命令）**：最大风险。漏迁一个 = 沙盒漏洞。→ 缓解：用 grep 找全 `std::fs::` 调用点建清单，逐个打勾；每批迁移后跑全量回归；读命令先迁（风险低）验证 resolver 正确，再迁写命令。
- **canonicalize 跨平台**：Windows 上 `canonicalize` 返回 `\\?\` 前缀，可能影响 `starts_with` 比较。→ 缓解：沙盒根也走 canonicalize（双方都带前缀，比较一致）；测试覆盖 Windows。
- **不存在路径的 canonicalize**：touch 新文件、mkdir 新目录时目标不存在。→ `canonicalize_or_parent` 兜底（canonicalize 父目录 + 文件名）。corner case：父目录也不存在（`mkdir -p a/b/c`）→ 逐级向上找存在的祖先目录 canonicalize。
- **性能**：每次文件操作多一次 canonicalize（syscall）。→ 可接受（命令本来就是 IO bound）；不做 cache（canonicalize 结果可能因 symlink 变化失效）。
- **grep/find/glob 的多文件遍历**：这些命令遍历目录树，resolve_path 只检查入口路径。遍历中遇到 symlink 跳出沙盒？→ 遍历内部也要检查（find 的 `read_dir` 结果每项过沙盒检查，或遍历后 canonicalize 检查）。本期 find/glob 的遍历内 symlink 检查标为"已知限制"，入口路径必查。
- **外部进程绕过**：`ash -c "git clean -fdx" --sandbox /tmp` —— git 是外部进程，不受沙盒约束。→ 文档明确（§1 已声明）；用户要完全隔离需 `--no-exec` 或容器。这是 OS 级限制，非 ash 能解。
- **向后兼容**：无 `--sandbox` 时 resolve_path 仍 canonicalize（路径行为可能微变，如 `cat ./file` 变成绝对路径）。→ canonicalize 只影响内部 PathBuf 表示，不影响命令输出；回归测试验证无破坏。
