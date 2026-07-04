# Plan 011: MS3-B — Shell 桥接（system() / exit / export 从 AutoLang 调 shell）

- **日期**: 2026-07-04
- **状态**: 待实施
- **RoadMap**: MS3（`docs/roadmap.md` §Milestone 3）
- **依赖**: Plan 010（建议先做，但技术上独立）
- **目标**: 让 AutoLang 脚本/函数能**调用 shell 命令**、**设置环境变量**、**控制脚本退出码**——补齐 RoadMap §MS3 的 "脚本能写真实自动化" 缺口。`fn deploy() { let out = system("ls -la"); print(out) }` 成为可能。

> **跨仓库**：本 plan 同时改 **auto-lang 仓库**（加 host-context 插槽 + native）和 **ash 仓库**（实现 HostContext + 注册）。这是 MS3 解锁"端到端脚本"的关键。

## 1. 背景与现状

### 调研结论（2026-07-04 实读代码）

**AutoLang 与 shell 是单向文本桥**：
- `execute_script_content`（shell.rs ~2184）按行分类：`>` 前缀 = shell 行，其余 = AutoLang 块。
- `let x = > cmd` 把 shell 命令输出绑定到 AutoLang 变量（`try_capture_assignment`）。
- **但 AutoLang 函数内部无法调用 shell**：没有 `system()`/`exec()`/`shell()` native。`fn foo() { ls }` 不会工作（ls 不是 AutoLang 函数）。
- `exit <code>`、`export VAR=val` 都是 shell builtin，不是 AutoLang 语句。

### 桥接的核心难题

AutoVM 的 native shim 签名是 `Fn(&mut AutoTask, &AutoVM) -> Result<(), VMError>`——**拿不到 Shell**。AutoVM 定义在 auto-lang 仓库，不依赖 ash。所以 system/exit/export 需要：

1. **auto-lang 侧**：定义一个 `ShellHost` trait（`fn system(&mut self, cmd: &str) -> String`、`fn exit(&mut self, code: i32)`、`fn export(&mut self, k: &str, v: &str)`）；AutoVM 加一个 `host: Option<Arc<Mutex<dyn ShellHost>>>` 插槽；注册 native shim 调 `vm.host` 转发。
2. **ash 侧**：实现 `ShellHost for ShellHostImpl`（持有 `&mut Shell` 或通道），在 `Shell::new()` 后把 host 注入 VM。

> **为什么用 trait + Arc 而非直接回调**：AutoVM 是 `Send + Sync`（DashMap/Arc），native shim 在多任务下跑。host 必须线程安全。ash 的 Shell 是 `!Sync`（含 reedline 等），所以 host 实现内部用 `Mutex<...>` 或请求队列。

### 不在本期范围（YAGNI）

- `system()` 的管道组合（`system("ls") | system("sort")`）——本期 system 返回字符串，管道靠 ash 端 dispatch
- `system()` 的退出码返回（本期返回 stdout 字符串；退出码通过单独的 `system_status()` 或返回对象留后续）
- 每个内置命令单独导出为 AutoLang 函数（`ls()`/`cat()`）——本期只做通用 `system(cmd)`，按需后续包装
- `source` 脚本从 AutoLang 调（留后续）

## 2. 设计

### 行为契约

| # | 规则 |
|---|------|
| 1 | `system(cmd: String) -> String`：在 shell 层执行 cmd，返回其 stdout（去尾换行）。错误时返回空串或抛 Err（见下）|
| 2 | `system` 的 cmd 走 shell 完整 dispatch（含安全策略、管道、重定向），与 `>` 前缀等价 |
| 3 | `exit(code: Int)`：终止当前脚本/会话，进程退出码 = code |
| 4 | `export(key: String, val: String)`：设置环境变量（等价 shell `export key=val`），影响后续 system() 子进程 |
| 5 | `system` 受安全策略约束（--sandbox/--read-only/--deny 等）——通过 shell dispatch 自动生效 |
| 6 | `system` 的输出不自动打印（赋值才可见）；要打印用 `print(system("ls"))` |

### `system()` 的返回与错误

返回 `Result<String, String>`？还是 `String`？选 **`String`**（简单），退出码/错误用单独函数：
- `system(cmd) -> String`：返回 stdout（即使命令失败也返回输出；失败时 stderr 不进 stdout）。
- `system_status(cmd) -> Int`：返回退出码（0 成功）。

这样脚本可写：
```auto
fn safe_run(cmd) {
  let out = system(cmd)
  if system_status(cmd) != 0 {
    print("warning: " + cmd + " failed")
  }
  out
}
```

### ShellHost trait（auto-lang 侧）

```rust
// auto-lang: crates/auto-lang/src/host.rs (新文件)
pub trait ShellHost: Send + Sync {
    /// Execute a shell command, return its stdout (trailing newline trimmed).
    fn system(&self, cmd: &str) -> String;
    /// Return the last command's exit code.
    fn system_status(&self) -> i32;
    /// Set an environment variable.
    fn export(&self, key: &str, val: &str);
    /// Request the script to exit with `code` (host stops the run loop).
    fn exit(&self, code: i32);
}
```

### AutoVM 加 host 插槽

`engine.rs` `AutoVM` struct 加：
```rust
pub host: Option<Arc<Mutex<dyn ShellHost>>>,
```
- 默认 `None`（纯 AutoLang 用例不受影响，向后兼容）。
- native shim 调 `vm.host.as_ref().and_then(|h| h.lock().ok()).map(|h| h.system(...))`；host 为 None 时 `system` 返回空串（或抛"no shell"错误）。

### Native 注册

在 `autovm_persistent.rs::new()` 或独立 `init_shell_module()`（模仿 `init_io_module`）注册：
- `system` → 调 `host.system(arg0)`
- `system_status` → `host.system_status()`
- `export` → `host.export(arg0, arg1)`
- `exit` → `host.exit(arg0)`

这些 native 从 task 栈取参数、把返回值压栈（现有 native 模式）。

## 3. 实现架构

### 改动 A：auto-lang 加 host.rs + AutoVM 插槽

- 新文件 `crates/auto-lang/src/host.rs`：`ShellHost` trait。
- `engine.rs`：AutoVM 加 `host` 字段，`AutoVM::new` 初始化 None。
- 暴露 `pub fn set_host(&mut self, host: Arc<Mutex<dyn ShellHost>>)`。

### 改动 B：auto-lang 注册 shell native

- 新文件 `crates/auto-lang/src/vm/shell_module.rs`：`init_shell_module()` 注册 `system`/`system_status`/`export`/`exit`（模仿 `init_io_module` 的 native 注册模式）。
- native 实现：从栈取 String 参数，调 `vm.host`，压返回值。
- `autovm_persistent.rs::new()` 调 `init_shell_module()`。

### 改动 C：ash 实现 ShellHost

- `ash/auto-shell/src/host.rs`（新文件）：`struct ShellHostImpl`，持有通道或 `Arc<Mutex<HostState>>`。
- 因 Shell 是 `&mut self`（execute 需要），host 不能直接持有 `&mut Shell`（借用冲突）。**方案：请求队列**。
  - `ShellHostImpl` 持有 `channel: (Sender<HostRequest>, Receiver<HostReply>)` 或 `Arc<Mutex<VecDeque<(Request, oneshot::Reply)>>>`。
  - `system("ls")` 时：host 把请求入队，ash 在 `session.run()` 后/前 drain 队列执行。
- **简化方案（推荐）**：因 ash 脚本是单线程顺序执行（`session.run` 同步），用 `RefCell`/运行时借用：host 持有 `*mut Shell`（unsafe）或把 Shell 操作 defer 到 run 循环外。

> **关键设计决策**：native shim 在 VM 内同步执行，需要立即拿到 `system()` 返回值。但 Shell 被 `&mut` 借用在 dispatch 中。最干净：`ShellHostImpl` 持有 `Rc<RefCell<Shell>>`？不行（Shell 含非 Send 类型）。→ 用 **同步通道**：host 发请求 + 阻塞等回复；ash 的 run 循环在每次 native 调用后 yield 让 host 处理。这要求 VM run 循环可暂停——复杂。
>
> **务实方案**：system/exit/export 用一个"待执行队列"：native 把请求压入 `host.pending`，返回占位（system 暂时返回空，因为 VM 同步）。**这不够**——system 需要同步返回值。
>
> **最终方案**：ShellHostImpl 持有 `Arc<Mutex<Shell>>` 不现实（Shell 不可跨线程）。改为：**AutoVM run 时传入 host 引用**，ash 在调 `session.run_with_host(&mut shell, code)` 时把 shell 注入，native 直接调 shell。这需要改 `AutovmReplSession::run` 签名加 host 参数（或 session 持有 `&mut Shell` 的运行时引用）。

> 见 §7 风险——这是本 plan 最难的设计点，需在实施时原型验证。

### 改动 D：ash 注入 host + 脚本集成

- `Shell::new()` 后调 `session.set_host(...)` 或改 `execute_script_content` 用 `run_with_host(self, code)`。
- `execute_script_content` 的 `>` 前缀机制保留（向后兼容）；system() 是新通道。

### 改动 E：exit 语义

`exit(code)` 在 host 层：设置一个"请求退出"标志 + 退出码，ash 的脚本循环检查该标志，停止后续行执行 + 进程退出。不是 VM panic，是协作式停止。

## 4. 测试策略

| 层级 | 测试 | 位置 |
|---|---|---|
| 单元 | `system("echo hi")` 返回 `"hi"` | ash tests |
| 单元 | `system_status()` 返回 0（成功）/ 非 0（失败命令）| ash tests |
| 单元 | `export("FOO","bar")` 后 `system("echo $FOO")` 含 bar | ash tests |
| 单元 | `exit(42)` 停止脚本，进程退出码 42 | ash tests |
| 集成 | 端到端部署脚本：函数 + if + system + 错误处理 | ash tests（MS3 总验收）|
| 安全 | `system("rm file") --sandbox` 被拦（system 走 shell dispatch，策略生效）| ash tests |
| 回归 | host=None 时 AutoLang 纯用例不受影响（向后兼容）| auto-lang |
| 回归 | ash 全量测试 | ash |

### TDD 流程

1. RED：ash test `system("echo hi")` 返回 hi → 失败（system 不存在）
2. GREEN：host trait + AutoVM 插槽 + native + ash host 实现（先解决借用方案）
3. RED：export / exit 测试 → 失败
4. GREEN：实现
5. RED：端到端部署脚本 → 失败
6. GREEN：调试 + 验证
7. 全量回归

## 5. 实施步骤

1. **auto-lang host.rs**：定义 `ShellHost` trait。
2. **auto-lang engine.rs**：AutoVM 加 `host` 字段 + `set_host`。
3. **auto-lang shell_module.rs**：注册 system/system_status/export/exit native。
4. **原型验证借用方案**：确认 ash 如何在 native 同步执行时访问 Shell（§3 改动 C 的最终方案）。这是 go/no-go 关卡。
5. **ash host.rs**：实现 ShellHost。
6. **ash session 注入**：`Shell::new()` 或 `execute_script_content` 注入 host。
7. **exit 集成**：脚本循环检查 exit 标志。
8. **测试**：system/export/exit + 端到端脚本。
9. **安全验证**：system 受 --sandbox/--deny 约束。
10. **全量回归** + 提交 push。

## 6. 验收标准

- [ ] `fn main() { let out = system("echo hello"); print(out) }` 打印 hello
- [ ] `export("MY_VAR", "value")` 后 `system("echo $MY_VAR")`（Unix）/ 对应 env 读取 返回 value
- [ ] `exit(42)` 让 `ash script.ash` 进程退出码 = 42，脚本后续行不执行
- [ ] `system_status()` 反映命令成功/失败
- [ ] **端到端部署脚本**：含 fn + if + for + system + try/catch（配合 Plan 010），`ash deploy.ash` 端到端跑通
- [ ] system 受安全策略约束（`--sandbox`/`--deny` 下被拦）
- [ ] 无 host 时（纯 AutoLang）向后兼容，auto-lang + ash 全量测试通过

## 7. 风险

- **Shell 借用与 VM 同步执行冲突**（最大风险，可能阻塞 plan）：native 同步执行需要立即访问 Shell，但 Shell 在别处 `&mut` 借用。→ **步骤 4 是 go/no-go 关卡**：若无法干净同步，回退方案是 system 改异步（native 压请求 + run 循环 yield + 回填返回值），但复杂度高。原型先行。
- **exit 的协作式语义**：VM run 循环不天然支持"中途停"。exit 需要 host 标志 + run 循环检查。若 VM run 是深递归，检查点不够密可能延迟退出。→ run 循环每条 bytecode 检查 exit 标志（开销小）。
- **跨仓库 + 线程安全**：host 是 `Arc<Mutex<dyn ShellHost>>`，但 Shell 非 Send。host 实现内部不能持有 Shell 引用跨线程。→ host 实现用通道/队列，或限制 AutoVM 单线程（ash 脚本是单线程顺序，task/spawn 留空）。
- **system 的输出量大**：system 把整个 stdout 读入 String 压栈，大输出爆栈。→ 本期文档限制（system 用于小输出）；流式留后续。
- **环境变量跨平台**：`export` 在 Windows/Unix 行为差异（大小写、作用域）。→ 走 ash 现有 `ShellVars::set_env`（已跨平台处理）。
