# TODO — 长远方向与 deferred 事项

> 这些事项**不是当前 ash-gui-vue 计划的一部分**(当前计划见 `docs/plans/old/039-041-*.md`),而是等完整 ash-gui 跑通后再评估的方向。按领域分组,不承诺时间。

## 工程审核整改(来源:REVIEW-2026-08-26.md)

> 2026-08-26 全工程审核出的 P1/P2/P3 项。**P0 安全项已实施归档:`docs/plans/archive/071-security-hardening.md`**;此处登记其余各项,逐个另立计划实施,编号顺延 071+。

### P1(结构止血,建议紧随 070)

- [ ] **CI 修复与全量化**:ash workspace `cargo test --workspace`(ash-tui 138 单测与 examples e2e 回归网入 CI);ci.yml:79 parity 选择器指向改名前的包,疑似失效需修;rustfmt/clippy 从 continue-on-error 转硬门槛。(候选 Plan 071,半天量,可与文档止血合并)
- [ ] **文档止血**:README/for-agents/SKILL 删除代码中不存在的 `ash agent` 子命令族与已退役 F3 流程;README 项目结构图修正(auto-shell/ash 角色写反、漏 ash-tui);补一个"文档冒烟"脚本(校验文档中 `ash xxx` 子命令真实存在)。
- [ ] **合并根 workspace**:统一 Cargo.lock/target,消 axum 0.7+0.8 双版本与 windows 0.58/0.61/0.62 三版本共存(双 target 实测 ~6.9GB)。
- [ ] **shell.rs 第一刀拆分**:内联 builtins(cmd_* 19 个)迁入既有 registry 抽象 + expansion 抽独立模块,调用点不动。
- [ ] **前后端契约 codegen**:serde 结构体为源生成 TS 类型,删 ash-server/src/types.rs 与 vue 侧 api.ts 两处手工镜像。
- [ ] **外部命令 fallback 引号注入修复**(external.rs 3+3 处 PS/sh 拼接不转义)+ **中文控制台编码**(全仓 UTF-8 硬编码,cp936 下必乱码)。
- [ ] **子进程超时与上限**:cmd.output() 无超时、捕获 stdout 无内存上限、每 spawn 泄漏 wait 线程;按 DEBTS 既定务实补丁路线(sleep/http 上限)起步。

### P2(内核偿债,一个月)

- [ ] PipelineData→AtomPipeline 迁移定截止日期,桥接层(pipeline_convert.rs)限期拆除。
- [ ] 错误类型分域:thiserror 枚举替代 miette 即时字符串(现全仓仅 1 处 derive),denial 可编程识别;清理 157 处 `let _ =`/`.ok()` 吞错聚集。
- [ ] 解析器诊断化:未闭合引号报错而非静默吞到 EOF;lexer 与 external.rs parse_command 两套 tokenizer 合一。
- [ ] lazy operator 全覆盖(FilterAll/Map/Reverse 等中途 collect)与 cat/grep 大文件分块(现 17 命令全量 read_to_string)。
- [ ] 安全集成测试套件:REVIEW §二每条发现一个红→绿用例。
- [ ] host.rs 重入死锁风险:锁外调用 + debug 断言(unsafe Send/Sync 担保机制化)。

### P3(季度)

- [ ] 平台抽象层:cfg 分支散落 11 文件、进程控制三套机制(external_stream/job.rs WinAPI/libc)语义不对称,收敛合一。
- [ ] ash-server/GUI 测试建设(现 ash-server 0 测试)与前端测试选型(vitest vs 依赖 .at 侧校验)。
- [ ] plugin 供应链:域名白名单/签名/lockfile,capabilities 强制化(现仅 stderr 警告);`.ashrc` 与插件加载先于 set_policy 的时序问题。
- [ ] 文档体系补全:ARCHITECTURE.md、THREAT-MODEL 一页纸、TESTING.md、ADR 索引(plans/archive 重编排)。

## 真终端专属能力(ash-gui-vue 范围外)

这些是 TUI/CLI ash 有、但一个 webview GUI 天然做不了的——需要**嵌入式终端模拟器**方案才能支持。参考 `ash-core/src/cmd/interactive.rs:10-52` 的交互命令清单。

- [ ] **全终端模拟 / 交互命令**:`vim`、`top`/`htop`、`ssh`、`tmux`、`man`、`psql`、`python` 等需要 PTY + 原始模式的程序,在 GUI 里无法直接运行。
  - **候选方案 A(嵌入终端)**:集成 WebView 内嵌终端(如 `@xterm/xterm` 前端 + 后端 PTY 桥,或 `go-horizontal`/`ttyd` 类方案),为"需要交互的命令"打开一个终端面板。类似 VS Code 的集成终端。
  - **候选方案 B(外部程序)**:检测到交互命令时,用系统终端(Windows Terminal / Alacritty / kitty)启动,命令以 `ash -c "..."` 传递。最简单,但跳出 GUI。
  - **候选方案 C(专用协议)**:对少数高频命令(编辑器)做"编辑器面板"定制(如内置文件树 + 编辑器),而不是通用终端模拟。对齐 ash 的"结构化原生"定位。
- [ ] **raw-mode / 备用屏幕 / 真彩检测**:`ash-tui/src/term/color.rs`、`commands.rs:98-117` — 这些是终端内部机制,GUI 不需要(webview 自有渲染)。
- [ ] **OS 级管道直通**:`ExternalStream::into_raw_stdout`(`external_stream.rs:97-111`)— 把子进程 stdout 直接连到终端 fd,GUI 场景应改用流式事件(见 040 M4)。

## 需要谨慎评估的架构项

- [x] **`.at` 复刻**:ash-gui-auto 已完成 .at 复刻(VM 模式可跑)。VM 模式下命令执行闭环已打通(SSE 桥)、12 处 Vue→Auto 差异已对齐、56 测试用例 pass。详见 `designs/ash-gui-native-plan.md`。**a2r(可分发二进制)路径待 codegen 修复**(2026-08-08 二次实测 72 个编译错误,全前端 main.rs;`--server rust` merged mode 绕过后端。见归档文档 §4)。
- [x] **iced 版去留**:已退役(2026-08-23)——`ash-gui-bin` 与手写 `ash-gui-vue` 参考实现一并删除(.at 复刻期参考使命完成,ash-gui-auto 已全面超越;git 历史保留)。ash-gui 目录现为 ash-gui-auto(.at 项目)+ ash-server(外部后端)双成员。
- [ ] **跨后端一致性**:TUI / iced / Vue 三端的配色与状态语义应统一到单一来源(当前分散在各前端)。可能抽一个共享 token 定义。
- [ ] **VM hover 通用化(方案 B,悬停状态 → 消息 → 视图重建)**:2026-08-21 已落地 hover:bg-* 与图标 hover:text-*(方案 A:iced button Status + svg 画时着色,零重建),但仅限这两类。浏览器级 :hover 语义(任意 hover 类生效、`group-hover:opacity-100` 操作区悬停显隐等)需要:mouse_area on_enter/on_exit(iced 0.14 已有)或全局光标订阅 + layout_collector 包围盒 → state 存 hovered vnode → 触发一次 view 重建,转换器把 hover 上下文向下传(参考 inherit_text_color 模式)。代价:每次悬停进出各一次重建(iced 本来每消息都重建,本应用规模无压力)。前置条件:若实现 opacity 类,VM 端 `opacity-0 group-hover:opacity-100` 的"常显"行为会被打破,此项即为依赖。改动应在 auto-shell worktree 的 auto-lang 里做。

## 与外部生态的协同

- [ ] **auto-lang WIP 稳定性**:另一个 agent 在 master 上实时修改 auto-lang(Plan 364/380)。当前 ash-gui-vue 联调依赖 auto-lang 编译稳定。长期看,ash 的构建应能容忍 auto-lang 的中间状态(如 pin 到稳定 commit)。
- [ ] **测试覆盖**:ash-gui-vue 目前无前端测试(无 vitest/playwright)和后端命令测试。跑通完整功能后补。
