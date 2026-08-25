# 066 — GUI 模式减法:撤除 smart command 表面层

- 日期:2026-08-25
- 状态:**已实施,验收通过**(plan-066 worktree 分支)
- 实施记录(2026-08-25 晚):
  - **撤除面**:worker `smart` 词法分支(list/run/NL 全部)+ SmartNlReq +
    spawn_smart_route_worker 路由线程及通道 + FakeNluClient;ShellHandle::
    run_smart + CommandReq::RunSmart + `/api/run_smart` 端点(http/backend
    桥);侧栏 SmartCommands 分区 + App.RunSmart + store.RunSmart +
    smart_commands 字段;BootSnapshot.smart_commands / SmartCommandEntry /
    SmartResult 契约(types.rs/api.at/types.at/手写 api.ts)。
  - **保留**:auto-shell crate smart_command/nlu 全量(CLI `ash smart` 不动
    —— 未来 AI-skill 形态的底件);worker 的 smart_block/smart_acc 输出槽
    (064 `script` 复用,注释已更新)。
  - **测试**:SM-01/02+zz_smart、TS-03/04、BACK-06、CMD-09..11 删除;
    BACK-07 stop 按钮改耐心搜索(单次快照竞态,CD-04 同款修法)。
  - **验收**:电池(ai_parity/chat_drawer/boot_script/tool_sidebar/backend/
    command_lifecycle)30 过 + backend 族 11 过;062 test_cli_parity 回归
    (he02/03 键盘 flake 在册豁免);vue-tsc 0 错 + vite build 绿(gen 重生成
    + restore-vue-assets,手写 api.ts smart 条目已清)。
  - **连带发现(非本计划改动所致)**:master codegen 对 `oninput` 发
    `$event.target.value` 实参与 0 参预置 handler 冲突(HistorySearch →
    TS2554)—— 066 之前已存在,gen 后手工回补一行保绿,已登记 DEBTS
    引擎清单第 6 条待引擎侧根治。
- 决策(用户裁定,2026-08-25):ash-gui 的"模式"层面只保留**普通命令模式**
  与 **AI 模式**(`?` 翻译 / `??` 对话)两种;smart command 从表面层撤除。
  未来若重启 smartcommand,定位是 **AI 模式内的轻量 skill 工具**(注册表
  + 本地路由作为 AI 的快路径/工具),不再是独立概念、独立入口。

## 0. 动机

- 063 T3 把 smart 做成了第三个 NL 入口(`smart <NL>` 词法 + 专用路由线程),
  与 `?`(LLM 翻译)、`??`(LLM 对话)表面同构,用户无从分辨 —— 概念上
  smart 实为"命令面板 + 本地路由"(闭集、零 LLM、确定性),不是 AI 模式,
  却以 AI 的姿态出现在提示符层。
- 实践中休眠:真实后端从未注册过 smart spec(BACK-06/TS-04/CMD-09..11
  四处 skip 同此理由),表面成本一直在付,本体没人用。
- CLI 侧 `ash smart` 子命令保留不动(auto-shell crate 的 smart_command/
  nlu 全部保留 —— 它是未来 "AI skill" 形态的底件)。

## 1. 范围(撤什么、留什么)

**撤(GUI 表面层):**
- worker `smart` 词法分支:`smart list | smart run <名> [args] | smart <NL>`
  全部拦截逻辑 + `SmartNlReq` + `spawn_smart_route_worker` 路由线程及其通道。
- 侧栏 SmartCommands 分区(tool_sidebar.at 的 prop/渲染)+ App.RunSmart
  handler + store.RunSmart handler 与 smart_commands 字段。
- `/api/run_smart` 端点(http.rs)+ backend.rs host_call 桥 +
  ShellHandle::run_smart + CommandReq::RunSmart。
- BootSnapshot 契约的 smart_commands 字段(types.rs/api.at/types.at);
  SmartCommandEntry/SmartResult 类型随撤(后端无消费方)。
- 测试:SM-01/02 + zz_smart fixture(test_ai_parity)、TS-03/04
  (test_tool_sidebar)、BACK-06(test_backend)。

**留(不动):**
- auto-shell crate:`smart_command::{config,loader,executor}`、`nlu`
  (未来 AI skill 的底件);CLI `ash smart` 全部行为。
- worker 内部的 `smart_block`/`smart_acc` 输出槽(064 `script` 命令复用,
  属内部管道,与表面无关;名字保留,注释说明)。

## 2. 验收

- 撤除后 `smart anything` 作为普通外部命令执行(不再被拦截);侧栏无
  SmartCommands 分区;BootSnapshot 无 smart_commands 字段。
- 回归:063 SN/ST 族 + CD 族 + 064 BS 族 + 062 test_cli_parity
  (he02/03 键盘 flake 在册豁免)+ vue-tsc/build 绿。

## 3. 明确不做

- **CLI 侧改动**:CLI 的 smart 是子命令不是"模式",不在本决定射程;
  未来 AI-skill 形态立项时两端一起定。
- **skill 化实现**:本计划只做减法;skill(注册表作为 AI 快路径/工具)
  是未来独立计划,方向已在 DEBTS/TODO 留档。
