# 061 — 外部后端配置:ash-server Auto 化 + 启动形式自由(HTTP/merged)

- 日期:2026-08-23
- 状态:**待实施**(设计定稿见 `designs/ash-gui-external-backend.md`)
- 上游:Plan 060(契约归一 + M3 手写宿主)、Plan 057(HTTP 一等模式)
- 跨仓:auto-shell(主)+ **auto-lang(引擎侧,见 §3 实施约束)**

## 0. 目标(用户裁定原文)

后端(ash-server)做成自带 api.at 的 Auto 项目,纯后端形态;对外接口即
api.at,启动参数决定 HTTP(独立服务)或 merged(VM 动态链接库加载);
前端 ash-gui-auto 的 back/ 不再自写,pac.at 配置指向外部后端项目。

验收总则:
- `cd ash-gui/ash-server && auto run --http` 起独立服务(行为 = 现有
  ash-server bin);
- `cd ash-gui/ash-gui-auto && auto run -r vm` 直接可用(merged,装载
  ash-server cdylib,进程内真 ash-core)——**不再需要 ash-runner**;
- 前端 back/ 目录清空后全套测试(pytest + MCP)口径不劣于 060 R16
  (63 pass / 44 skip / 0 fail)。

## 1. 现状基线(2026-08-23)

- merged 现役入口 = ash-runner(手写宿主,`register_bridges` 10 端点);
- api.at 在前端 `ash-gui-auto/src/back/`,shell.at 空桩;
- 引擎 master 有 RC 金丝雀确定性崩溃(060 §十六轮补记,419 域)——
  **实施基线必须避开该坑**(见 §3);
- 机制存量:`register_host_call` / `inject_shell_event` / libloading
  装载先例均已验证可用。

## 2. 任务拆解

### M1 ash-server Auto 化(auto-shell,先行,零引擎依赖)

- **T1** 迁移契约:`ash-gui-auto/src/back/api.at` → `ash-server/api.at`
  (项目根,与 pac.at 同级);前端 back/ 暂留转发引用(迁移期双源,
  M3 切换时清除)。
- **T2** ash-server 加 pac.at(声明纯后端 Auto 项目 + cdylib 产物约定)
  与 `crate-type = ["cdylib", "rlib"]`;新增导出
  `auto_backend_register(host: *const HostVtable) -> i32`(ABI 版本参数、
  10 端点注册逻辑自 `ash-runner::register_bridges` 迁入,基本现成)。
- **T3** HostVtable 定义(宿主回调表:register_host_call / inject_event /
  log):放中立 crate(auto-lang 侧定义类型,ash-server 引用——方向合法)。
- 验收:同一 cdylib 既能被 ash-runner 手动装载(过渡期),也能被 M2 的
  `auto run` 装载;`auto run --http` 等价现有 bin。

### M2 引擎侧(**auto-lang,必须 worktree 实施,见 §3**)

- **T4** pac.at 外部后端配置:解析 `back: { project: <path> }`;前端
  编译期只读后端 api.at 做契约签名检查(不装载、不执行)。
- **T5** `auto run` merged 编排:按 back.project 定位 cdylib(后端 target
  产物;缺失报"先构建后端")→ libloading 装载 → 校验 ABI 版本 → 调
  `auto_backend_register`(传 HostVtable)→ 既有 `auto run -r vm` GUI
  流程不变。
- **T6** 事件回流:确认插件线程经 HostVtable.inject_event →
  inject_shell_event 通道畅通(理论零新代码,验证 + 必要适配)。
- 验收:引擎测试新增 back.project 配置用例;最小 demo(前端 + 外部
  echo 后端)merged 跑通。

### M3 前端切换与退役(auto-shell)

- **T7** ash-gui-auto:pac.at 加 `back: { project: "../ash-server" }`;
  删除 back/ 全部桩(shell.at 退役);conftest/AUTO_BIN 默认入口改回
  `auto run -r vm` 形态。
- **T8** ash-runner 退役:run_vm.ps1/sh 改为薄封装(`auto run -r vm`);
  ash-server bin(HTTP)保留;README/plan 文档收口。
- **T9** 全量回归:pytest 全套 + MCP 功能口径(060 §4 清单 + BI-01..04
  桥回归)+ HTTP 模式交叉冒烟(060 遗留顺延项一并补上)。

## 3. 实施约束:auto-lang 一律 worktree(硬性)

auto-lang master 长期被并发 agent 占用(Plan 419 等),且当前有 RC
金丝雀崩溃未修。**T4/T5/T6 全部在 worktree 实施**,工作流照抄 060 R16
已验证模式:

1. `git -C D:\autostack\auto-lang worktree add .worktrees/plan-061 -b plan-061 <稳定基线>`;
   基线选择:RC 崩溃修复后的首个绿色 commit(或 db8a4600 后继稳定点),
   实施前先在 worktree 跑 ash-gui 冒烟确认无 RC panic;
2. junction:`D:\autostack\auto-shell\.worktrees\auto-lang` → 该 worktree
   (注意:当前 junction 指向 ash-bridge-060,切换前确认无并发构建);
3. 验证构建在 auto-shell 侧 worktree(如 `.worktrees/plan-061`)进行,
   主检出零接触;
4. 合并窗口:engine master 空闲期(工作区干净)一次性 merge,提交前
   跑 auto-lang 全量测试;冲突高发文件(renderer.rs/dynamic.rs)提前
   rebase 预演;
5. 合并后重建主检出 ash-server + auto.exe,恢复 junction 指向主检出。

auto-shell 侧(T1-T3/T7-T9)在 main 或专用分支直接实施,无并发冲突
(本仓其他会话主要动 docs/tests)。

## 4. 风险与对策

| 风险 | 对策 |
|---|---|
| cdylib 与宿主 ABI 漂移 | 注册入口带版本号;同机同 target 构建天然同版 |
| RC 崩溃修复周期不定 | §3 基线选择规则;merged 验证前强制冒烟 |
| 契约迁移期双源漂移 | M1→M3 间隔压短;前端编译签名检查兜底 |
| HostVtable 放哪(auto-lang 不依赖 auto-shell) | 类型定义在 auto-lang(中立),ash-server 消费;方向合法 |
| 419 并发合并冲突 | worktree + 短生命周期分支 + 合并前 rebase 预演 |

## 5. 遗留(预期)

- a2r 后端产线(生成 Rust 后端作为 cdylib 另一来源):a2r 修复后另议;
- 后端 cdylib 的跨平台签名/符号裁剪:Windows 优先,Linux/macOS 随后;
- pac.at `back` 语法进 AutoUI 官方 schema 文档(auto-lang 侧)。

## 6. 执行记录(2026-08-23,M1+M2 全量落地,E2E 全绿)

### 交付形态(与设计的偏差)

| 设计 | 实施 | 理由 |
|---|---|---|
| C-ABI 插件入口(草案) | `extern "Rust"` + **Arc<dyn BackendRegistry>** 交割 | 同机同工具链构建(宿主与后端同 target 树),Arc 是刚需:后端事件泵线程须持宿主 registry 回流事件 —— `&dyn` 不可跨线程。**关键教训**:cdylib 场景进程内有两份 auto_lang(宿主 + cdylib 各一),后端若调本地 `inject_shell_event` 写的是休眠副本 → 事件全丢、块永挂 Running(实测);必须经宿主 registry 注入 |
| 前端编译期"只读"外部契约 | **契约同步式引用**:run 时把后端 api.at 复制到本地 src/back/api.at | 编译器/loader 零改动,`use back.api` 路径不变;后端仍是契约唯一真源(本地副本是生成物) |
| (未定)worker 初始化时机 | cdylib 注册入口内**boot 探活**(command_list) | ① fail-fast;② Shell 会话 cwd 惰性取首次调用时的进程 cwd,宿主随后 chdir 到 src/front —— 先发制人把 cwd 锁定在项目根(否则起始 cwd 漂移到 src/front,`cd <项目名>` 语义错位) |

### 关键改动

- **auto-lang(worktree `plan-061`,基线 dba0b9a4,commit 848a666b)**:
  `vm/backend_abi.rs`(trait+装载器)、pac.rs `external_backend` 解析、
  rust_ui.rs merged 分支装载编排(同步契约 + 定位 cdylib + 注册;库名取
  后端 pac.at name,debug 优先,缺失明确报错)。
- **auto-shell(worktree `plan-061`,commit e66cb4f)**:ash-server Auto 化
  (api.at/pac.at 迁入项目根)、lib crate-type += cdylib、src/backend.rs
  共享装配(10 端点注册 + registry 事件泵)、ash-runner 重构为薄过渡
  (assemble_host_bridge)、前端 pac.at `back: {project: "../ash-server"}`。

### 验证(2026-08-23)

- **E2E(裸 `auto run -r vm` + ash-server cdylib)**:boot 81 命令 / echo /
  ls 表格+着色 / `ls \| where` 管道 / cd 往返(含连字符目录)/ show 代码
  块 / Stop 取消(Cancelled 终态)/ 会话起始 cwd=项目根 —— 全绿。
- **pytest 全套**:**63 pass + 44 skip,零失败**(与 ash-runner 形态完全
  同口径,AUTO_BIN=worktree auto.exe 走 `run -r vm` 参数路径)。

### 过程事故与恢复

并发会话执行了全局 worktree 清理,auto-lang `.worktrees/plan-061`
(当时未提交)连带删除 —— 改动全量重放(内容在案)+ 恢复构建重新冒烟
通过后立即提交。**教训:worktree 改动必须小时级提交,不能隔夜裸放。**

### 遗留(下一步)

- **M3 收尾**:主检出 auto.exe/master 合并 plan-061 分支后,run_vm.ps1/sh
  切 `auto run -r vm`,ash-runner 退役(auto-shell 侧改动已就绪);
- **合并窗口**:auto-lang master 活跃(RC 金丝雀崩溃仍未修,见 060 §十六
  轮补记),plan-061 分支(基线 db8a4600+R16 桥)待窗口期合并;
- HTTP 交叉验证(auto run --no-merge + ash-server :3000)顺延;
- 契约签名校验(编译期比对前端调用与 api.at 签名)未做 —— 同步式引用
  下前端编译即读真契约,弱校验已天然成立,强校验待需求。
