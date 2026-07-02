# Plan 008: MS2-A — 安全策略框架（allow/deny + 能力开关 + 危险模式 + dry-run + 审计）

- **日期**: 2026-07-02
- **状态**: 待实施
- **RoadMap**: MS2（`docs/roadmap.md` §Milestone 2）
- **目标**: 给 ash 加一个集中的"命令执行前拦截层"，让 Agent 调用 `ash -c "..." --read-only` / `--no-exec` / `--deny rm` / `--dry-run` / `--audit log.jsonl` 时，命令在 spawn 之前被策略检查，可审计、可预演、可拒绝。

> **与 Plan 009 的关系**：本 plan 做"命令级"拦截（拦外部进程 spawn / 拦网络命令 / 拦危险模式 / 审计 / dry-run）。路径沙盒（拦文件系统写操作、限制在 `--sandbox <dir>` 内）是"文件级"拦截，改动面大，拆到 **Plan 009** 单独做。两者共享同一个 `SecurityPolicy` 结构。

## 1. 背景与现状

### 调研结论（2026-07-02 实读代码）

MS2 在 ash 里**完全是从零**（无任何 permission/sandbox/security/audit 模块）。但地基已具备：

| 地基 | 位置 | 复用方式 |
|------|------|---------|
| 外部命令集中入口 | `ash-core/src/cmd/external.rs` 4 个公开 spawn 函数 | 在入口处插策略检查 |
| shell 级 spawn | `shell.rs:569`（`execute_external`）+ `shell.rs:604`（`execute_external_with_redirect`，**自己直接 `Command::new`**，绕过 external.rs） | 两处都要插 |
| 命令注册表 dispatch | `registry.rs` → `shell.rs:495/531` `self.registry.get(cmd_name)` | 在 dispatch 前查能力开关（`--no-network` 拦 http_*）|
| 配置系统 | `config.rs` `AshShellConfig`，支持 `[section]`（config.at + ash.toml） | 加 `[security]` 段 |
| CLI 解析 | `main.rs` 手写 while 循环 | 加新 flag |
| 命令解析 | `external.rs:517` `parse_command(input) -> Vec<String>` | 提取 cmd_name 做匹配 |

### 关键风险点

1. **`execute_external_with_redirect`（shell.rs:604）绕过 external.rs**，直接 `std::process::Command::new(cmd_name)`（line 621）。只在 external.rs 插检查会漏掉带重定向的外部命令。→ 必须两处都插，或在 `execute_single_command`（shell.rs:472）派发前统一拦截。
2. **8 个 `Command::new(...)` 调用点**分散在 external.rs。不逐个改，而是在 **4 个公开入口函数**（`execute_external` / `spawn_external_stream*` / `spawn_external_chained` / `spawn_external_background`）顶部统一检查。
3. **能力开关分三类**，拦截点不同：
   - `--no-exec`：拦所有外部 spawn（external.rs 入口 + shell.rs:604）
   - `--no-network`：拦 http_* 注册命令（dispatch 处）+ 拦 curl/wget/ssh/nc 等外名（external.rs 入口）
   - `--read-only`：拦文件写——**但这块在 Plan 009 做**（需中央 path resolver）。本期 `--read-only` 只拦已知"纯写"外部命令（rm/mkdir 等作为外部进程时）+ 注册命令名黑名单，**注册命令的路径级写拦截留给 009**。

### 不在本期范围（YAGNI / 拆给 009）

- **路径沙盒**（`--sandbox <dir>`、符号链接穿透检测、文件写路径检查）→ Plan 009
- **中央 path resolver + fs.rs/touch.rs/ln.rs 重构** → Plan 009
- `--read-only` 对**注册命令内部** `std::fs` 写操作的拦截（需 path resolver）→ Plan 009 补全。本期 `--read-only` 仅做命令名级 + 外部进程级拦截。
- 配置文件的 allow/deny **通配符/正则**（本期只做精确命令名 + 简单前缀，正则留后续）

## 2. 设计

### 行为契约

| # | 规则 |
|---|------|
| 1 | 默认（无任何安全 flag）：行为与现状完全一致（零策略 = 全放行），向后兼容 |
| 2 | `--deny <cmd>`：命令名匹配（精确）→ 拒绝执行，stderr 给诊断，退出码非 0 |
| 3 | `--allow <cmd>`：白名单。设了任何 allow 后，**只有** allow 列表里的命令能跑（默认拒绝）|
| 4 | `--no-exec`：禁用所有外部命令执行（内置命令 + 注册命令仍可用）|
| 5 | `--no-network`：禁用 http_* 命令 + curl/wget/ssh/nc/ftp/telnet 等网络外部命令 |
| 6 | `--dry-run`：解析命令、打印"会做什么"，但**不执行写操作 / 不 spawn 外部进程**；只读命令（ls/cat/show）正常执行 |
| 7 | `--audit <file>`：每条执行的命令追加一行 JSON（命令 / 退出码 / 时间戳 / 是否被拒）到 file |
| 8 | 危险模式：`rm -rf /`、`rm -rf ~`、`> /dev/sda` 等命中预置模式 → 拒绝，无论是否设了其他 flag |
| 9 | 所有拒绝走 `Result<Err>`（不 panic）→ 非零退出码 + stderr 诊断 |
| 10 | 策略检查在 **spawn 之前**（外部命令）和 **dispatch 之前**（注册命令），绝不事后补救 |

### 安全决策顺序（pipeline）

每条命令进入 `execute_single_command`（shell.rs:472）时，在派发前依次过：

```
命令输入
  │
  ├─① 审计记录（always，若 --audit 开启）：记命令 + 开始时间
  │
  ├─② 危险模式检测（always，命中即拒，最高优先级）
  │
  ├─③ 命令名 allow/deny 检查
  │     · deny 命中 → 拒
  │     · allow 非空且不在 allow → 拒
  │
  ├─④ 能力开关检查
  │     · --no-exec 且是外部命令 → 拒
  │     · --no-network 且是网络命令（http_* 或网络外名）→ 拒
  │
  ├─⑤ dry-run 检查
  │     · --dry-run 且命令会写/spawn → 打印意图，跳过执行，返回 Ok
  │
  └─ 放行 → 正常 dispatch/spawn
```

### `SecurityPolicy` 结构

```rust
/// Plan 008: 集中安全策略，在命令 spawn/dispatch 前统一检查。
/// 存于 Shell 字段，由 CLI flag + config 灌入。零策略 = 全放行（向后兼容）。
#[derive(Debug, Clone, Default)]
pub struct SecurityPolicy {
    pub allow: Vec<String>,        // 命令名白名单（空 = 不启用白名单模式）
    pub deny: Vec<String>,         // 命令名黑名单
    pub no_exec: bool,             // 禁所有外部命令
    pub no_network: bool,          // 禁网络命令
    pub dry_run: bool,             // 预演模式
    pub audit_file: Option<PathBuf>, // 审计日志文件
    // Plan 009 会扩展：sandbox_dir, read_only（read_only 本期做命令名级，路径级留给 009）
}

impl SecurityPolicy {
    /// 是否启用任何安全限制（全 false/空 = 兼容模式，跳过所有检查）
    pub fn active(&self) -> bool { ... }

    /// 在命令 spawn/dispatch 前检查。返回 Ok(()) 放行，Err 拒绝。
    /// `is_external`: 命令是否将作为外部进程执行（决定 --no-exec 是否适用）
    pub fn check(&self, cmd_name: &str, args: &[String], is_external: bool) -> Result<()> { ... }
}
```

**决策依据**：policy 存在 `Shell` 字段（`shell.rs` `Shell` struct 加 `pub policy: SecurityPolicy`），因为：
- 四条 `Shell::new()` 路径（main.rs:50/73/125/REPL）都能从同一个地方灌入
- 注册命令 handler 拿到 `&mut Shell`，可直接读 `shell.policy`
- external.rs 在 ash-core（拿不到 Shell），通过新增参数 `policy: &SecurityPolicy` 传入（见改动 D）

### 配置文件集成（`[security]` 段）

`config.rs` `AshShellConfig` 加字段，`config.at` / `ash.toml` 双格式：
```ini
[security]
allow = ls,cat,show,grep      # 逗号分隔命令名
deny = rm,rmrf
no_exec = false
no_network = false
dry_run = false
# audit = ~/.config/ash/audit.jsonl
```
config.at 格式（auto_config）：`[security]` 段下 key=value，字符串列表用逗号分隔。

## 3. 实现架构

### 改动 A：新建 `security.rs` 模块

`ash/auto-shell/src/security/mod.rs`：
- `SecurityPolicy` 结构（上述）
- `check()` 方法（实现决策顺序 ②③④⑤）
- `DangerousPattern` 匹配器（预置危险模式列表，返回 bool）
- `audit_log()` 函数：追加一行 JSON 到 file（`{cmd, ts, decision, exit_code}`）
- 网络命令名表：`const NETWORK_COMMANDS: &[&str] = &["curl","wget","ssh","scp","nc","ftp","telnet","http_get",...];`

**危险模式预置列表（起步）**：
- `rm -rf /` / `rm -rf /*` / `rm -rf ~` / `rm -rf $HOME` / `rm -rf ~/*`
- `rm -rf .*`（家目录通配）
- `> /dev/sd*` / `dd if=... of=/dev/sd*`（写裸设备）
- `:(){ :|:& };:`（fork bomb）
- `mkfs`（格式化）
- 可配置扩展（config `[security]` dangerous_patterns）

### 改动 B：`Shell` 持有 policy + `execute_single_command` 前置检查

`shell.rs`：
1. `Shell` struct 加字段 `pub policy: SecurityPolicy`（Default = 全放行）。
2. `execute_single_command`（line 472）入口处，在派发前调 `self.policy.check(...)`：
   ```rust
   fn execute_single_command(&mut self, input: &str) -> Result<Option<String>> {
       let (clean_input, redirect) = parse_redirect(input);
       let mut parts = parse_args(&clean_input);
       let cmd_name = &parts[0];
       // Plan 008: 策略检查（派发前）
       if self.policy.active() {
           let args: Vec<String> = parts[1..].iter().cloned().collect();
           // 判断是否将走外部路径（不在 registry/builtin/auto 里）
           let is_external = !self.registry.get(cmd_name).is_some()
               && !builtin::is_builtin(cmd_name)
               && /* not auto fn */;
           self.policy.check(cmd_name, &args, is_external)?;
           // dry-run: 命令会写/spawn → 打印 + 跳过
           if self.policy.dry_run && self.command_writes_or_spawns(cmd_name, is_external) {
               eprintln!("[dry-run] would execute: {}", input);
               return Ok(None);
           }
       }
       // 审计：记录开始（若 --audit）
       // ... 原有派发逻辑 ...
       // 审计：记录结果 + 退出码（若 --audit）
   }
   ```
   > 注：`command_writes_or_spawns` 用一个"已知写命令"集合判断（rm/mv/cp/mkdir/touch/ln/build/run + 所有外部命令）。注册命令是否写，精确判断需 path resolver（009），本期用命令名近似。

### 改动 C：`main.rs` 解析新 CLI flag

main.rs while 循环加分支（模仿 `--json` 全局预扫描）：
```rust
"--no-exec" => { policy.no_exec = true; }
"--no-network" => { policy.no_network = true; }
"--dry-run" => { policy.dry_run = true; }
"--read-only" => { /* 本期标记，路径级拦截在 009 */ }
"--allow" | "--deny" | "--audit" => { /* 消费下一个参数 */ }
```
然后四个 `Shell::new()` 站点（line 50/73/125/REPL）后调 `shell.policy = policy;`（或 `shell.set_policy(policy)`）。

> **read-only 处理**：本期 `--read-only` 只在 `SecurityPolicy` 标记一个 `read_only: bool`，`check()` 里对已知写命令名（rm/mv/cp/mkdir/touch/ln）拒绝。**注册命令内部的 `std::fs` 写拦截（路径级）在 Plan 009** 经 path resolver 完成。

### 改动 D：external.rs 入口插策略检查

`ash-core/src/cmd/external.rs` 4 个公开入口函数加 `policy: &SecurityPolicy` 参数：
```rust
pub fn execute_external(
    input: &str, current_dir: &Path, capture_output: bool,
    policy: &SecurityPolicy,   // ← 新增
) -> Result<Option<String>> {
    let parts = parse_command(input);
    let cmd_name = &parts[0];
    let args = &parts[1..];
    policy.check_external(cmd_name, args)?;  // --no-exec / deny / 网络 / 危险模式
    // ... 原逻辑 ...
}
```
调用点（shell.rs:569 等）传入 `&self.policy`。
> `SecurityPolicy` 需要 ash-core 可见——放到 ash-core（`ash-core/src/security.rs`），ash/auto-shell re-export。这样 external.rs 和 shell.rs 都能用。

### 改动 E：配置集成

`config.rs`：
- `AshShellConfig` 加 `pub security: SecurityConfig`（子结构，含 allow/deny/no_exec/...）
- `from_auto_config` + `parse_toml` 各加 `[security]` 段读取
- `Shell::new()` 里 `self.policy = config.security.merge_cli(cli_policy)`（CLI 覆盖 config）

## 4. 测试策略

| 层级 | 测试 | 方式 |
|---|---|---|
| 单元 | `SecurityPolicy::check` 各分支（deny 命中/allow 不在/no-exec/网络/dry-run） | 纯函数测试，不 spawn |
| 单元 | `DangerousPattern` 命中 `rm -rf /` 等 | 模式匹配器测试 |
| 单元 | `audit_log` 写出合法 JSON line | tempdir + 读回解析 |
| 集成 | `execute_single_command` 带 deny policy → 拒绝 | Shell + policy |
| 集成 | dry-run 不 spawn 外部命令 | 标记 + 断言未调用 |
| CLI | `ash -c "rm -rf /" --deny rm` 退出码非 0，stderr 有诊断 | 子进程 |
| CLI | `ash -c "ls" --allow ls` 放行；`--allow cat` 时 ls 被拒 | 子进程 |
| CLI | `ash -c "curl ..." --no-network` 被拒 | 子进程 |
| CLI | `ash -c "echo hi" --audit log.jsonl` 生成审计行 | 子进程 + 读文件 |
| 回归 | 无安全 flag 时行为不变 | 全量 cargo test |

### TDD 流程

1. RED：`SecurityPolicy::check` 单测（deny/allow/no-exec/网络）→ 失败（结构不存在）
2. GREEN：建 `security.rs`，实现 check + 危险模式 + 审计
3. RED：`execute_single_command` 带 policy → 拒绝 → 失败（未接线）
4. GREEN：Shell 加 policy 字段 + execute_single_command 前置检查
5. RED：external.rs 入口 policy 参数 → 失败（签名变了）
6. GREEN：改 external.rs 4 入口 + 调用点
7. RED：CLI flag 测试（--deny/--no-exec/--audit）→ 失败
8. GREEN：main.rs 加 flag + config 集成
9. 回归 + 手动验证

## 5. 实施步骤

1. 建 `ash-core/src/security.rs`：`SecurityPolicy` + `check` + `DangerousPattern` + `audit_log` + 网络命令表。
2. TDD：`SecurityPolicy` 单元测试（决策顺序全分支）。
3. `Shell` struct 加 `policy` 字段；`execute_single_command` 前置 `policy.check` + dry-run + 审计。
4. external.rs 4 入口加 `policy` 参数；shell.rs 调用点传 `&self.policy`；`execute_external_with_redirect`（shell.rs:604）也插检查。
5. `config.rs` 加 `[security]` 段；`Shell::new()` 从 config 灌 policy。
6. `main.rs` 加 CLI flag（--allow/--deny/--no-exec/--no-network/--dry-run/--audit/--read-only）+ 四站点 set_policy。
7. 全量回归（cargo test）+ 手动验证验收标准。
8. 提交 + push。

## 6. 验收标准

- [ ] `ash -c "rm -rf /"` 被危险模式拦截（退出码非 0，stderr 有诊断）
- [ ] `ash -c "ls" --deny ls` 被拒绝；`--deny rm` 时 ls 放行
- [ ] `ash -c "ls" --allow ls` 放行；`--allow cat` 时 ls 被拒（默认拒绝）
- [ ] `ash -c "curl http://x" --no-network` 被拒绝
- [ ] `ash -c "grep foo file" --no-exec` 放行（grep 是内置）；`ash -c "git status" --no-exec` 被拒（git 是外部）
- [ ] `ash -c "touch f" --dry-run` 打印意图但不创建文件
- [ ] `ash -c "echo hi" --audit log.jsonl` 在 log.jsonl 写入合法 JSON 行
- [ ] `ash -c "touch f" --read-only` 被拒（命令名级拦截，本期）
- [ ] 无任何安全 flag 时，全量 cargo test 通过，行为零变化（向后兼容）
- [ ] config `[security]` 段生效（allow/deny 从配置读取）

## 7. 风险

- **external.rs 签名变更**：4 个公开函数加参数，所有调用点都要改。ash-core 是底层库，调用点多（shell.rs 多处 + 可能 ash-gui）。→ 需全量 grep 调用点，逐个改。考虑用 builder 或保留旧签名（无 policy 版调 `SecurityPolicy::default()`）减少波及。
- **`--read-only` 本期不完整**：只拦命令名，注册命令内部 `std::fs` 写不拦（如某命令 `run()` 里直接 `fs::write`）。→ 文档明确"完整 read-only 需 Plan 009"，验收标准只测命令名级。
- **dry-run 的"写判断"是近似的**：用命令名集合判断"是否写"，可能误判（如 `cat > file` 重定向写）。→ 重定向写（`apply_output_redirect` shell.rs:581）本期 dry-run 也拦（重定向目标文件视为写）。
- **审计日志并发**：多条命令快速执行时 append 可能交错。→ 用 `OpenOptions::append` + 单条 `writeln`（原子性足够，不跨进程并发）。
- **危险模式误报**：`rm -rf /tmp/old` 不应被 `rm -rf /` 模式误拦。→ 模式匹配要精确（`rm -rf /` 后跟结尾或空格+通配，而非前缀子串）。
