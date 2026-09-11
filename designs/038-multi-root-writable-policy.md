# 38 — 多路径可写白名单 + 项目/会话级策略关联 设计文档

> 状态:设计稿(Plan 081 交付物) · 2026-09-11
> 定位:ash 策略模型从"单根沙箱"演进为"多根可写白名单"的总设计,含与
> auto-ai PLAN-033 的跨仓执行层契约。落地由 Plan 081 承接;R5(OS 级强制)
> 本计划只做设计与可行性,实现留待后续计划。

## 1. 背景与动机

姊妹仓库 auto-ai 已于 2026-09-11 落地 PLAN-033「ash 优先命令执行层」
(`docs/plans/archive/033-ash-first-shell-execution.md`,status: archived):
auto-ai-cli 的 `run_command`/`run_ash_script` 经子进程调 ash,默认
`ash --sandbox <cwd>` 执行,策略拒绝不回退。

当前 ash 的沙箱是**单根**(`SecurityPolicy.sandbox_dir: Option<PathBuf>`,
Plan 009),无法表达 agent 场景的核心需求:

> 本项目这几个目录可写,其余只读;越界必须提醒用户并经审批。

单根模型下,agent 要么把整个用户目录设为沙箱(失去保护),要么把项目
目录设为沙箱(依赖项、全局工具配置全不可读)。需要:

1. **多根可写白名单**(R1):可写路径显式列举,写默认拒绝、读默认放开;
2. **策略文件入口**(R2):auto-ai 按"内置默认(cwd) < 项目配置 < 会话
   审批追加"合并后生成文件传给 ash,ash 只做执行层;
3. **结构化拒绝原因**(R3):agent 能做路径级补救,而非只看到"被拒";
4. **修复已知怪癖**(R4):脚本失败 exit 0 等三项,破坏 agent 对成败的判定;
5. **OS 级强制**(R5,P2):当前拦截是策略级,外部命令实际不受约束。

## 2. 现状(2026-09-11 实测,ash v0.1.0 debug 构建)

### 2.1 策略模型与拦截点

| 拦截点 | 位置 | 覆盖 |
|---|---|---|
| 危险模式(总是) | `ash-core/src/security.rs` `is_dangerous` | rm -rf /、mkfs、dd 写裸设备、fork bomb |
| allow/deny 名单 | `security.rs` `check()` | 命令名级(Plan 008) |
| no_exec / no_network / read_only | `security.rs` `check()` | 能力开关;read_only 是命令名级 |
| 路径级沙箱 | `ash/auto-shell/src/shell.rs:1567` `resolve_path(for_write)` + `cd`(shell.rs:1604) | canonicalize(含不存在路径的父目录回退 `canonicalize_or_parent`)+ `starts_with` 边界;符号链接逃逸经 canonicalize 检出 |
| `resolve_path` 生产调用点 | 内建 fs 命令(cat/cp/grep/head/ln/mv/rm/touch,`ash/auto-shell/src/cmd/commands/*.rs`)+ 输出重定向(shell.rs:956) | **内建与重定向受控;外部命令不经 `resolve_path`** |
| 外部命令 | `ash-core/src/cmd/external.rs` | Windows 委托 PowerShell;仅有名字级 allow/deny/no_network,OS 层行为不受约束(R5 动机) |

`SecurityPolicy` 字段(ash-core/src/security.rs:44):`allow` / `deny` /
`no_exec` / `no_network` / `read_only` / `dry_run` / `audit_file` /
`sandbox_dir: Option<PathBuf>`(单根)。空策略 = 完全直通(向后兼容,
`active()` 快路径)。

策略来源与优先级:`~/.config/ash` 配置 `[security]` 节
(`ash/auto-shell/src/config.rs:99` `SecurityConfig`,支持 config.at 与
ash.toml 双轨)为基,CLI flags 叠加(`ash/ash/src/main.rs:291`
`parse_security_flags`:布尔取 OR、名单去重并集、`--sandbox` 单值覆盖)。
**无策略文件入口**。

`ash agent` 信封(describe-tools / describe-policy / check / run):核心
库有 `PolicySummary`(`security.rs:341`,Plan 028,能力位摘要不泄漏路径)
与 `execute_for_agent` JSON 通道(shell.rs:1133,被 F4 tool-calling 复用),
但**二进制中不存在 `agent` 子命令**(main.rs 分发仅 plugin/ask/flags/脚本/
REPL)。原设计 `designs/028-agent-execution-engine.md` 已删除、委托给
auto-ai(designs/029 §错误纠正)。

### 2.2 已知怪癖(本设计实测复现,机制定位到行)

**① 拒绝时 stderr 双行、其一双前缀**:

```
$ ash --deny rm -c "rm foo.txt"
Error: security: 'rm' is denied by --deny          # ← execute() 打印(shell.rs:813)
Error: security: security: 'rm' is denied by --deny # ← execute_for_agent 再包一层
                                                    #   security: {reason}(shell.rs:1148),
                                                    #   main.rs:144 打印 "Error: {e}"
exit=1
```

auto-ai 的 `DENIED_MARKERS`(`crates/auto-ai-cli/src/shell_exec.rs:150`)
是 `contains` 子串匹配 `["Error: security:", "Error: sandbox:"]`,且其
fixture **把双前缀形态钉死在单测里**(classify 输入是静态字符串)。

**② 脚本失败 exit 0**(agent 无法可靠判定脚本成败):

```
$ printf 'no_such_fn_xyz()\n' > q.at && ash q.at
Error: Undefined function: no_such_fn_xyz
exit=0                                            # ← flush_auto_block(shell.rs:2959)
                                                  #   捕错只 eprintln、返回 Ok
$ printf '> definitely_not_a_cmd\n' > q.at && ash q.at
<PowerShell 报错文本>
Error: program not found
exit=0                                            # ← `> cmd` 错误同样吞掉(shell.rs:2764-2768)
```

main.rs 脚本路径(main.rs:249-253)只检查 `script_exit_requested()`,
不感知执行期错误。auto-ai 的 FailureClassifier fixture
`classify(Some(1), "Error: Undefined function: println") → PreExecFailure`
建模的是**应然**(exit 1),现状实然是 exit 0 → 被分类为 RanOk,契约落空。

**③ 脚本 exit(N≠0) 附无信息栈回溯**:

```
$ printf 'fn do_exit() {\n  exit(7)\n}\ndo_exit()\n' > q.at && ash q.at
Stack trace:
  #0 <anonymous> at line 0
exit=7
```

来源:auto-lang `crates/auto-lang/src/vm/engine.rs:2602`——"Task Error" 行
有 `!matches!(e, VMError::ExitRequested(_))` 守卫,**栈回溯块没有**该守卫,
ExitRequested 也打印;帧名 `<anonymous>`、行号 0 无诊断价值。顶层 exit
调用栈为空,不触发(与 ③ 原始观察"附一行"一致)。

**补充(同族,纳进 R4② 结论范围)**:沙箱拒绝的 stderr 文案为
`Error: sandbox: \\?\C:\... is outside sandbox \\?\C:\...`(exit 1,与
auto-ai fixture 的 `\\?\` 规整路径形态一致)——该行为正确,本设计要求
保持。

### 2.3 消费方契约(auto-ai 侧已上线,不可破坏)

auto-ai PLAN-033 `shell_exec.rs` FailureClassifier 四分类
`RanOk / Denied / PreExecFailure / RanFailed`,输入 = exit code + 全量
stderr:

| 分类 | 判据(2026-09-11 fixture 钉死) |
|---|---|
| RanOk | exit 0 |
| Denied | exit≠0 且 stderr 含 `Error: security:` 或 `Error: sandbox:`(contains 子串) |
| PreExecFailure | exit≠0 且含 `is not recognized` / `command not found` / `Undefined function:`(命令未开始执行,零副作用,可回退) |
| RanFailed | 其余(保守) |

两条并存时 **Denied 优先**。probe 机制要求 ash `--help` 含 `--sandbox`
字样。AshOptions 现状:`sandbox_root: Option<PathBuf>`(默认 cwd)→
`--sandbox <root>`。**ash 改错误文案/退出码属于破坏性变更**,迁移策略见 §6。

## 3. 与既有计划的关系

| 计划 | 关系 |
|---|---|
| 008(MS2-A 安全策略) | 本设计扩展其 `SecurityPolicy`;allow/deny/能力开关语义不动 |
| 009(MS2-B 路径沙箱) | **核心演进对象**:`resolve_path` 的 canonicalize+symlink 语义原样保留,判定从单根 `starts_with` 扩展为多根 `any(starts_with)`;`--sandbox` 单根语义保持兼容 |
| 028(Agent 执行引擎,已删除/委托) | R3 的 `denied_reasons` 字段按其信封 schema 设计,为未来 `ash agent` 子命令铺路;本计划不实现子命令 |
| 072(安全加固 P0) | S-6 修复的 `sandbox_only_policy_is_active` 回归必须保持;S-5 的 policy 透传模式(ask/plugin 受策略约束)沿用 |
| 037(Auto 单源化迁移) | 本设计改动落在手写 Rust(security.rs = L2 候选,shell.rs = L3 候选);语义先于迁移落地,后续单源化以 parity 网锁本设计语义 |
| auto-ai 033(ash 优先执行层) | 消费方。§2.3 契约不可破坏;目标对接形态见 §4.2 |

## 4. 跨仓契约

### 4.1 必须保持(auto-ai 已上线依赖,fixture 钉死)

1. **前缀子串**:策略拒绝 = exit≠0 且 stderr 含 `Error: security:` 或
   `Error: sandbox:`。文案只能**追加**(如 rule_id 后缀),不能改写前缀。
2. **预执行失败标记**:`is not recognized` / `command not found` /
   `Undefined function:` 保留在对应错误的 stderr 中。
3. **退出码传播**:`-c` 通道拒绝 exit 1(现状);R4② 修复后脚本通道
   失败 exit≠0——这是**修复契约落空**,与 fixture 的应然建模一致,非破坏。
4. **probe**:`--help` 输出保留 `--sandbox` 字样(新 flag 追加,不重排文案)。

### 4.2 目标对接形态(auto-ai 侧规划,ash 只做执行层)

- auto-ai `AshOptions.sandbox_root: Option<PathBuf>` → `writable_roots:
  Vec<PathBuf>`,组装 `--writable <path>`(可重复)或生成 `--policy-file`;
- **策略文件由 auto-ai 生成**:按"内置默认(cwd)< 项目配置 < 会话审批
  追加"三方合并后写出临时文件;ash 不内嵌 agent 侧 UI/审批/合并逻辑;
- ash 提供的仅是:多根写判定(R1)、文件加载(R2)、机器可解析拒绝
  (R3)、可靠退出码(R4)。

## 5. 设计

### 5.1 R1 — 策略模型:`writable_roots` 多根白名单

`SecurityPolicy` 演进(ash-core/src/security.rs):

```rust
pub struct SecurityPolicy {
    // ...既有字段不动...
    /// Plan 009 单根沙箱(全操作 confinement)。保留以兼容 --sandbox。
    pub sandbox_dir: Option<PathBuf>,
    /// 081/R1:可写白名单(可重复)。非空时写默认拒绝,仅列内路径放行;
    /// 读默认放开。canonicalize + symlink 逃逸检查沿用 Plan 009。
    pub writable_roots: Vec<PathBuf>,
}
```

**语义矩阵**(关键:`writable` 与 `sandbox` 是两个正交开关):

| 输入 | 读 | 写 | cd |
|---|---|---|---|
| 无标志(空策略) | 放开 | 放开(现状直通,**不变**) | 放开 |
| 仅 `--sandbox D` | 限 D | 限 D | 限 D(= 现状 Plan 009,兼容) |
| 仅 `--writable W…` | 放开 | **默认拒绝**,仅 W 内放行 | 放开 |
| 两者同时 | 限 D | 限 (D ∩ W),D 外直接拒 | 限 D |

判定实现:

- `resolve_path(for_write=true)`:`canonicalize_or_parent` 后,先查
  `sandbox_dir`(现状),再查 `writable_roots` 非空时
  `roots.iter().any(|r| canonical.starts_with(r_canon))`;每个 root 只
  canonicalize 一次(构造期或首次使用缓存,避免每命令重复 IO);
- `resolve_path(for_write=false)`:仅受 `sandbox_dir` 约束(读放开);
- `cd`:仅受 `sandbox_dir` 约束(现状);
- `active()` 增加 `!self.writable_roots.is_empty()`;
- `PolicySummary` 增加 `writable_roots_count: usize`(只暴露计数,不泄漏
  路径,沿用 028 能力位原则);
- **覆盖边界(如实声明)**:路径级判定只约束内建 fs 命令与输出重定向
  (§2.1);外部命令(git/cargo 等自行写盘)不受路径约束,仅有名字级
  拦截——OS 级强制是 R5(P2)。此边界写入 for-agents.md。
- 可选 `readable_roots` **不做**(YAGNI;读默认放开已覆盖目标场景,
  需要时由 sandbox 表达"读也 confinement")。

### 5.2 R2 — CLI 与策略文件

CLI(ash/ash/src/main.rs):

- `--writable <path>` 可重复,与 `--allow` 同式的预扫描解析;
- `--policy-file <file>` 加载策略文件;
- `--sandbox <dir>` 保留,语义不变(单根全 confinement;**等价于**
  "sandbox=dir" 而非 writable——兼容矩阵第二行);
- `--help` SECURITY 节追加两行(保留 `--sandbox` 字样,probe 契约);

配置(config.rs `SecurityConfig`)增加 `writable: Vec<String>`
(`[security] writable = "a,b"` CSV,与 allow/deny 同式)。

**策略文件**(新 `ash/auto-shell/src/policy_file.rs`,auto-shell 已有
serde_json 依赖;ash-core 保持 dependency-light 不引 JSON 解析):

```json
{
  "schema_version": "1",
  "sandbox": null,
  "writable": ["D:/proj/a", "D:/proj/b"],
  "read_only": false, "no_exec": false, "no_network": false, "dry_run": false,
  "allow": [], "deny": [],
  "audit": null
}
```

- `schema_version` 必填,非 `"1"` 拒绝启动(exit 2,usage 错误);
- 优先级:**config < policy-file < CLI flags**,合并规则沿用
  `parse_security_flags` 现状语义:布尔 OR(取更严)、名单并集、单值路径
  CLI 覆盖。policy-file 相对 config 是叠加而非替换;
- 文件不存在/解析失败:明确报错退出(不静默降级——安全配置静默失效是
  072 S-6 教训);
- auto-ai 负责三方合并后生成该文件;ash 不感知"项目/会话"概念。

### 5.3 R3 — 结构化拒绝原因

规则 ID 表(security.rs,`check()` 与 `resolve_path` 的错误统一携带):

| rule_id | 触发 | 现状文案(保持) |
|---|---|---|
| `deny-list` | 命令在 deny 名单 | `security: '<cmd>' is denied by --deny` |
| `allow-list` | 默认拒绝(不在 allow) | `security: '<cmd>' not in allow-list ...` |
| `no-exec` / `no-network` / `read-only` | 能力开关 | 现状文案 |
| `dangerous-pattern` | 危险模式 | 现状文案 |
| `sandbox-outside` | sandbox 单根越界 | `sandbox: <p> is outside sandbox <root>` |
| `sandbox-invalid` | sandbox 根不存在 | `sandbox: invalid --sandbox ...` |
| `writable-outside` | **新增**:写目标不在 writable_roots | `sandbox: write to <p> denied: not under any --writable root` |

机制:

- `SecurityPolicy::check` 的错误从裸字符串升级为结构体
  `DeniedReason { rule_id, path: Option<PathBuf>, message }`(miette
  Diagnostic 附着,`Display` 仍是现状文案前缀);
- **stderr 机器可解析行**(信封落地前的过渡契约):拒绝的 stderr 行追加
  机器段,格式

  ```
  Error: security: write to 'X' blocked by --read-only [rule=read-only path=X]
  ```

  方括号段可被 `\[rule=(\S+)(?: path=(.+))?\]` 提取;**前缀子串契约
  (§4.1)不动**;
- 未来 `ash agent run` 信封(Plan 028 形态)的 `denied_reasons:
  [{rule_id, path, message}]` 直接由 `DeniedReason` 序列化,本设计只保证
  结构体字段与信封 schema 对齐;
- `execute_for_agent` / `take_denial` 返回值同步携带 rule_id(agent 桥
  `eval_auto` 的 system() 可编程消费)。

### 5.4 R4 — 怪癖修复

**① 单行单前缀**(shell.rs:1148 与 813 二选一打印):

- `execute_for_agent` 在调 `execute()` 前置"抑制下一次拒绝打印"标志;
  execute() 的 `Err` 臂(shell.rs:812-813)见标志则跳过 eprintln,由
  main.rs 的 `Error: {e}` 打印唯一一行;
- `execute_for_agent` 不再二次包 `security:` 前缀(1148 改为
  `miette!("{reason}")`);
- 结果:`Error: security: 'rm' is denied by --deny` 一行,exit 1;
  contains 契约不受影响(auto-ai fixture 是静态输入,分类器测的是
  contains,修复后实然输出仍是其子串超集)。

**② 脚本失败退出码**(核心修复,"至少 ② 要有结论"):

- Shell 增加 `script_had_error: bool` 锁存:
  - `flush_auto_block`(shell.rs:2966)错臂:打印后置位
    `script_had_error = true`、`last_exit_code = 1`(文案
    `Error: Undefined function: ...` **保持不动**——auto-ai 预执行标记);
  - 脚本循环 `> cmd` 错臂(shell.rs:2767):同样置位;
  - **跑完不中断**(bash 式续跑语义保留,输出顺序不变);
- main.rs 脚本路径与 `-s` 路径:`!script_exit_requested() &&
  script_had_error` → `exit(1)`;显式 `exit(N)` 仍以 N 优先(现状);
- REPL 不受影响(交互式逐行,本就无进程退出码);
- 结论表述:**脚本进程退出码从此可靠**——0 当且仅当全程无错(或显式
  exit 0);auto-ai `run_ash_script` 的 FailureClassifier 因此真实生效。

**③ ExitRequested 不打栈回溯**(auto-lang 单文件改动):

- `crates/auto-lang/src/vm/engine.rs:2602` 栈回溯块补齐与上一行
  "Task Error" 相同的 `!matches!(e, VMError::ExitRequested(_))` 守卫;
- 顺带把帧信息从 `<anonymous> at line 0` 改为尽力取 `frame.fn_name` /
  实际行号(若数据本就在帧里;不做侵入式改造);
- **fallback**:work 阶段若跨仓(auto-lang)变更不可协调,则按仓库惯例
  落 DEBTS 条目(有意识接受 + 推翻条件),并在 081 复审记录注明。
  ③ 不阻塞 ②——exit code 本身(7/5)已正确传播,仅stderr 噪声。

### 5.5 R5 — OS 级强制(仅设计与可行性,P2)

现状边界(§2.1):外部命令不受路径约束;Windows 上 ash 将外部命令委托
PowerShell,OS 层完全不受控。策略级拦截的威胁模型是"防误操作"而非
"防绕过"。

| 平台 | 机制 | 可行性评估 |
|---|---|---|
| Windows | Job Object | **不能**表达 FS 路径白名单(Job 限制进程数/内存/UI/桌面);可作子进程生命周期绑定(超时 kill)的补充 |
| Windows | Restricted token + ACL | 可行方向:启动子进程前对白名单外路径 CalculateACL 拒绝?开销大;现实路径是 AppContainer(已用 chrome/edge 验证)或对 writable 根设置显式 ACL + 以受限令牌运行子进程。工程量大,需独立设计 |
| Unix | landlock(rust-landlock crate) | 最直接:规则集 = 可读(全)+ 可写(roots);ABI 已进主流内核; ash 子进程 fork 前应用即可,**推荐 P2 首选** |
| Unix | seccomp | 过滤 syscall 粒度,表达"路径"需与 namespace/lsync 结合,复杂度高,不推荐 |

P2 结论(写入本设计,实现留后续计划):优先 landlock(Unix 一体化),
Windows 走 AppContainer/restricted-token 专项设计;两平台都需解决
"PowerShell 中转"的特殊性(Windows 上先评估是否绕开 PowerShell 直接
CreateProcess)。**本计划零代码**。

## 6. 兼容与迁移

| 变更 | 破坏性 | 迁移策略 |
|---|---|---|
| stderr 双行→单行 | 否 | contains 子串契约保持;auto-ai fixture 为静态输入不受影响;live 输出仍是 `Error: security:` 超集 |
| 拒绝文案追加 `[rule=... path=...]` 段 | 否 | 前缀不动,追加式;fixture 逐字相等的断言若存在需复核(已核实 auto-ai fixture 均为 contains 判据,无逐字相等) |
| 脚本失败 exit 0 → exit 1 | **是(修复性)** | 属修复契约落空:fixture 建模应然(exit 1);对依赖"失败也 exit 0"的用户属可见变化,release note 声明;auto-ai 无需改动即获益 |
| `--sandbox` 语义 | 否 | 完全保持(兼容矩阵第二行);现有单测 `sandbox_only_policy_is_active` 等全部保持 |
| 新增 flag/字段 | 否 | 纯增量 |
| auto-lang 栈回溯 | 否 | stderr 噪声消除,exit code 不变 |

**版本协调**:不引入协议版本握手;靠 §4.1 的"前缀 + exit code"弱契约 +
auto-ai probe(要求 `--writable` 出现在 `--help` 后才启用新通道——建议
auto-ai 侧跟进项,不在本计划射程)。

## 7. 测试策略概览

1. **单元**(ash-core):`writable_roots` 判定矩阵(§5.1 四行全覆盖)、
   多根 any 匹配、symlink 逃逸(多根语义)、DeniedReason 结构与 Display;
2. **单元**(auto-shell):policy_file 解析(schema_version 校验/坏文件
   报错)、config `[security] writable` CSV、三层优先级合并;
3. **集成**(ash 二进制):CLI 冒烟——`--writable` 可重复、写放行/拒绝
   exit code、`--sandbox` 兼容回归、脚本失败 exit 1(怪癖②)、单行拒绝
   (怪癖①)、`exit(7)` in fn 无栈回溯(怪癖③)、`--help` 含
   `--sandbox`(probe 契约);
4. **跨仓复核**(不改 auto-ai):auto-ai `shell_exec.rs` 单测基于静态
   fixture,复核其在新 ash 实然输出下判据依然成立,结论记入 081 复审
   记录;
5. **全量回归**:`cargo test --workspace`(auto-shell 仓)0 失败 0 新
   警告;auto-lang 仓单独跑其 vm 测试(若 T-⑦ 落地)。

## 8. 风险登记

| 风险 | 缓解 |
|---|---|
| 路径规整形态差异(`\\?\` 前缀、大小写、盘符大小写)导致 starts_with 误判 | 判定统一用 canonicalize 后路径(现状已规整为 `\\?\` 形态);测试覆盖混合大小写 |
| writable 根含不存在路径(--writable D:/new-dir) | canonicalize_or_parent 同款父回退;启动期对 root 本身做一次解析并报错(明确失败优于静默) |
| 合并优先级理解歧义 | 三层优先级 + 合并规则在 --help 与 for-agents.md 文档化,单测钉死 |
| 外部命令绕过路径白名单造成"假安全感" | for-agents.md 明示覆盖边界;R5 落地前 agent 侧(no_exec 白名单)兜底 |
| 跨仓 ③ 阻塞 | §5.4 fallback(DEBTS),不阻塞主线 |

## 9. 开放问题

见 Plan 081 `10. 待澄清事项`(Q1 auto-ai 跟进时点、Q2 auto-lang 变更
协调方式、Q3 atom 格式策略文件的取舍时点)。
