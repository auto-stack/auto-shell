---
plan_id: PLAN-081
status: reviewed
feature_name: ash 多路径可写白名单 + 项目/会话级策略关联
author: [agent]
created_at: 2026-09-11T00:00:00Z
updated_at: 2026-09-11T00:00:00Z
plan_revision: 1
current_step: 8
total_steps: 8
supersedes_spec_components: []
new_spec_components: []
touched_goals: []
worktree: D:/autostack/auto-shell/.worktrees/plan-081-dev
worktree_branch: plan-081-dev
worktree_base_commit: e930134
worktree_head_commit: 57eef86
dependency_commits:
  - repo: auto-lang, commit: d8971f4b1 (master, T-07 R4③ 单文件 scoped)
dependency_snapshots:
  - repo: auto-ai, ref: PLAN-033 (archived, 46d169a), 消费方契约 crates/auto-ai-cli/src/shell_exec.rs
  - repo: auto-lang, ref: master (junction 只读依赖), R4③ 单文件改动点 vm/engine.rs:2602
design_ref: designs/038-multi-root-writable-policy.md
---

# PLAN-081：ash 多路径可写白名单 + 项目/会话级策略关联

## 0. 变更摘要

ash 策略模型从"单根沙箱"(Plan 009 `sandbox_dir: Option<PathBuf>`)演进为
"多根可写白名单":新增 `writable_roots: Vec<PathBuf>`(写默认拒绝、白名单
放行、读默认放开),CLI 新增可重复 `--writable <path>` 与 `--policy-file
<file>`(JSON v1,由 auto-ai 按"内置默认(cwd) < 项目配置 < 会话审批追加"
合并后生成),拒绝原因结构化(`rule_id` + 机器可解析 stderr 段),并修复
三项已知怪癖(拒绝双行/双前缀、脚本失败 exit 0、exit(N≠0) 无信息栈回溯)。
R5(OS 级强制)仅设计与可行性,零实现。本计划不改 auto-ai——其
PLAN-033 已上线的 FailureClassifier 前缀/退出码契约(§4)是本计划的硬约束。

设计全文:`designs/038-multi-root-writable-policy.md`(本计划的契约基础)。

## 1. 目标

| 需求 | 内容 | 来源 |
|---|---|---|
| R1 | `sandbox_dir` 演进 + 新增 `writable_roots: Vec<PathBuf>`;写默认拒绝、仅白名单放行、读默认放开;canonicalize + symlink 逃逸检查沿用 Plan 009 扩展到多根 | 用户需求 §R1 |
| R2 | `--writable <path>` 可重复;`--policy-file <file>`(JSON v1);`--sandbox` 保持兼容(单根全 confinement) | §R2 |
| R3 | 结构化拒绝原因 `DeniedReason {rule_id, path, message}`;stderr 机器可解析段 `[rule=... path=...]`;对接 Plan 028 信封 schema(`ash agent` 子命令本身不做) | §R3 |
| R4 | 修复怪癖:① 拒绝 stderr 双行/双前缀;② 脚本失败 exit 0(**必须有结论**);③ exit(N≠0) 无信息栈回溯(auto-lang 单文件,fallback DEBTS) | §R4 |
| R5(P2) | OS 级强制(Windows Job Object/restricted token、Unix landlock/seccomp)只做设计与可行性,落 designs/038 §5.5 | §R5 |

**成功形态**:auto-ai 侧可把 `AshOptions.sandbox_root` 换成
`writable_roots: Vec` 并传 `--writable`/`--policy-file`,ash 的退出码 +
stderr 契约在其 FailureClassifier 下真实生效(脚本失败不再被分类为
RanOk)。

**非目标**:

- 不实现 `ash agent` 子命令与 JSON 信封(Plan 028 形态,后续计划);
- 不实现 OS 级强制(R5 仅设计);
- 不改 auto-ai 仓库(消费方;其 fixture 基于静态字符串,复核不改代码);
- 不做 `readable_roots`(YAGNI,"读也 confinement" 由 sandbox 表达);
- 不动 allow/deny/危险模式/no_exec/no_network 的既有语义。

## 2. 架构方案

- **ash-core**(纯逻辑):`SecurityPolicy` 增 `writable_roots` 字段与
  `DeniedReason` 结构;`resolve_path`/`cd` 判定扩展(实现在
  `ash/auto-shell/src/shell.rs`,策略定义在 `ash-core/src/security.rs`——
  维持 008/009 的"策略在 core、判定在使用方"分工);
- **auto-shell**(库):`policy_file.rs`(新)JSON v1 解析与三层优先级
  合并;`SecurityConfig.writable` CSV;脚本失败锁存
  `script_had_error`;
- **ash**(二进制):`--writable`/`--policy-file` 预扫描解析、`--help`
  SECURITY 节追加、脚本/`-s` 路径退出码传播;
- **auto-lang**(跨仓单文件):`vm/engine.rs:2602` 栈回溯块补
  ExitRequested 守卫(fallback:DEBTS 条目)。

## 3. 技术栈

Rust(edition 2021,既有 workspace:`ash/`= auto-shell + ash,ash-core
独立 package);serde_json(仅 auto-shell,已有依赖;ash-core 保持
dependency-light,策略文件解析不入 core);miette 诊断;无新第三方依赖。
测试:cargo test(单元)+ 二进制集成冒烟(复用 080 的
`_bench-*.at`/report/ 惯例放临时脚本,不入库)。

## 4. 需求分析与背景调查

### 4.1 授权记录

- 授权范围:本计划为**立项(drafting)**,/auto-plan:new 交接 work,
  不实施代码;work 阶段授权仓库 = auto-shell(本仓)+ auto-lang(仅
  T-07 单文件,须遵守其仓规约;若不可协调走 DEBTS fallback);
- 预算:用户未设定专项预算;自动延续遵循 auto-plan-work 默认;
- 修订绑定:本计划 plan_revision: 1 对应 designs/038 当前版本
  (2026-09-11)。

### 4.2 现状证据(2026-09-11 实测,ash v0.1.0 debug 构建)

- 策略模型:`ash-core/src/security.rs:44` `SecurityPolicy`(单根
  `sandbox_dir`、命令名级 allow/deny、no_exec/no_network/read_only/
  dry_run/audit_file;空策略直通);
- 判定点:`ash/auto-shell/src/shell.rs:1567` `resolve_path(for_write)`
  (canonicalize_or_parent + starts_with 单根)与 `:1604` `cd`;生产调用
  点 = 内建 fs 命令(cat/cp/grep/head/ln/mv/rm/touch,
  `ash/auto-shell/src/cmd/commands/*.rs`)+ 输出重定向(shell.rs:956);
  **外部命令不经路径判定**,Windows 委托 PowerShell;
- CLI:`ash/ash/src/main.rs:291` `parse_security_flags`(--allow/--deny/
  --audit/--sandbox 值式;--no-exec/--no-network/--dry-run/--read-only
  布尔式);config `[security]` 节(`ash/auto-shell/src/config.rs:99`,
  config.at + ash.toml 双轨);无 `--writable`/`--policy-file`,二进制无
  `agent` 子命令(main.rs 分发仅 plugin/ask);
- 怪癖复现(designs/038 §2.2,机制定位):
  - ① 双行/双前缀:`shell.rs:813` execute() 打印一次 + `shell.rs:1148`
    execute_for_agent 再包 `security: {reason}` + `main.rs:144` 打印
    `Error: {e}` → 实测两行,exit 1;
  - ② 脚本失败 exit 0:`shell.rs:2966` flush_auto_block 捕错仅 eprintln
    返回 Ok;`shell.rs:2767` `> cmd` 错臂同样吞掉;main.rs:249-253 只查
    `script_exit_requested()`。实测:未定义函数与外部命令失败均 exit 0;
    **auto-ai fixture `classify(Some(1), "Error: Undefined function: ...")
    → PreExecFailure` 建模的是应然,现状被分类 RanOk,契约落空**;
  - ③ 栈回溯:auto-lang `crates/auto-lang/src/vm/engine.rs:2602`——
    "Task Error" 行有 ExitRequested 守卫,栈回溯块没有;实测
    `exit(7)` in fn 输出 `Stack trace:\n  #0 <anonymous> at line 0`,
    exit code 7 正确;
- 消费方契约:auto-ai `crates/auto-ai-cli/src/shell_exec.rs:150`
  `DENIED_MARKERS = ["Error: security:", "Error: sandbox:"]`(contains
  子串)、预执行标记 `is not recognized`/`command not found`/
  `Undefined function:`、Denied 优先、probe 要求 `--help` 含
  `--sandbox`;fixture 均为静态输入的 contains 判据(逐条核实
  shell_exec.rs:463-634,无逐字相等断言);
- 依赖事实:auto-shell 已依赖 serde_json(config.rs 同 crate);
  ash-core 刻意不引 JSON 解析(audit 手写 JSON,注释明示)。

### 4.3 既有计划关系

008(策略框架,语义不动)/ 009(路径沙箱,本计划核心演进对象)/
028(信封 schema 对齐目标,已删除委托 auto-ai)/ 072(S-6 sandbox-active
回归必须保持)/ 037(本计划语义是 L2/L3 单源化迁移的锁定对象)/
auto-ai 033(消费方,archived)。详见 designs/038 §3。

## 5. 详细设计

核心设计(dyn 语义矩阵、策略文件格式、rule_id 表、怪癖修法、R5 可行性)
见 `designs/038-multi-root-writable-policy.md` §5,此处只列实现要点:

- **语义矩阵**(§5.1):空策略直通不变;仅 sandbox = 全 confinement
  (现状);仅 writable = 写白名单/读放开;并存 = 交集(writable 根在
  sandbox 外等于不可写,如实文档化);
- **root canonicalize 缓存**:writable 根在策略构造/首次使用时解析一次
  并缓存,不逐命令重复 IO;根不存在启动期报错(exit 2,明确失败优于
  静默);
- **policy file JSON v1**(§5.2):`schema_version` 必填非 "1" 拒绝;
  优先级 config < policy-file < CLI;布尔 OR、名单并集、单值路径 CLI
  覆盖;坏文件报错不降级(072 S-6 教训);
- **DeniedReason**(§5.3):rule_id 表 deny-list/allow-list/no-exec/
  no-network/read-only/dangerous-pattern/sandbox-outside/sandbox-invalid/
  writable-outside(新增);stderr 追加段
  `[rule=<id> path=<p>]`(可正则提取);前缀与文案主体保持;
- **单行原则**(§5.4①):execute_for_agent 前置抑制标志,execute()
  错臂跳过 eprintln,机器面(main.rs -c)唯一一行;
- **脚本退出码**(§5.4②):`script_had_error` 锁存(flush_auto_block +
  `> cmd` 错臂置位),跑完不中断;main.rs 脚本/-s 路径
  `!script_exit_requested() && script_had_error → exit(1)`;显式 exit(N)
  优先;
- **栈回溯守卫**(§5.4③):engine.rs:2602 条件补
  `&& !matches!(e, VMError::ExitRequested(_))`;帧名/行号 best-effort
  (数据已在帧里才做,不侵入)。

### 规范增量

> 注:本仓无 `docs/specs/` 体系(080 收据在案),规范载体即
> `designs/038` + `docs/for-agents.md`;frontmatter 三字段留空,
> 下表 target 列指实际载体。

| delta_id | 类型 | target | before/after | rationale | acceptance IDs |
|---|---|---|---|---|---|
| SD-01 | add | designs/038 §5.1 | 无 → writable_roots 多根写白名单语义矩阵(四行) | agent 场景"项目目录可写、其余只读"不可表达 | AC-01, AC-02, AC-03 |
| SD-02 | add | designs/038 §5.2 + docs/for-agents.md | 无 → `--writable`(可重复)/`--policy-file`(JSON v1)/config `[security].writable` | 策略由 auto-ai 生成,ash 只执行 | AC-04, AC-05 |
| SD-03 | modify | designs/038 §5.3 + docs/for-agents.md | 拒绝仅人类文案 → 追加 `[rule=... path=...]` 机器段 + DeniedReason 结构 | agent 路径级补救;对齐 028 信封 schema | AC-06 |
| SD-04 | modify | designs/038 §2.2/§5.4 | 拒绝双行/脚本失败 exit 0/无信息栈回溯 → 单行单前缀/失败 exit 1/无栈回溯噪声 | agent 依赖退出码与 stderr 判定成败(auto-ai 契约) | AC-07, AC-08, AC-09 |
| SD-05 | add | designs/038 §5.5 | 无 → OS 级强制可行性结论(landlock 首选;Windows 需专项) | 声明策略级拦截的威胁模型边界 | AC-10 |

## 6. 测试设计

1. **单元(ash-core,`cargo test -p ash-core`)**:writable 判定矩阵
   四行、多根 any 匹配、symlink 逃逸(白名单根经符号链接指外)、
   root 不存在报错、DeniedReason Display 保持前缀、`active()`//
   `summarize()` 增量(计数不泄漏路径);
2. **单元(auto-shell,`cargo test --workspace` in `ash/`)**:
   policy_file 解析(schema_version 校验、坏 JSON/缺字段报错)、config
   CSV、config < file < CLI 三层合并(布尔 OR/名单并集/单值覆盖)、
   `script_had_error` 锁存与 `--sandbox` 兼容回归(072 S-6 用例保持);
3. **集成冒烟(ash 二进制,临时脚本不入库)**:
   - `ash --writable A --writable B -c "> A/f.txt"` 成功;`> C/out.txt`
     拒绝 exit 1 且 stderr 含 `[rule=writable-outside`;
   - `ash --deny rm -c "rm x"` 恰一行 `Error: security:` 开头(怪癖①);
   - 脚本含未定义函数 → exit 1,stderr 含 `Undefined function:`
     (怪癖②,auto-ai 判据成立);
   - `exit(7)` in fn → exit 7 且 stderr 无 `Stack trace:`(怪癖③);
   - `--help` 含 `--sandbox`(probe 契约);`--sandbox` 单独使用行为与
     现状一致(矩阵第二行);
4. **跨仓复核(不改 auto-ai)**:以新 ash 实然输出逐条喂
   auto-ai `classify()` 判据做人工对照,结论(应为:全部判据成立,
   Denied 子串保持、预执行标记保持、脚本失败 exit 1 落入
   PreExecFailure/RanFailed 建模)记入 §9 复审记录;
5. **全量回归**:auto-shell 仓 `cargo test --workspace` + ash-core
   `cargo test -p ash-core` 0 失败 0 新警告;若 T-07 落地,auto-lang 仓
   `cargo test -p auto-lang --lib vm` 单独跑。

## 7. 验收标准

| ID | 验收项(可观察行为) | 验证方法与预期 | 需求 |
|---|---|---|---|
| AC-01 | 多根写白名单:`--writable A --writable B` 下,写 A/B 内成功,写外拒绝 exit 1 | 集成冒烟 §6.3-1;单元矩阵 §6.1;拒绝 stderr 含 `writable-outside` 语义 | R1 |
| AC-02 | 空策略直通不变:无安全标志时读写行为与现状一致 | 既有测试 0 改动全绿;矩阵第一行单元用例 | R1 |
| AC-03 | `--sandbox` 完全兼容:单根全 confinement 语义、canonicalize/symlink 检查、072 S-6 回归不破 | 既有 sandbox 用例全绿 + 矩阵第二/四行单元用例 | R1 |
| AC-04 | `--writable` 可重复且入 `--help`;config `[security].writable` CSV 生效 | 冒烟:两根同时生效;help 文本含 `--writable` | R2 |
| AC-05 | `--policy-file` JSON v1:合法文件生效,schema_version 非 "1"/坏文件报错 exit≠0;优先级 config < file < CLI | 单元 §6.2 + 冒烟:file 设 deny、CLI 追加,断言合并结果 | R2 |
| AC-06 | 拒绝 stderr 机器可解析:每个拒绝行含 `[rule=<id>` 段,且 `Error: security:`/`Error: sandbox:` 前缀保持 | 单元断言 Display;冒烟 grep `\[rule=`;auto-ai 判据对照 §6.4 | R3 |
| AC-07 | 拒绝输出单行单前缀:`--deny rm -c` 恰一行 stderr | 冒烟 §6.3:stderr 行数 = 1 且前缀唯一 | R4① |
| AC-08 | 脚本失败 exit≠0:未定义函数/外部命令失败 → exit 1,stderr 标记文案保持;显式 exit(N) 优先;REPL 不受影响 | 冒烟 §6.3 + 单元锁存;对照 auto-ai PreExecFailure 标记 | R4② |
| AC-09 | `exit(N≠0)` 无栈回溯噪声:in-fn exit 不再输出 `Stack trace:`,退出码保持 | 冒烟 §6.3;若 T-07 落地含 auto-lang 单测,否则 DEBTS 条目可查 | R4③ |
| AC-10 | R5 仅设计:designs/038 §5.5 含两平台机制对比与结论(landlock 首选),仓库无 OS 级强制的实现 diff | 文档评审;`git diff` 核对无相关代码 | R5 |
| AC-11 | 全量回归绿:两仓 cargo test 0 失败 0 新警告;auto-ai 判据复核结论在案 | §6.5 命令输出;§9 复审记录引用 | 全局 |

## 8. 执行步骤

> 均在 worktree `.worktrees/plan-081-dev`(分支 plan-081-dev,base e930134)执行;
> T-07 在 auto-lang 仓主检出 scoped 提交。

**T-01 [x] R4① 拒绝单行单前缀** — commit `0f05302`
- 实现:execute() 三个拒绝臂(单命令/管道阶段/后台)加 `suppress_denial_print`
  守卫;execute_for_agent 置位抑制交互打印且不再二次包 `security:` 前缀
- 证据:单测 2 项(denial_error_carries_exactly_one_security_prefix /
  denial_suppress_flag_resets_after_agent_call)绿;活体冒烟恰一行
  `Error: security: 'rm' is denied by --deny`,exit 1 → AC-07

**T-02 [x] R4② 脚本失败退出码** — commit `fb3abb7`
- 实现:`script_had_error` 锁存(flush_auto_block / `> cmd` 错臂 /
  capture 赋值失败三处置位,跑完不中断);main.rs 脚本与 -s 路径无显式
  exit 时按锁存 exit 1
- 证据:单测 3 项绿;活体冒烟:未定义函数 exit 1(原 0,文案保持)/
  缺失命令 exit 1/干净脚本 exit 0/`exit(7)` 仍优先 → AC-08

**T-03 [x] R1 writable_roots 策略模型** — commit `1bba099`
- 实现:SecurityPolicy.writable_roots + active()/summarize()
  (writable_roots_count 只暴露计数);Shell canon 缓存
  (canon_writable_roots,set_policy/new 共用);resolve_path 多根写判定
  (sandbox 后 any(starts_with),读放开,cd 不变);config
  `[security].writable` CSV 双轨接 to_policy
- 证据:ash-core 单测 2 + auto-shell 3(含正交交集矩阵)绿 → AC-01,
  AC-02, AC-03

**T-04 [x] R2a CLI --writable + help + 根校验** — commit `152c2eb`
- 实现:--writable 可重复(去重并集);--help SECURITY 节追加(保留
  `--sandbox` 字样);validate_writable_roots 启动校验(不存在/非目录
  exit 2)
- 证据:冒烟双根写放行 exit 0/越界拒绝 exit 1/坏根 exit 2/help 含
  `--sandbox`+`--writable` → AC-04

**T-05 [x] R2b --policy-file JSON v1** — commit `5b470da`
- 实现:新模块 auto-shell/policy_file.rs(load + merge_over;
  schema_version≠"1"/坏 JSON/缺文件硬错;布尔 OR/名单并集/单值路径覆盖);
  parse_security_flags 重构三层装配 config < policy-file < CLI;
  serde derive 入 auto-shell 清单(树内已有,非新三方)
- 证据:单测 4 绿;冒烟文件策略写放行/deny/no_network 生效 + CLI 并集 +
  坏文件 exit 2 → AC-05

**T-06 [x] R3 结构化拒绝原因** — commit `1ff2b56`
- 实现:ash-core DeniedReason{rule_id,path,message}(Display 追加
  `[rule=<id>[ path=<p>]]`,文案主体逐字保持);check() 六类 + resolve_path
  /cd 四类路径规则全部结构化;last_denial/take_denial 自动携带
- 证据:单测更新/新增绿;活体冒烟 deny-list/writable-outside/
  sandbox-outside 三类段格式正确、前缀保持 → AC-06

**T-07 [x] R4③ auto-lang 栈回溯守卫** — auto-lang commit `d8971f4b1`(master)
- 实现:engine.rs 栈回溯块补 `!matches!(e, VMError::ExitRequested(_))`
  守卫(与上方 Task Error 行 Plan 011 守卫对齐);Q2 决策=主检出 scoped
  小改,fallback 未触发
- 证据:auto-lang vm:: 469 测试 0 失败;活体冒烟 `exit(7)`/嵌套 `exit(5)`
  无 Stack trace 行且退出码保持,真实错误仍报 Error → AC-09
- ⚠ 事故记录:首次提交误并入并发会话(Plan 394)在 engine.rs 的未提交
  WIP(112 insertions);已拆分修复——soft reset 后在纯净版重放守卫单独
  提交(+9/-2),Plan 394 WIP 完整还原为其未提交态并验证编译通过。
  关联:见 §9。

**T-08 [x] 文档 + 跨仓复核 + 全量回归** — commit `57eef86`
- 实现:for-agents.md 语义矩阵/三层优先级/契约(rule_id 全集+退出码+单行
  原则)/覆盖边界
- 证据:auto-ai shell_exec 判据 17/17(只读);ash-core 415/0;workspace
  940/0(排除两个基线预存失败:shell::tests::test_auto_expression_execution
  与 frontend::tail_cmd::tests::spill_writes_readable_unique_files,均在
  主检出 e930134 复现,与 081 无关——前者 auto-lang junction 漂移,后者
  毫秒唯一名时序 flake)→ AC-10, AC-11

## 9. 复审记录

- 2026-09-11 drafting 交接(stage: new,PLAN-081 rev:1):
  调研与设计已完成(designs/038 + 本计划),三条怪癖当日实测复现并
  定位机制行级;跨仓契约(§4)经 auto-ai `shell_exec.rs` 逐条核实。
  outcome: **pass** — 可交接 work;R1-R5 全覆盖映射见 §1/§7,
  唯二开放项 Q2(auto-lang 协调方式)与 Q3(policy 文件 atom 形态)
  已带默认决策,不阻塞。next: **work**。
  变更任务/验收集:T-01~T-08,AC-01~AC-11(初次建立)。

- 2026-09-11 work 完成(stage: work,rev:1,outcome: **pass** →
  status: execution_done,next: **review**):
  - code_commit: worktree plan-081-dev @ `57eef86`(T-01 0f05302 / T-02
    fb3abb7 / T-03 1bba099 / T-04 152c2eb / T-05 5b470da / T-06 1ff2b56 /
    T-08 57eef86);依赖仓 auto-lang @ `d8971f4b1`(T-07)。
  - task_ids: T-01~T-08 全部 [x];AC-01~AC-11 全部有落点(§8 证据列)。
  - evidence:
    - 单测:ash-core 415/0;auto-shell lib 722 中除 1 基线预存外全绿
      (081 新增/更新 18 项全绿);auto-lang vm:: 469/0;auto-ai
      shell_exec 判据 17/17(只读复核,零代码改动)。
    - 全量:ash workspace 940/0(排除两个基线预存失败:
      test_auto_expression_execution = auto-lang junction 漂移
      `<obj#...>`,spill_writes_readable_unique_files = 毫秒唯一名时序
      flake;两者在主检出 e930134 均复现,建议后续 DEBTS/独立小修)。
    - 活体冒烟:v0.1.0 debug 二进制逐条验证语义矩阵四行/双根并集/
      坏根 exit 2/policy-file 坏文件 exit 2/三类拒绝 [rule=...] 段/
      单行单前缀/脚本失败 exit 1/exit(7) 无栈回溯,输出见各任务证据。
    - AC-10(R5 仅设计):designs/038 §5.5 已含两平台对比与结论;
      实现期零偏差,无需回写。
  - 事故记录(T-07 所有权):首次在 auto-lang 提交时误并入并发会话
    (Plan 394)在 engine.rs 的未提交 WIP;当场发现(112 insertions 与
    3 行守卫不符),soft reset 后纯净版重放单独提交(+9/-2 =
    d8971f4b1),Plan 394 WIP 完整还原未提交态且 auto-lang 编译恢复。
    教训:auto-lang master 存在活跃并发会话,跨仓 scoped 提交前必须
    先核对目标文件的既有 diff(`git diff --stat -- <file>`),而不是
    只看 status 头几行。
  - blockers: 无。
  - next: review(auto-plan-review;worktree 保留供复审与 merge;
    auto-lang d8971f4b1 的归属合并路径由 review/merge 阶段确认)。

- 2026-09-11 review(stage: review,PLAN-081 rev:1,outcome: **pass** →
  status: reviewed,next: **merge**):
  - 独立性声明:复审在实施会话内进行,结论由工件独立重建——全部验证
    命令在本复审中重跑,未采信实施期摘要;过程中两处复审脚本缺陷
    (AC-01 未显式 cd 导致写目标错位;AC-05 heredoc 展开把 `C:\` 反斜杠
    写入 JSON 触发非法转义)被当场识别并修正,均为脚本侧问题,产品行为
    每次均符合设计。
  - reviewed_commit: `57eef86`(worktree plan-081-dev,working tree
    clean,7 commits)/ base_commit: `e930134` / dependency_revisions:
    auto-lang `d8971f4b1`(master)、auto-ai(只读复核,未改)。
  - diff 范围核对:e930134..57eef86 恰为 9 个预期文件(security.rs /
    shell.rs / config.rs / policy_file.rs / lib.rs / main.rs /
    auto-shell Cargo.toml+Cargo.lock / for-agents.md),零射程外改动;
    OS 级强制关键词(landlock/seccomp/JobObject/AppContainer)在 diff 中
    零命中 → AC-10。
  - spec_inputs:无 docs/specs 体系(080 先例在案),载体 =
    designs/038(§5.5 R5 可行性在案)+ docs/for-agents.md;rule_id 文档
    清单与代码 9/9 一致;frontmatter 三 spec 字段留空已附书面说明。
  - acceptance_results(全部独立复现):
    - AC-01 pass:双根写放行 exit 0 ×2/越界拒绝 exit 1/
      `[rule=writable-outside]`+`Error: sandbox:` 前缀保持;
    - AC-02 pass:无策略读写直通 exit 0;AC-03 pass:`--sandbox` 单根
      内写 exit 0/越界 exit 1(兼容矩阵第二行);
    - AC-04 pass:坏根 exit 2/help 含 `--sandbox`(probe)+`--writable`;
    - AC-05 pass:文件策略写放行/越界拒绝/对方言 deny
      (`[rule=deny-list]`)/CLI 叠加/坏 schema 与缺文件 exit 2;
    - AC-06 pass:sandbox-outside/writable-outside/no-network 三类
      `[rule=... path=...]` 段格式正确、人类文案前缀逐字保持;
    - AC-07 pass:拒绝 stderr 恰 1 行(wc -l=1)且无 `security: security:`;
    - AC-08 pass:未定义函数 exit 1+`Undefined function:` 保持/缺失命令
      exit 1/干净脚本 exit 0/`exit(7)` 优先;
    - AC-09 pass:`exit(7)` in fn 退出码 7 且 stderr 为空(无 Stack trace);
    - AC-10 pass:见上;AC-11 pass:见 evidence。
  - evidence(回归,本复审重跑):ash-core 417/0;auto-shell lib 722/0
    (排除 1 基线);ash workspace 全量已由 T-08 覆盖 940/0(排除 2 基线);
    基线两失败(test_auto_expression_execution /
    spill_writes_readable_unique_files)在未修改主检出再次复现,确证
    预存;auto-lang vm:: 469/0;auto-ai shell_exec 17/17。
  - findings:无阻塞项。非阻塞观察:N-1 基线两失败修复归属入 §10 Q5
    (射程外);N-2 T-07 所有权事故已于 work 期修复在案,merge 阶段吸收
    auto-lang `d8971f4b1` 时按其仓惯例确认;N-3 策略文件路径须 OS 原生形
    (MSYS 不转换文件内容)已在 for-agents.md 明示,auto-ai 接入时注意。
  - next: merge(auto-plan-merge;worktree `.worktrees/plan-081-dev` 与
    分支 plan-081-dev 由 merge 流程守卫与清理)。

## 10. 待澄清事项

| # | 事项 | 当前默认决策(不阻塞 work) | owner/next |
|---|---|---|---|
| Q1 | auto-ai 侧跟进(writable_roots 通道 + probe 升级)的时点与本计划的先后 | 本计划只保证契约可用;auto-ai 跟进由其仓另立计划 | auto-ai 侧维护者 |
| Q2 | ~~T-07 的 auto-lang 变更走主检出小改还是 DEBTS fallback~~ | **已决**:主检出 scoped 小改落地(auto-lang `d8971f4b1`),fallback 未触发;遗留:该提交并入 auto-lang master 的常规流由其仓惯例吸收(未见阻塞性约束) | 已闭环 |
| Q3 | 策略文件是否需要 atom(Auto/Atom)形态 | v1 仅 JSON(auto-ai 生成方便 + auto-shell 已有 serde_json);atom 形态留待 ash 配置体系统一(config.at)时再评估 | 后续计划 |
| Q4 | writable 根在 sandbox 外时的行为(交集为空集) | 按矩阵:直接拒绝写并给 `writable-outside`;文档明示,不做特殊报错 | 已定(实现与冒烟一致) |
| Q5 | 基线预存失败两项(test_auto_expression_execution / spill_writes flake)的修复归属 | 本计划不修(射程外,基线在案);建议各自立独立小修或入 DEBTS | 仓维护者 |
