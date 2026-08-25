# 068 — 统一 agent:`?` 唯一 AI 入口 + 命令审批门

- 日期:2026-08-25
- 状态:**Phase 1(GUI)已实施,验收通过**(plan-068 worktree 分支;Phase 2 CLI 另期)
- 实施记录(2026-08-25 深夜,Phase 1):
  - **crate**:`is_readonly_command` 白名单(纯读命令表;git 整体保守走
    提案侧)+ `ProposeTool`(与 AshCommandTool 同名注册,execute 不执行、
    命令串送 proposal 通道,返回 agent「已提交审批」)+ ChatSession
    `with_client_and_proposals`(sink 存字段,clear() 重建复用;None=
    CLI 旧行为全直exec)。
  - **worker**:`?` 与 `??` 同走 chat worker(`?` 不再一次性翻译);nl
    线程/NlReq/nl2cmd 端点(HTTP+bridge+api.at+手写 api.ts)整体退役;
    chat 线程持 proposal 通道,on_event 与回合收尾双 drain —— 每条提案
    写 AI_PENDING 槽(RefreshContext 拉 → 建议条 ✎/✕)+ 发 ai_chunk
    抽屉行「📋 建议命令: cmd」;抽屉自动开改判单 `?` 前缀。
  - **测试**:ST-01..03(翻译多步卡)删除;NL-01..03 重写为统一 agent 流
    (直答/建议条+✎ 填入/审批门不自动执行);SP-01..02 新增(提案行+
    建议条 + `?` 直达);Fake 加 propose 旋钮(build --check —— 注册表内
    非只读命令;git 等外部命令不在 agent 工具表)。
  - **验收**:两批回归 29+19 过(he02/03 键盘 flake 在册豁免;BS-03/
    NL-02 复跑绿 —— 前者长会话劣化,后者测试内旧串残留);vue-tsc 0 错 +
    build 绿(引擎 oninput 根治 auto-lang 4c9dc5516 已生效,无需手补)。
  - **连带销案**:DEBTS 引擎清单#6(oninput codegen)已由并行会话在
    auto-lang 4c9dc5516 根治。
- 决策(用户裁定,2026-08-25):去掉 `??`,`?` 作为唯一 AI 模式入口。
  形态选**统一 agent**:`? <一句话>` 全部进对话 agent;agent 想执行命令时
  **产建议卡等审批**(工具级审批门),对话与执行完全统一。066/067 已把
  smart 从三端撤除,本计划完成 AI 入口的最终收敛:整个产品只有
  **普通命令模式**与 **AI 模式(`?`)** 两种。

## 0. 安全模型(本设计的核心)

现状 `??`(F4)的 agent 对**所有**命令全自动执行 —— 自主性最高。统一后
反而收紧:**非只读命令 100% 走审批门**,不再有"agent 直接 rm"的路径。

**工具分级(register_ash_tools 改造):**
- **只读白名单**(纯读命令):保持现状自主执行(⚙/← 实时流),agent 的
  探索能力(看目录/读文件/查 git)完整保留。
- **其余一切命令 → 同名提案工具(ProposeTool)**:agent 调用时**不执行**,
  经 proposal 通道产建议卡;工具返回"已生成建议卡,等待用户执行",agent
  可继续总结收尾。同名注册的意义:agent 的调用习惯与工具清单不变,只有
  执行语义变了。

**审批后闭环:**用户点建议卡 [▶ 执行] → 走普通 RunCommand → 执行结果经
nl_context 快照(last_command/last_exit,既有机制)回流 → 下一轮 `?`
agent 可见,多轮协作闭环("现在把它们删掉"类追问可用)。

## 1. Phase 1(GUI,零引擎)

- **入口**:worker Run 分派 —— `?` 与 `??`(兼容)都进 chat worker;
  nl 翻译线程(spawn_nl_worker)退役,`?` 不再走一次性翻译。
- **proposal 通道(零引擎)**:
  - 抽屉行:proposal 发生时 chat worker 经既有 `ai_chunk` 事件送一行
    `📋 建议命令: <cmd>`(renderer/vue 两轨零改动);
  - 建议条:proposal 写 `AI_PENDING` 槽(062 T11 既有),回合收尾
    RefreshContext 拉取 → PromptBar 建议条(✎ 编辑/[▶] 执行)复用。
  - 多 proposal:v1 单活跃(后写覆盖,同 063 steps 单份边角)。
- **is_readonly(name)**:白名单判定新件(名字前缀/完全匹配表),借鉴
  validate_suggestion 的模式法;白名单 = ls/cat/head/tail/wc/file/which/
  type/pwd/echo/date/env/git status|log|diff|show|branch|remote|rev-parse
  等纯读集合。边角(构建/测试类写盘命令暂入提案侧)记 DEBTS。
- **建议上下文**:ProposeTool 返回文本告知 agent"命令已提交审批,用户
  执行后下一轮可见结果",agent 提示词补一句引导(提案后给用户操作指引)。
- **测试**:ST-01..03 重写为 agent proposal 流(fake client 造 tool_use);
  CD 族 `?`/`??` 双入口;SN(suggest-next)不动 —— 独立通道;
  CH-01/02(`??` 块 chat)兼容不破。

## 2. Phase 2(CLI,另期)

- F4 chat 循环成为唯一 AI 模式;F3 的 ask_ai 一次性翻译退役。
- proposal 在 TUI 渲染为 F3 既有审批卡([Enter]执行/[s]分步/[e]编辑/
  [Esc]取消)—— F3 的 UI 复用、F4 的 agent 内核,两端收敛完成。
- repl_mode 的 AI 模式(`?` 提示符)语义同步。

## 3. 明确不做

- **回合中途挂起等审批**(human-in-the-loop 暂停 agent 循环):需 auto-ai
  引擎支持挂起恢复,v1 用"提案即收尾/下一轮回流"近似,够用且零引擎。
- **白名单配置化**(用户自定义只读集):v1 硬编码表,有真实需求再开。
- suggest-next / AI 补全层:独立通道,不在本计划射程。
