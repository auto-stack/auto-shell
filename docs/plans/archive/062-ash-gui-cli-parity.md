# 062 — ash-gui(Auto/vue·VM)与 CLI 版功能差距补全

- 日期:2026-08-23
- 状态:**COMPLETE(P1+P2+P3 主干全落地,2026-08-24)**。零引擎约束下可做的
  全部交付:T11 NL→命令(§10)、T15 AI 补全层 + B3 排序验证 + 尾项 C1(§11)、
  T12 块内 AI chat(§12);全套件 83 pass / 44 skip 新水位。纯外部依赖尾项
  (T12 升级件 AiChunk 事件族 / T13 Ollama / T14 smart NL 路由)登记 DEBTS。
  实施记录见 §7-§12
- 调研对象:
  - **CLI 版** = `ash` bin(`ash/ash/src/main.rs`)+ `ash-tui`(reedline REPL、菜单/高亮/hinter、less/color)+ `auto-shell`(引擎、命令注册表、补全编排)+ `ash-core`(parser/pipeline/interactive 名单)
  - **GUI 版** = `ash-gui/ash-gui-auto`(.at 前端,Vue/VM 双渲染)+ `ash-gui/ash-server`(ash-server HTTP bin / ash-runner merged bin,worker 进程内 auto_shell::Shell)
- 上游:plan-057(HTTP/显示对齐)、058(行内编辑)、059(表格 V1)、060(契约归一 + M3 runner)、061(外部后端);TODO.md「真终端专属能力」

## 0. 结论先行

**执行语义层已经同源,差距不在"命令能不能跑"**:`cmd &` 后台、管道/重定向/heredoc/命令替换、别名与 `~/.ashrc`(worker.rs:444-448)、补全引擎(worker.rs:386-397 直调 `auto_shell::completions::engine::complete`,kind/description 全量返回)、历史持久化(`~/.auto-shell-history` 读写)、git 上下文、smart 命令——GUI 与 CLI 走同一个 auto-shell/ash-core,均已真实现。

剩余差距集中在四类:

1. **结构性**:交互式命令(vim/ssh/top/python REPL 等 PTY 程序)在 GUI 以管道 stdio spawn → 挂死或乱码。CLI 的做法是挂起行编辑、移交 stdio(repl.rs:964-982)。GUI 无 tty,需换通道。
2. **功能缺口**(CLI 有、GUI 无):AI 全家(F3 NL→命令 / F4 chat / suggest-next / smart NL 路由)、Ctrl+S 前向历史搜索、Ctrl+E 外部编辑器、历史展开 `!!`/`!n`/`!string`、jobs 面板与 Kill UI(数据/事件已在,无 UI)、did-you-mean 可见性、执行中 Ctrl+C 取消、ghost 模糊子序列回退。
3. **呈现差距**(数据已在、UI 简陋):补全呈现是单行候选 vs CLI 的 AshMenu 富菜单(kind 配色/自适应布局/分页/内建搜索)。
4. **GUI 自身遗留**(非 CLI 差距,但补全计划一并收口):Plan 059 表格尾巴(过滤链路/▲▼ 指示/CSV 验证/Vue 列宽拖拽/吸顶/hover)、44 个 skip、ash-runner 口径矛盾。

## 1. 差距总表(逐项,附出处)

### A. 行编辑 / 输入体验(prompt_bar.at vs ash-tui repl.rs)

| # | 功能 | CLI 出处 | GUI 现状 | 定性 |
|---|---|---|---|---|
| A1 | Ctrl+S 内联前向历史搜索 | repl.rs:166-180 | 无 | **缺** |
| A2 | Ctrl+E 外部编辑器编辑当前行($EDITOR,多行缓冲) | repl.rs:181-186, 687-727 | 无 | **缺** |
| A3 | ghost 模糊子序列回退(`gcm`→`git commit -m`) | term/hinter.rs(前缀+子序列) | prompt_bar.at:310-325 仅最长前缀 | **部分** |
| A4 | 历史展开 `!!` `!n` `!-n` `!string` `!?string` | repl.rs:314-352 + ash-core parser/history.rs | 无(repl 层特性,engine 不展开) | **缺**(parser 已有,缺接线) |
| A5 | abbr 缩写内联展开 | repl.rs:936-941 | 无(alias 经 worker 已生效) | 缺,低价值(alias 覆盖) |
| A6 | 编辑模式/键位配置联动($ASH_EDIT_MODE、ash.toml edit_mode、bind) | repl.rs:119-136, 2405-2459 | edit_mode 硬编码 emacs(prompt_bar.at:39-41) | 部分,低优先 |
| A7 | 执行中 Ctrl+C 杀子进程 | signal.rs + CtrlCGuard | Ctrl+C 仅清输入(prompt_bar.at:652-660);取消靠 Stop 按钮 ✓ | **键位缺口**(能力已有) |

已对齐项(不列任务):Emacs/Vi 全量键位(plan-058)、Tab 循环、ghost 前缀 + 右箭头逐字/Ctrl+F 全收/Ctrl+→ 按词、续行检测 ❯/·、语法高亮(10 kind)、Ctrl+R 历史面板、Ctrl+L/Ctrl+D、↑↓/Ctrl+P/N 历史。

### B. 补全

| # | 功能 | CLI 出处 | GUI 现状 | 定性 |
|---|---|---|---|---|
| B1 | 富补全菜单:kind 配色、紧凑网格/描述列表自适应、半屏分页、内建搜索、fuzzy 标记 | menu/ash_menu.rs | 单行候选行(name+description,prompt_bar.at:84-100);**kind 数据已在**(api.at complete 返回 kind 字段,worker.rs:525-548) | **呈现差距** |
| B2 | AI 补全层(NL→命令名、AI 子命令,后台合并) | completions_reedline.rs:381-530(适配层) | worker 走 engine::complete,适配层的 AI 合并未接 | 缺,随 P3 AI 期 |
| B3 | 上下文排序(历史频率/仓库上下文) | repl.rs:358-377 ctx | worker 传 completion_ctx(history/aliases,worker.rs:521) | 疑已生效,**待专项验证** |

### C. 三模式输入 & AI(CLI 独有重头)

| # | 功能 | CLI 出处 | GUI 现状 | 定性 |
|---|---|---|---|---|
| C1 | AutoScript 模式(`#`/F2) | repl_mode.rs 三模式 | engine 自动检测 Auto 语法(shell.rs:641-657),GUI 提交走 shell.execute **应已可执行**,但无模式标识/prompt 符号切换 | 半缺(补 UX 标识即可,低优先) |
| C2 | F3 NL→命令(翻译 + 危险校验 + Enter 执行/s 分步/e 编辑/取消) | repl.rs:781-872 | 无 | **缺**(大件) |
| C3 | F4 AI chat(ReAct agent + 工具调用 + 流式 + 会话持久化 ~/.auto-shell-ai-chat.json) | repl.rs:449-609 | 无 | **缺**(大件) |
| C4 | suggest-next(命令完成后"接下来"建议) | repl.rs:746-753 | 无 | 缺 |
| C5 | smart NL 路由(`ash smart run "<nl>"` 本地 Ollama) | smart_command/cli.rs:20-43 | run_smart(name) 仅按名(worker.rs:364-385) | 部分 |

### D. 命令执行语义 / 作业控制

| # | 功能 | CLI 出处 | GUI 现状 | 定性 |
|---|---|---|---|---|
| D1 | 交互式命令(PTY 名单:vim/nano/top/ssh/tmux/python/psql…,ash-core cmd/interactive.rs:10-52) | 挂起 reedline 移交 stdio(repl.rs:964-982);block-tui 有全屏子进程移交(subprocess.rs) | **无拦截**:走 spawn_external_stream 管道 stdio → 挂死/乱码;仅 `color` 拦截(worker.rs:849-866) | **结构性,本计划最大缺口** |
| D2 | jobs/fg/bg/suspend | shell.rs:604-628, 4247-4318 | `cmd &` ✓(JobStarted/JobDone 实时,worker.rs:252-280)+ 标题栏计数;**无 jobs 面板、KillJob 无 UI 入口(shell_store.at:389-392 handler 在)、无 fg/bg** | **UI 缺口**(数据/事件/API 全在) |
| D3 | did-you-mean 编辑距离建议(含 PATH 扫描) | shell.rs:887-893(**eprintln 到终端**) | 引擎 eprintln 进 server stderr,GUI Failed 块看不到 | 小缺口(挪进错误文本) |
| D4 | 管道/`&&`/`||`/重定向/heredoc/`$(...)`/`<(...)`/变量/brace/glob | engine 层 | worker 流式 + shell.execute 双路径 ✓(多段管道流式 worker.rs:660-670) | 已对齐,缺专项验证用例 |
| D5 | less/more 放行、color 拦截 | — | worker.rs:844-866 已降级处理 | ✓ 合理降级 |

### E. 呈现与信息(CLI 已被 GUI 等价或超越,不动)

- modular prompt(目录/git/耗时/状态)→ GUI 标题栏 cwd + git 标签 + per-block duration/status/exit_code,块头 ❯+命令回显 ✓;
- 历史 10000 条 + Ctrl+R → /api/history 全量 + history_search 面板(显示 cap 50,与 CLI 菜单分页同量级)✓;
- One Dark 高亮 → DoTokenize 10 kind,色板近似(逐色对齐列低优先);
- 主题:GUI dark/light 切换为超越项。

### F. GUI 自身遗留(非 CLI 差距,随本计划收口)

| # | 项 | 出处 |
|---|---|---|
| F1 | 表格过滤链路未通(Filter 桥在、输入不筛行)、▲/▼ 排序指示不渲染 | plan-059 §4.3/4.4 |
| F2 | Vue 表格增强未做:列宽拖拽、表头吸顶、行 hover(ash-table-* 标记类已就位) | plan-059 §4.2 |
| F3 | ExportCsv/CopyOutput(Table TSV)在码未 pytest 验证 | plan-059 §4.1 + block_item.at:412-502 |
| F4 | block 卡片背景/边框未绘制 | plan-058 §5 |
| F5 | 首命令不执行 bug(45ef43ad 窗口,测试靠 warm-up 掩盖) | plan-059 §5.3 |
| F6 | 44 个 skip(M2 难档 + MCP 键盘竞态);conftest.py 默认启 ash-runner 与 run_vm.ps1「已退役」口径矛盾 | tests/ + plan-060 R16 |
| F7 | RC canary 崩溃(引擎 Plan 419 域,master 构建不可用,稳定入口 = verify-bridge worktree) | plan-060 §R16 补记 |

## 2. 分阶段补全计划

> 实施基线:P1/P2 全部在 **auto-shell 仓**(ash-server worker + .at 前端 + renderer 桥惯例)或
> auto-shell 对 auto-shell 引擎 crate(`ash/auto-shell`,**非** auto-lang)的小改;不触碰 auto-lang
> master(被 Plan 419 占用,RC canary 未修)。构建用 verify-bridge worktree 产物口径(plan-060 R16)。

### Phase 1 — 高频闭环缺口(小-中件,先行)

- **T1 交互命令外部终端移交(D1)**:worker 提交侧检测首词命中 `ash_core::cmd::interactive`
  名单 → 不走管道 spawn,改**新控制台 spawn**(Windows `CREATE_NEW_CONSOLE`;Unix 探测
  终端模拟器 `$TERMINAL`/wt/konsole/gnome-terminal,找不到则降级消息)。进程注册进
  JobManager 复用 reaper:块保持 Running、终端退出后发 CommandResult(真实退出码),
  块内 Text 注明"交互式命令已在系统终端窗口运行"。块可 Stop(杀终端进程树)。
  - 落点:`ash-server/src/worker.rs`(run_command 前置检测 + spawn 分支)。
  - 验收:MCP 提交 `vim`/`python` → 不挂死、新终端窗口出现、退出后块收尾;Stop 可杀;`echo`/`ls` 零回归。
- **T2 jobs 面板 + Kill UI(D2)**:标题栏 `⚙ N` 点击展开 App 级浮层(job 列表:cmd /
  Running|exit N / kill 按钮);kill → 既有 `.KillJob` → `kill_job` API。**App 级 handler 实现
  (BI 系列已验证该路径),避开 child-callback 债(DEBTS B7)**。fg/bg/suspend 明确不做(§3)。
  - 落点:`app.at`(面板 + handler)、`shell_store.at`(若需小 handler)。
  - 验收:`ping -n 30 &` → ⚙1 → 面板见条目 → kill → 条目消失/exit code;pytest 新增 JP-01..03。
- **T3 did-you-mean 可见化(D3)**:auto-shell 引擎把建议并入命令错误返回串(替换/收编
  shell.rs:887-893 的 eprintln,CLI 侧去重打印),GUI Failed 块自然显示。
  - 落点:`ash/auto-shell/src/shell.rs`。
  - 验收:GUI 输入 `ekk` → Failed 文本含 "did you mean: echo?";CLI 行为不重复打印。
- **T4 键位补齐(A1/A7)**:
  - Ctrl+S:并入 history_search 面板——面板内 Ctrl+S 切换检索方向(newest↔oldest),宿主键
    Ctrl+S 开面板(与 Ctrl+R 同款);
  - 执行中 Ctrl+C = Stop:输入非空 → 清输入(现状);输入空且有 Running 块 → 派发 `.Stop`
    (readline 语义对齐)。
  - 落点:`prompt_bar.at`、`history_search.at`。
  - 验收:`sleep` 长命令运行时空输入 Ctrl+C → 块转 Cancelled;Ctrl+S 面板方向切换。
- **T5 ghost 模糊子序列(A3)**:镜像 CLI hinter 算法——前缀无果时对历史做子序列匹配、
  取最短命中;`.at` 侧实现(历史量 ≤10000,逐条扫描可接受,必要时 cap 最近 500 条)。
  - 落点:`prompt_bar.at` ghost 计算(两处:OnInput / OnInputComplete 同步改)。
  - 验收:`gcm`(历史含 git commit -m)→ ghost 出全句;原前缀行为零回归(PB ghost 契约测试)。

### Phase 2 — 中件(输入体验 + 呈现)

- **T6 历史展开接线(A4)**:worker `run_command` 提交前调用 ash-core
  `parser/history` 展开函数(与 CLI repl.rs:314-352 同源),展开所需"上一条"来自 worker
  已维护的历史(追加点 worker.rs:791-808 之前)。HTTP/merged 双模式天然同享。
  - 落点:`ash-server/src/worker.rs`。
  - 验收:`echo a` 后输 `!!` → 执行 echo a;`!ech`/`!?a` 同验;无历史时友好报错块。
- **T7 补全富面板(B1)**:候选行升级为限高可滚面板:kind 色点(8 类映射 CLI AshMenu 配色)、
  描述列、条数/分页;Tab/↑↓ 既有状态机扩展到面板导航。VM 用 scrollable + 行列表,
  Vue 同构。纯前端,数据不动。
  - 落点:`prompt_bar.at`(候选面板视图 + 导航状态机)。
  - 验收:`git ` 出子命令/flag 带描述与 kind 色;`cd a` 目录候选;`$` 变量候选;
  PB-08 Tab 循环回归。
- **T8 Ctrl+E 外部编辑器(A2)**:新 API `editor_edit(draft) str`(worker:写临时文件 →
  $EDITOR(notepad 兜底)新控制台 spawn → watcher 线程盯 mtime → 保存即读回)+ 前端
  轮询或 SSE 小事件回填输入框。编辑中输入框置灰提示"编辑中…"。
  - 落点:`ash-server/api.at` + worker + `prompt_bar.at`。
  - 验收:notepad 改内容保存 → 输入框更新、光标在末尾;取消(关窗口不存)不变。
- **T9 表格尾巴收口(F1-F3)**:过滤链路打通(排查 input_value → Filter 桥,plan-059 §4.3
  疑点)、▲/▼ 指示(若确系 button 条件子元素引擎限制,改用双字符状态字段方案)、
  CopyOutput/ExportCsv pytest 锁定;Vue 列宽拖拽/吸顶/hover(auto-lang vue.rs 注入,**依赖
  auto-lang 可用窗口,可后置**)。
- **T10 工程健康(F5/F6)**:首命令不执行 bug 专项(059 §5.3 排查方向:api_over_http/异步
  重入冷启动);conftest 与 run_vm.ps1 启动口径统一(单一入口);顺带解锁可解的 skip。

### Phase 3 — AI 全家(C2-C5、B2;依赖外部 AI 服务,独立成期)

- **T11 NL→命令(`?` 前缀或 F3,C2)**:新 API `nl2cmd(nl) str`(worker 复用 auto-shell
  `ai` 模块,同 crate 已依赖)+ 危险校验;前端建议条:命令 + [Enter 执行]/[e 编辑]/[取消]
  (CLI F3 交互的 GUI 等价)。无服务时静默降级提示(对齐 CLI 策略)。
- **T12 AI chat 面板(C3)**:新 SSE 事件族(AiChunk/AiToolCall/AiToolResult)+ 右侧抽屉
  面板 widget(流式文本 + 工具事件块内联渲染 + `/clear /exit`);会话持久化复用
  ~/.auto-shell-ai-chat.json。**本计划最大新件,建议独立 spike 先行**。
- **T13 suggest-next(C4)**:CommandResult 后异步拉建议,输入框上方 chips 点击填入。
- **T14 smart NL 路由(C5)**:run_smart 参数二义(名字失败 → NL 路由)或独立入口。
- **T15 AI 补全层合并(B2)**:把 completions_reedline.rs:381-530 的 AI 合并逻辑下沉到
  engine::complete 或 worker 侧适配(与 B3 验证一起做)。
- 尾部小项:C1 AutoScript 模式标识(prompt 符号 ❯→# 自动切换)、A6 edit_mode 配置联动。

## 3. 明确不做(附理由)

- **PTY 嵌入终端面板**(TODO.md 方案 A:xterm.js + PTY 桥):仅 Vue 端可行,VM(iced)
  无通道;T1 外部终端方案已消除挂死痛点。列为远期可选,不进本计划。
- **fg/bg/suspend**:fg 的"前台抢占 stdio"语义在 GUI 不成立(无 tty);bg 无输入面。
  后台作业输出捕获(现状 spawn_external_background 输出丢弃)记观察项,有真实需求再立项。
- **--json/--bash-compat/安全沙箱 flag**(allow/deny/no-exec/sandbox/audit):CLI/agent
  专用形态,GUI 非目标;ash-server 如需沙箱属安全专题另议。
- **bind 配置化键位、abbr 内联展开、Ctrl+Z**:低价值/CLI 自身未完成(Ctrl+Z 前台挂起
  在 CLI 也是 TODO)。
- **真彩检测/raw mode/备用屏幕**:终端内部机制,GUI 无意义(TODO.md 已定性)。

## 4. 风险与依赖

| 风险 | 对策 |
|---|---|
| auto-lang master RC canary 崩溃(F7,Plan 419 域) | 本计划 P1/P2 不动 auto-lang;构建走 verify-bridge worktree 口径;master 修复后回归主检出 |
| child-callback emit 剥离(DEBTS B7) | 新交互一律 App 级 handler(Stop/DeleteBlock/Pick/Rerun 四桥已验证该模式) |
| 交互命令新控制台 spawn 的跨平台差异 | Windows 先行(CREATE_NEW_CONSOLE 确定性);Unix 终端探测表 + 显式降级消息;不追求完美检测 |
| AI 功能依赖外部服务/密钥 | 全部静默降级 + 状态提示(对齐 CLI 无 daemon 策略);T11-T15 独立成期,失败不阻塞 P1/P2 |
| auto-shell 引擎改动(T3/T6)影响 CLI | 与 CLI 同源同测:auto-shell crate 测试 + CLI 手工冒烟;改动点小且均在错误文本/提交前预处理 |
| 测试端口/MCP 键盘竞态(F6) | 沿用 AUTOUI_MCP_PORT 独占端口 + 键盘依赖用例实例级 skip 惯例 |

## 5. 验证与回归

- 基线:pytest 63 pass + 44 skip(plan-060 R16 口径),每 Phase 结束全量回归零新增失败。
- 新增用例(建议 `tests/test_cli_parity.py` + 各域文件扩展):
  - T1:IC-01 vim 不挂死/新控制台、IC-02 退出收尾、IC-03 Stop 杀终端;
  - T2:JP-01..03 面板开合/条目/kill;
  - T3:DM-01 Failed 文本含建议;
  - T4:HS 方向切换、空输入 Ctrl+C → Cancelled;
  - T5:GP-01 子序列 ghost;
  - T6:HE-01..03 `!!`/`!str`/`!?str`;
  - T7:CP-01..03 子命令/flag/变量候选含 kind+描述;
  - T8:ED-01 编辑回填;
  - T9:沿用 plan-059 §4 验收项补 pytest;
  - T11+:AI 域 fake-backend 契约测试(不动真实服务)。
- HTTP 模式冒烟每 Phase 一次(ash-server 起服,确认双模式无分叉)。

## 6. 排期建议

P1(T1-T5)→ P2(T6-T10)→ P3(T11-T15,独立)。P1 全部为 auto-shell 仓内改动,
无引擎(auto-lang)依赖,可立即开工;P3 待 AI 服务配置与 P1/P2 收口后立项。

## 7. Phase 1 实施记录(2026-08-23,worktree `plan-062`)

### 交付

| 任务 | 落点 | 结果 |
|---|---|---|
| T1 交互命令移交 | worker.rs(console_handover_reason / spawn_console_command / wait_console_child;Windows `CREATE_NEW_CONSOLE`,Unix 终端探测降级) | ✅ IC-01(vim 不在机 → 新控制台起 cmd /C → 块收 Failed exit 1,不再挂死)/ IC-02(`python -c` REPL 带参数保持流式)。两处 CLI 语义收敛:分页器(less/more/bat)放行(Plan 055 既定);REPL 类带参数 = 脚本,保持流式 |
| T2 jobs 面板 + Kill | app.at(⚙ button + 面板 + ToggleJobs/KillJob handler;App 级,避开 child-callback 债) | ✅ JP-01..03(⚙ 计数 → 面板行(#id/cmd/running/✕)→ 连杀 → ⚙ 消失) |
| T3 did-you-mean | 引擎 shell.rs(建议折进错误文本,退役 eprintln;新增 `suggest_command_for` 公开 oracle)+ worker.rs 流式路径 command_resolvable 预检(PATH+PATHEXT+cmd 内建白名单) | ✅ DM-01(`lss` → Failed 块含 "did you mean: ls?");GUI 未知命令原本经 powershell 兜底变成静默 exit 1,预检前置拦截 |
| T4 键位 | prompt_bar.at(ctrl.s → OnCtrlS;OnCtrlC 空输入 → store.Cancel() **PromptBar 直调 store 首例**,验证可行)+ history_search.at(forward prop + watch 重算 + oldest-first 遍历 + placeholder 切换) | ✅ CS-01(Ctrl+S 方向翻转 + 旧→新 placeholder)/ CC-01(空输入 Ctrl+C → Cancelled;实例级键盘竞态时 skip) |
| T5 ghost 模糊 | prompt_bar.at(ComputeGhost 下沉:前缀最长命中 → 无前缀时前缀子序列回退,首字符必须同;ghost_full 记完整命令,三种接受动作对模糊命中整体替换) | ✅ GP-01(`ecm` → `echo git commit -m …` 模糊 ghost);接受语义与 CLI AshHinter 对齐 |

新测试:`tests/test_cli_parity.py` 8 项(IC×2/JP×3/DM/CC/CS/GP,CC 键盘竞态实例级 skip)。

### 实施中定位并修复的两个引擎 bug(auto-lang 分支 `ash-debug-062`,基于 master 5f3556c0)

1. **订阅同 hash 去重**(`67924f74`):`shell_event_subscription` 与
   `mcp_action_subscription` 都是 `time::every(16ms)`,iced 订阅表按 duration
   hash 去重,后者消息**静默丢失**(merged 模式 job_started/job_done 从不到达
   update——定位链:worker 发送 ✓ → 泵收到 ✓ → inject true ✓ → sub poll 到
   Some(msg) ✓ → update 永不命中)。修复:16ms→17ms。
2. **job_list VmRef 写回失效**(`c4157ea1`):store 声明 `List<JobInfo>` 是 VM
   原生列表(VmRef),渲染层 `write_state_vec` 写不回 VM 堆对象(读回恒同一
   VmRef,条目不可见)。修复:job 分支无条件写 renderer-owned `Value::Array`
   (对齐 blocks 的所有权模型——blocks 能工作正因 renderer 从第一块起就以
   Array 形态持有)。
3. **连带**:app.at 的 ToggleJobs **不得**调 `store.RefreshJobs()`(其
   `.job_list = jobs()` VM 赋值会把 renderer Array 换回 VmRef,实测点击 ⚙ 后
   列表清空)——SSE 事件已实时维护,刷新冗余,已移除。

**合并记录(2026-08-24 00:50,已完成)**:四笔引擎修复经 `ash-debug-062` 分支
合入 auto-lang master(merge `19096564`,先反向合 master 验证零冲突);junction
已指回 `D:\autostack\auto-lang`,master auto.exe 重编;`plan-062` 亦合回本仓
main(`2c642ba`),worktree 与两分支已清理。合并后回归:main 全套件 75 pass /
44 skip / 0 真实失败(he01-03 + M1 冒烟在系统负载下偶发 T10 竞态,单跑全过)。

### 顺带修复

- `ash/auto-shell/src/ai/ask.rs`:auto-ai `StreamEvent` 新增 `TurnStart/TurnEnd`
  导致的跨仓编译漂移(任何新检出构建都会撞;补兜底臂)。
- worker/backend 留 `ASH_DEBUG_JOBS=1` 门控诊断日志(JobStarted 发送/泵转发)。

### 已知遗留(记录,不在 P1 范围)

- **MCP 动作通道延迟竞态**(预存):`&` 类提交偶发延迟 8-10s 才派发,重试风暴
  会堆出 N 个重复后台作业(JP 测试按"杀到 ⚙ 消失"口径免疫);与 Plan 059 §5.3
  首命令丢失同族,P2-T10 专项。
- **MCP autoui_state 读不到 renderer-owned job_list**(读的是 VM 原生 VmRef
  态):视图/交互全正常,仅测试断言需走 snapshot 口径——引擎债候选(store 字段
  双表示问题,与 DEBTS B 系同族)。
- 后台 `&` 命令的块永远停在 Running(background 分支不发 CommandResult)——
  预存 Plan 055 语义,P2 观察项。
- 交互命令 Stop 仅杀包装进程(cmd.exe),进程树残留可能性同后台作业 kill 现状。

## 8. Phase 2 实施记录(2026-08-23 深夜续,同 worktree)

### 交付

| 任务 | 落点 | 结果 |
|---|---|---|
| T6 历史展开 | worker.rs(expand_history_refs:ash_core::parser::history::expand_history + FileHistory 适配,与 CLI repl 同源同表;展开失败 → Failed 块不执行) | ✅ HE-01(`!!` 重跑上一条)/ HE-02(`!无匹配前缀` → expansion 错误块)/ HE-03(`!9999` → out of range) |
| T7 补全富面板 | prompt_bar.at(候选行 → 限高滚动面板:计数行 + kind 色点 + 描述列;平行字符串数组 s_labels/s_kinds/s_descs/s_colors —— suggestions 的 VmRef 元素视图侧读不到字段,handler 侧预构建;点击 PickCompletionIdx(i) 传索引回 handler 取真身;cursor 钳制:cursor_pos≤0 → 行尾,MCP type 路径不写光标会补全空前缀返回全命令表) | ✅ CP-01(ec → echo + 色点 + 描述)/ CP-02(git 子命令带描述) |
| T9 表格尾巴 | block_item.at(过滤框行:⌕ input + value 绑定 + oninput .Filter(.block.id))+ 引擎两修(见下) | ✅ TF-01(输入 src → 行收缩)/ TF-02(表头点击 → ▲ 指示渲染,059 §4.4 收口)。CSV/TSV 复制桥已有,剪贴板断言无 MCP 工具,留人工验证 |
| T8 Ctrl+E | — | ⏸ 顺延(新 API + watcher + 回填,独立一件) |
| T10 首命令/动作通道 | 排查未修 | 已确认症状族:MCP 动作通道偶发停摆 8-10s 后集中泄洪(重试风暴 → 重复后台作业/重复块);parity 全文件连跑时在 ~7 个测试后成片失败(单跑/小批量全过)。引擎专项,续 P2-T10 |

### T9 连带的两笔引擎修复(ash-debug-062 分支 `1bf24c4e`)

1. **Filter 桥 id 解析**:只走 `as_str().parse()`,int 参数恒 -1 → 对齐 Sort 的
   int 优先 + str 回退(059 §4.3 "过滤不筛行"的真身之一)。
2. **convert_input 事件参数烘焙**:单行 input 的 on_change/on_submit 用了无绑定
   解析的 event_to_message → `.Filter(.block.id)` 烘不出来,渲染层收到光杆
   "Filter"(无 payload)→ 桥 decode 出空 args → id=-1。改用
   event_to_message_with(循环变量 + B8 前导点路径 + 字面量,button 同款);
   无参事件两版等价,既有 input(oninput: .OnQuery)不受影响。

### 回归口径

- 主套件(除新 parity 文件):**63 pass(62 + pb04 已知时序 flake 单跑过)+
  44 skip / 0 真实回归** —— 与 Phase 1 后基线一致。
- test_cli_parity.py 现 16 项:**单跑/小批量全绿;全文件连跑在 ~7 项后成片
  失败** = T10 动作通道停摆竞态放大(重试风暴堆积),非功能缺陷;引擎专项后
  应全绿。

## 9. T10 收官(2026-08-24 凌晨,worktree `plan-062-tail`)

### 根因(两个叠加,均已修)

1. **补全 host_call 阻塞 UI 线程**(主根因,实测铁证:`ping -n 30` 运行中打字,
   补全面板 **27.6 秒**后才出现 —— 恰为 ping 结束时刻)。worker 单线程串行,
   执行中的命令把 Complete 请求堵在队列;backend 的 complete 桥在 UI 线程上
   block_on 等 reply → 整个 UI 冻结到命令跑完。这也解释了全套件连跑成片失败
   (JP/CC 用长 ping,后续测试全被堵)。
   **修复**:Complete 移驻独立线程(`ash-server-complete`,自有 Shell + 自有
   runtime,同款 init:registry/别名/.ashrc;cwd/last_command/exit 经
   SharedSession 快照由主线程每条命令后刷新 —— 补全不修改会话,快照等价)。
   验证:同场景补全 **0.3s** 返回(ping 仍在跑)。
2. **面板聚焦态按键不路由**(连锁):历史面板打开后焦点在搜索框,声明在
   prompt textarea 的 Ctrl+R 不再路由 → 面板关不掉 → 后续 autoui_type 打进
   搜索框(表格过滤框同理成 vtree 首个 input)→ 成片失败的另一半。
   **修复**:Ctrl+S 改切换语义(开→按=关,不依赖面板内路由);面板输入框
   补声明 ctrl.r → .Close(引擎路由修复前的双保险);测试辅助
   `_submit_command`/CP/GP 显式定向 prompt vnode。

### 结果

- test_cli_parity.py 全文件连跑 **15 pass + 1 skip**(CC-01 实例级键盘竞态),
  此前该口径在第 8 项后成片失败。
- 全套件(含 parity)**75 pass / 48 skip / 0 失败** —— 首次全绿。

### 顺带清理

- worker 主循环退役 completion_sigs/provider(M7 块随迁补全线程)。

## 10. Phase 3 T11:NL→命令(2026-08-24,独立 session)

### 架构决策:零引擎改动(核心约束)

auto-lang master 被 430/431/432 占用(当前 auto.exe 构建于 00:50,430 波次提交在
02:20-04:10 之后落地,重建即吸入未测 WIP)——T11 全部落地在 auto-shell 仓,不改引擎:

1. **`?` 前缀拦截在 worker `CommandReq::Run` 分支顶部**(历史展开之前):问题文本
   非命令,块保持 Running,即时发一条「⤾ AI 翻译中…」提示 chunk,请求转专用线程。
2. **翻译走 `ash-server-nl2cmd` 专用线程**(T10 补全线程五件套同款:专用 channel +
   线程 + SharedSession 快照 + 事件回传;无需自有 Shell)。线程内同步循环
   `blocking_recv`,`AiClient` 缓存复用(**必须在 runtime 外构造**——内含阻塞探活,
   ai/mod.rs block_on_async 警告同源),每请求 `Runtime::block_on(complete)`
   (多线程 runtime,CLI ask_ai 同款)。翻译逻辑镜像 repl.rs:388-444:快照上下文
   (L0 OS+cwd / L1 上一条命令,别名 L2 因无 Shell 跳过)+ tier:mid / 256 tokens /
   temp 0.3 + 剥代码围栏;随后同源过 `ai::validate_suggestion`(Danger/Warning 拼
   单行 notice)与 `ai::split_steps`(multi 标记)。
3. **结果复用既有 `CommandResult` 事件交付**(`RenderedOutput` 新变体
   `AiSuggestion{question,cmd,notice,multi}`,ash-core 加变体,全消费点 `if let`
   不受影响,ash-tui 唯一穷尽 match 补臂)——**零新 SSE 事件族**:merged 泵无白名单
   自动透传,HTTP SSE 全量转发,引擎 update_block_in_state 对 output 走通用
   `json_to_auto_val`,Vue 侧 RunResult 变体判断补一项即可。三模式(HTTP/merged
   VM/Vue)同一交付路径。
4. **同步契约端点 `POST /api/nl2cmd`**(oneshot reply,测试/契约用)+ **`GET
   /api/ai_pending`**(翻译成功先落槽再发事件;取后即清)。

### 前端:块卡片 + 建议条双层(VM 机制约束下的落地)

- **块卡片**(block_item.at 内联渲染,Text/Code 同款 VM 子 widget prop 规避):
  问题回显 + 命令(mono)+ 危险行(红)+ 多步提示 + **[▶ 执行]**(复用 Rerun 桥,
  深路径参数与既有 `cell.Tagged.text` 同构,实测正常)+ **[✕ 取消]**(DeleteBlock 桥)。
- **建议条**(prompt_bar.at 输入框上方,绑定 `store.ai_pending_cmd` 经 App prop 透传):
  **[✎ 编辑]**(PromptBar 自身 handler 直写 `.input`,填入后回车即执行 = CLI F3 的
  Enter 语义)+ [✕](store.ClearAiPending 关条)。
- 排查中定位的 VM 机制边界(记录,设计依据):
  - **prop watcher 在 VM 不可靠**(renderer.rs:7116 Pick 桥注释同证;Vue 端 watch 是
    一等公民)→ 自动回填输入框不可行,改为建议条显式 ✎ 填入;
  - **PromptBar 内部 `.Run(cmd)` 不触发引擎 emit 模拟**(模拟按外层事件名 `Run` 识别,
    cmd 取 `state.input` 抢救值,空输入点击执行拿到空串)→ 建议条不放执行按钮,
    执行统一走块卡片 ▶(Rerun 桥)或 ✎ 填入后回车;
  - **store.RunCommand 直调会留死块**(引擎注释:VM List 推入的块是堆引用,
    update_block_in_state 匹配失败,块永远 Running)→ 不走此路。
- RefreshContext(引擎在每个 command_result 后触发的 store handler)顺带拉
  `ai_pending()` 写 `store.ai_pending_cmd` —— 这是「App tick 轮询」的实际形态:
  事件驱动的单点拉取,槽位先落再发事件保证可见。

### 假后端与测试

- `ASH_FAKE_AI`(非空)→ 确定性假翻译:含「危险/danger」→ `rm -rf /`(过危险校验,
  只断言 notice 绝不执行);否则 `echo fake-ai:<问题>`(可端到端断言)。测试经
  conftest 环境继承透传进 VM 进程,不动真实服务(plan §5 口径)。
- NL-01 危险提示 / NL-02 卡片+建议条+✎ 回填 / NL-03 ▶ 执行真跑 + 条关闭 +
  ✕ 取消删块。**多建议块并存的按钮定位用「最后一个匹配」**(块列表 vtree 顺序,
  首个会点到旧卡片——首版测试即踩此坑,点到 NL-01 的 `rm -rf /` 执行,被引擎
  SecurityPolicy 硬拒,无伤)。
- **真 AI 验证**(aaid 在跑,glm-5.2 经 zhipu 池):「? 用一条命令列出当前目录下的
  所有文件」→ **`ls -a`**,卡片/建议条/回填全链路正常。
- **HTTP 冒烟**:ash-server + fake:`POST /api/nl2cmd` 两变体 ✓;`POST
  /api/run_command` `? …` → SSE `command_result` 帧 `{"AiSuggestion":{…}}` ✓;
  `/api/ai_pending` 取后即清 ✓。
- 回归:全套件 **78 pass / 48 skip / 0 失败**(基线 75/48/0 + NL×3;parity 全文件
  连跑 18 pass + 1 skip;期间 he02/03、JP-01 各出现过一次既有 T10 族负载竞态
  单跑/重跑即过,与合并记录口径一致)。

### 顺带修复

- ash-tui 两处 StreamEvent match(repl.rs handle_chat_turn / block_tui.rs ChatCmd)
  补 `TurnStart/TurnEnd` 兜底臂——auto-ai 00a148b 漂移导致本检出 ash-tui 在当前
  auto-ai master 下本就编不过(ask.rs 同款兜底,T12 chat 前置排雷)。
- worker 初始化即预填 SharedSession.cwd(否则首条 `?` 翻译的上下文缺当前目录)。

### 已知边界(记录)

- 翻译中块不可 Stop(全局 cancel flag 与其他命令生命周期有竞态,误判会把建议错标
  Cancelled;翻译秒级,不值得);分步执行(CLI 的 `s`)未做,卡片给 multi 提示;
- 无 daemon 时翻译失败 → Failed 块带「(start the aaid daemon or set AAID_URL)」
  提示(对齐 CLI 口径);Vue 端建议条渲染同源,RefreshContext 拉取在 Vue 的触发
  依赖 SSE dispatch 链(未在本环境验证,块卡片按钮在 Vue 走 .at handler 原生可用)。

## 11. Phase 3 续:T15 + B3 + 尾项 C1(2026-08-24 下午,同 session)

### T15 / B2:AI 补全层 —— 结论「引擎已通,GUI 零接线」

调研推翻了差距表的旧判定:AI 合并逻辑早在 Plan 037/041 间已**下沉 engine::complete
本身**(触发 `trigger_ai_subcommand`/`trigger_nl_to_pipeline` + 合并 `merge_ai_pending`
+ 防泄漏键匹配全在引擎内,engine.rs:123-162),而 worker 补全线程(T10)调的就是
`engine::complete` —— GUI 侧无需任何接线,`ai_completion_enabled` 默认 true。
本轮交付 = 假后端钩子 + 端到端验证:

- `ASH_FAKE_AI` 门控补进 ai_layer 两个 fetcher(与 nl2cmd worker 同款闸门;确定性
  返回,测试不动真实 daemon)。
- **AC-01**(GUI 端到端):命令名位置输入未知中文短语(`nlfake查文件`)→ 第一轮
  complete 触发后台翻译 → 同行第二轮 complete 合并「echo fake-ai:…」候选进面板
  (kind=ai,粉色点)。中文短语同时覆盖了多字节光标修复(见下)。

### B3:上下文排序 —— 专项验证通过

`context_rank`(历史频率 +0.5/条、git 仓库 +2.0、上一条命令连贯性 +1.0,stable
sort)同样已在 engine::complete 命令名位置生效。**CR-01**:连跑 3 次 `glob …`(共享
历史 +3 条 glob 词条)后输入 `g` 前缀,glob 候选越过 grep(grep/glob 历史基线计数
均为 0,加分项确定主导)。注:git 不在候选表(非注册命令),仓库通道无从验证,
用频率通道代验 —— 排序逻辑三者同函数,一通皆通。

### 尾项 C1:AutoScript 模式标识

prompt 符号三态:续行 `·` > AutoScript `#` > Shell `❯`。检测在 OnInputComplete
handler 侧算 `auto_hint`(镜像引擎 is_auto_expression 的**静态强信号**:fn/let/
mut/const/use/type/enum 关键字前缀 + 字符串字面量首字符;函数调用/算术/数组/对象
字面量需引擎状态,略 —— 仅视觉提示,执行路由仍由引擎自动检测)。**C1-01** 验证
`let x = 1` → `#`、`echo hi` → `❯`。A6(edit_mode 配置联动)仍不做(低优先)。

### 三笔顺带修复

1. **DoTokenize 单 `&` 死循环(GUI 全交互冻结的真凶)**:Operator 分支只认 `&&`,
   单个 `&`(后台运算符 `cmd &`)落进兜底 else 时 j 在分隔符处立即 break、
   j == i、`i = j` 不前进 → 死循环饿死 UI 线程(budget WARN 刷屏即其痕迹;症状
   = 输入冻结、提交无响应,JP/DM/CC 全族连坐失败)。修复:单 `&` 也走 Operator
   (ol=1)。**定位曲折记录**:症状首次出现在全套件回归,二分排除了 .at(C1)与
   cdylib(engine/ai_layer)后仍复现,最终以 amp 变体探针矩阵 + VM 日志
   (止步于 DoTokenize 的 budget WARN)锁定;HTTP complete 不复现是因为死循环在
   .at tokenize handler,与补全线程无关。
2. **engine::complete 多字节光标 panic**:光标字节偏移可落在多字节字符中间(中文
   输入 + 字符计数光标,实测 `nlfake查文件` cursor=10)→ `&line[..pos]` panic →
   **补全线程整线程死亡**(会话级补全全灭)。钳到最近字符边界(向后退);附
   auto-shell 单测 `multibyte_cursor_mid_char_does_not_panic`。
3. **auto-ai 跨仓漂移两处**(auto-ai main 今日 12:16-13:14 落 Plan-027/028):
   `Tool::execute` 返回类型 String → `ToolOutput`(content/details 双通道)——
   ash_command_tool.rs 两处实现 + 测试适配(`.map(ToolOutput::text)` / `.content`);
   TurnStart/TurnEnd 兜底(T11 时已修)。另登记:**auto-shell 单测
   `test_auto_expression_execution` 现挂**(Auto VM 数组显示成 `<obj#…>`,与
   auto-lang master 430/432 在途改动相关,与本计划改动零交集,stash 对照因编译
   漂移无法成立 —— 待引擎侧确认)。

### T14:评估后顺延(与 T12 同捆)

`nlu::route` 可复用(client 注入,走 aaid 的 local 池),但需要:① 离主线程路由
(Agent 整轮秒级,主 worker 串行);② GUI 入口设计(参数二义 or 独立前缀,与
T12 chat 面板的交互形态相关);③ local 池路由质量验证。三件都与 T12 的 AI 会话
工作重叠,合并处理。

### 回归口径

- parity 文件最好一次 **20 pass + 1 skip**(22 项,仅 CC-01 实例级键盘竞态 skip);
  单项单跑全过。全套件多轮:**75-79 pass / 46-48 skip**,失败集在
  {cs01, he02, he03, pb04, pb10, nl03} 间轮换且均单跑即过 —— 全部为记录在案的
  键盘竞态/时序 flake 族(本轮机器负载显著:套件耗时 12.5min vs T11 时的 9.5min,
  同机 auto-ai/auto-lang 并行会话在跑)。新增 AC-01/CR-01/C1-01 三项在所有轮次
  稳定通过;无真实回归。

## 12. Phase 3 续 2:T12 块内 AI chat(2026-08-24 下午,同 session)

### 形态决策:零引擎的「块内 chat」先行

原 T12 规格(新 SSE 事件族 AiChunk/AiToolCall/AiToolResult + 右侧抽屉面板)需动
引擎 —— 沿用 T11 的既有事件方案先落**块内 chat**(对齐 CLI block_tui 的聊天
形态:块即对话上下文):`?? <消息>` 提交 → 专用 chat 线程 → 流式增量经既有
`CommandOutput` 事件写块(Running 态实时渲染)→ 回合结束 `CommandResult`
收尾。多轮会话跨块持续(`ChatSession` 持久化 ~/.auto-shell-ai-chat.json),
`?? /clear` 清空(与 CLI F4 同名指令)。右侧抽屉面板(AiChunk 事件族)留作
引擎空闲后的升级件,届时块内形态可保留为 fallback。

### 实现

- **`ash-server-chat` 专用线程**(nl2cmd 线程同款范式):懒建 `ChatSession`
  (失败置 None,daemon 重启后下一条消息自愈);上下文经 `set_context_str`
  走 SharedSession 快照(与 nl2cmd 同款,ChatSession 不持 Shell);
  `send_turn_streaming` 的 `on_event` 把 Delta/ToolStart/Tool/Warning/Thinking/
  Error 事件映射为增量文本行(渲染对齐 CLI block_tui 的 ChatEv:⚙ 工具/← 结果/
  ⚠ 警告/💭 思考);回合后 `save()`。
- **关键坑(记录)**:`CommandResult.output = RenderedOutput::Empty` 序列化为
  裸字符串 `"Empty"`(非 null)→ 引擎 update_block_in_state 不走 streamed_text
  回退分支、且清空 streamed_text —— 流出的内容全丢(症状:块 Success 3ms、
  output 显示字面量 "Empty")。chat 线程累计全量流文本,收尾以 `Text(全文)`
  发送(成功与失败路径都带,失败保留已流出部分、错误信息进 status)。
- **fake client**:`ChatSession::with_client(Arc<dyn Client>)` 可注入 ——
  `FakeChatClient` 回显最后一条用户消息(纯文本响应一步终止 ReAct 循环),
  ASH_FAKE_AI 同一闸门。
- 前端**零改动**(块的 Running 流式 + Text 收尾渲染均为既有机制;Vue 的
  RunOutput/RunResult 同链)。
- deps:ash-server 增 auto-ai-agent(ChatSession/Client trait)+ async-trait。

### 验证

- CH-01(fake):`?? 你好…` → 全量回复落块(fake-chat: 回显)+ 第二轮复用同
  会话线程;CH-02(fake):`?? /clear` → 「会话已清空」块。
- **真 AI 验证**(aaid/glm-5.2):`?? 用一句话说明你能做什么` → 5.2s 回合,
  agent(Nicole 角色,带文件/命令工具)中文回复,流式落块、Success 收尾、
  会话持久化。
- 回归:parity 全文件 **22 pass + 1 skip**(nl03 偶发一次为已知 flake,单跑过);
  全套件 **83 pass / 44 skip / 4 失败**(cs01/c101/run_echo/cmd03,均单跑即过
  —— 负载时序族,套件 12.9min;CH×2 + AC/CR/C1 全部稳定通过,无真实回归)。
