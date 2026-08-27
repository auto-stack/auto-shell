# 074 — 运行中命令动态块(071 §6 E2):尾部实时预览 + 完成冻结

- 日期:2026-08-28
- 状态:**✅ COMPLETE(M1-M3 全落地,2026-08-28;v1 限制入 DEBTS)**
- 实施记录(2026-08-28):
  - **M1**:external.rs 新 `execute_external_tailed`(stdout+stderr 双读线程
    → 单通道到达序交错;语义对齐捕获分支:成功 Some(trimmed)/空 None/失败
    Err;直接 spawn 失败回退平台链);Shell 新 `external_tail_tx` 字段 +
    `set_external_tail`,外部分支(shell.rs:918)有通道走 tailed 版 ——
    policy/审计/exit-code/did-you-mean 全部零改动共用。
  - **M2**:tail_cmd.rs(eligibility 闸 + TAIL_HEIGHT=10/VIEW_LINES=7 +
    draw_tail_frame 状态行渲染,3 单测);TailLease 增 `into_parts()`
    (terminal 移交渲染线程/`freeze_viewport` 自由函数,RawGuard 转 pub,
    070 编辑器的 lease 方法委托不变);repl.rs execute_with_header 顶部加
    should_tail × try_execute_tail 闸 —— 所有调用点(主命令/F2 运行/
    072 审批执行)自动获得;渲染线程为视口唯一写者(REPL 线程阻塞在
    engine 内),80ms 帧循环 + 按键排空丢弃,退出前终排水 + freeze;
    ASH_NO_TAIL=1 逃生门。
  - **M3**:回归 —— ash-core 411(+3 tailed 真子进程测试:行流/返回值/
    非零退出)、ash lib 117(+3 eligibility)、auto-shell 701 过 2 挂
    (在册预存不变);`ash -c` 冒烟不受影响(E2 仅交互 REPL 路径)。
    测试辅助首版曾因 tx 原件存活排水死锁(生产代码顺序正确),已修。
- 来源:071 §6 方向储备 E2,地基 073(尾部租约)已就位。用户放行"开工"。
- 目标形态:单条外部命令(cargo build/ping/长脚本等)运行期间,输出不再
  "逐行打出去即固定",而是在尾部动态区**限高滚动预览**(状态行:命令·耗时·
  行数);运行完毕 **begin_freeze 清视口 → 冻结输出线性落转录** —— 冻结格式与
  现有 execute_with_header 完全一致(块头 + 全文),总字节数不变,只是从
  "渐进直打"变为"先预览后整体落档"。

## 0. 现状与插入点(2026-08-28 核验)

- REPL 单条外部命令:execute_single_command 的 registry-miss 尾部
  (shell.rs:918)→ `external::execute_external(input, dir, capture=false)`
  → `cmd.status()` **stdout/stderr 直继承**(边跑边打)。返回 Ok(None)。
- 拦截若在 REPL 层自 spawn 会绕过 policy/audit/$?/did-you-mean —— 不可取。
  **正确插入点:engine 的外部分支内部**换管道模式,其余逻辑(policy 检查/
  历史追踪/exit code/建议)零改动。

## 1. M1 — engine 侧(ash-core + auto-shell)

- `external.rs` 新 `execute_external_tailed(input, dir, tx: &mpsc::Sender<String>)`:
  parse_command → 直接 spawn(stdout+stderr 均 piped)→ 两个读线程逐行
  `tx.send`(到达序交错)→ wait → 语义对齐捕获分支(成功 Some(trimmed 合并)/
  空 None/失败 Err("Command failed: {stderr}")→ last_exit_code 提取链不变)。
  直接 spawn 失败 → 落回现有平台 fallback 链(PowerShell/sh,不预览)——
  与 execute_external 行为一致。
- `Shell` 新字段 `external_tail_tx: Option<Sender<String>>` +
  `set_external_tail()`;shell.rs:918 分支:有通道走 tailed 版。

## 2. M2 — frontend 侧(ash/frontend)

- `tail_cmd.rs` 新模块(纯逻辑可测):
  - `tail_eligible_line(input)`:无 `| ; & > < \` $ ( ) 换行` 等 shell 结构
    才 eligible(引号内含结构者保守回退);
  - 常量 TAIL_HEIGHT=10(视口)、VIEW_LINES=7(预览行数);
  - `draw_tail_frame`:状态行(`⏳ {cmd 前 3 词} · {elapsed}s · N 行`)+
    TailBuffer 尾部行渲染。
- `TailLease` 增 `into_parts()`(terminal 移交渲染线程,raw guard 留 REPL
  线程);`begin_freeze` 改可对裸 terminal 调用的自由函数(lease 方法保留
  委托,070 编辑器不动)。
- `Repl::execute_with_header` 顶部加闸:`should_tail`(eligible ×
  `classify_is_external_pub` × 非 interactive 名单)→ `try_execute_tail`:
  erase → acquire(失败则回退老路径)→ 渲染线程(唯一 viewport 写者,REPL
  线程阻塞在 engine 内)recv_timeout 循环推 TailBuffer + 画帧 + **排空并
  丢弃按键事件**(防 raw 模式下残留字节泄入下轮 read_line)→ engine 执行
  (set_external_tail(Some/None) 包夹)→ drop(tx) 关通道 → join 渲染线程
  (其在退出前完成最终排水 + begin_freeze)→ drop guard 回 cooked →
  按既有格式打印块头 + 冻结全文 → 返回 (snippet, exit_code) 契约不变。
  所有 execute_with_header 调用点(主命令/F2 编辑器运行/072 审批执行)
  自动获得该行为。

## 3. M3 — 回归与限制记录

- 单测:tail_eligible_line 边界;execute_external_tailed 真子进程
  (Windows `cmd /c echo` / Unix `echo`,cfg 分平台)行流 + 返回值 + 非零
  退出 Err。
- 冒烟:ash -c 不受影响(-c 无 lease);交互 REPL 手测留日常。
- **已知限制(v1,入 DEBTS)**:① 预览期间 Ctrl+C 不能中断命令(raw 模式
  关闭了控制台 Ctrl+C 处理;既有 DEBT"阻塞路径不可取消"的延伸),事件被
  排空丢弃,逃生门 `ASH_NO_TAIL=1` 环境变量整体关闭 E2 回老路径;② 冻结为
  全文(摘要/落文件是 E5);③ stderr 与 stdout 按到达序交错(与今天直继承
  观感一致)。

## 4. 明确不做

- 管道/重定向/复合命令的动态预览(结构复杂,首版只覆盖单命令主场景)。
- E3(结构化表格渐进渲染)/E4(AI 回合动态渲染)/E5(摘要冻结)。
- 取消/kill 通道(需要 engine 子进程句柄暴露,配合既有不可取消 DEBT 另议)。
