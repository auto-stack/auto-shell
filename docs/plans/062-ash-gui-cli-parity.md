# 062 — ash-gui(Auto/vue·VM)与 CLI 版功能差距补全

- 日期:2026-08-23
- 状态:**Phase 1 完成并全量验证**(8 项新回归全过 + 全套件 71 pass/45 skip/0 fail,
  2026-08-23 深夜);Phase 2/3 未开工。实施记录见 §7
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

**合并提醒**:两笔引擎修复在 auto-lang worktree `.worktrees/auto-lang-p062`
(分支 `ash-debug-062`),**未合 master**(master 由 Plan 419/436 agent 实时占
用);`.worktrees/auto-lang` junction 现指向该 worktree,合并 master 后应指回
`D:\autostack\auto-lang` 并重编主检出 auto.exe(plan-060 R16 同款流程)。

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
