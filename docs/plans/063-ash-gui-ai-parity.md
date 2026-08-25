# 063 — ash-gui 与 CLI 版 AI 能力对齐

- 日期:2026-08-25
- 状态:**Phase 1(T1-T3)已实施,VM 端到端验收通过**(plan-063 worktree
  分支;T4/T5 仍需引擎窗口,未动)
- 实施记录(2026-08-25,Phase 1 = T1-T3):
  - **T1 suggest-next**:worker 收尾钩子(`is_enabled` → `suggest_next_async`,
    与 CLI repl 同款 best-effort)+ `/api/ai_next` 端点(drain auto-shell 的
    PENDING 槽,JSON 数组串取后即清)+ App 级「💡 接下来」chips 行(渲染读
    store 字段 —— `[]str` 走 widget prop 在 VM 视图侧读不到,App 级是计划
    原口径)+ `.PickNext` 经 injected_command 注入(Pick 同语义)。config
    注入走 `ASH_SUGGEST_NEXT` env 覆盖(零污染,ASH_FAKE_AI 同款门控惯例)。
  - **T2 分步执行**:nl worker 产 `split_steps` 全量(AiSuggestion 事件带
    steps + `ai_steps` 槽落 `
` 连接 str —— handler 三层深读在 VM 静默中止,
    str 槽是 ai_pending 已验证通道)+ RefreshContext 拆到 store 级
    `ai_steps_list/styles`(引擎边界:renderer 构造的块不含新契约字段)+
    multi 卡片分步行(每步 [▶] + [▶▶ 全部执行] + ✓/灰已执行标记)。
  - **T3 smart NL 路由**:命令行 `smart list | smart run <名> [args] |
    smart <NL>`(与 CLI 同词法,worker 解析)+ 专用路由线程
    (`ash-server-smart-nlu`,route 自带 one-shot runtime 故无需常驻)+
    命中回主循环 `RunSmart{reply: None}` 按名执行(executor 需 !Send Shell),
    未命中 Failed 带可用命令 hint;事件收尾自带 `Text(累计全量)`(Empty
    清空 streamed_text 的 DEBT)。smart 加载改用 **shell 会话 cwd**(
    `load_smart_specs`:VM 宿主 boot 时 chdir 到 src/front,进程 cwd 扫描
    落空)。
  - **验收**:tests/test_ai_parity.py —— 默认轮(SN-03/ST-01..03/SM-01..02)
    6 passed;SN-01/02 需 `ASH_SUGGEST_NEXT=1` 单独一轮(2 passed)。062
    基线(test_cli_parity 全族含 NL/CH/AC)分批回归通过;vue-tsc 0 错 +
    vite build 绿;auto-shell suggest 单测 3 passed。
  - **连带修复**:conftest `_kill` 改 `taskkill /T`(孤儿窗口进程占 9247
    是轮间「MCP connection refused」flake 的主因);FakeChatClient 补
    `model_meta`(auto-ai 上游新增字段,既有破坏)。
  - **新债**:见 DEBTS「Plan 063 新增已知限制」(store 级单份 steps /
    NL 回退仅命令行 / 打灰用 ✓+样式数组近似)。
- 调研对象:
  - **CLI 版** = `ash/ash-tui/src/repl.rs` + `ash/auto-shell/src/{repl_mode,ai/*,smart_command}`
  - **GUI 版** = `ash-gui/ash-gui-auto`(.at 前端)+ `ash-gui/ash-server/worker.rs`
- 上游:plan-062(P1-P3:交互命令/jobs/键位/`?` NL→命令/`??` 块内 chat/AI 补全);
  其 §0 结论"执行语义已同源"在本轮复核依然成立 —— **差距只在 AI 层的三件尾巴**。

## 0. 结论先行

用户问题"ash-gui 里没有 AI 功能吧"——**部分不成立**:plan-062 已落地 AI 主干
(`?` NL→命令、`??` 块内多轮 chat、AI 补全层、AutoScript `#` 提示),全部零引擎、
经 MCP + 真 aaid(glm-5.2)端到端验证过。**真正的差距是三件**:
suggest-next(T13)、smart NL 路由(T14)、NL 建议的分步执行;外加一件引擎侧
升级件(chat 抽屉面板)。另纠正一笔过时认知:**T13 不依赖本机 Ollama** ——
`suggest.rs` 走共享 `AiClient`(aaid daemon 即可,062 已实证在跑),
DEBTS 062-T13 的"依赖 Ollama"口径随本计划修正。

## 1. CLI AI 能力全表 × GUI 现状

| # | CLI 能力(出处) | GUI 现状 | 定性 |
|---|---|---|---|
| A1 | F3 一次性 NL→命令:翻译+危险校验+multi 拆步,**[Enter]执行/[s]分步/[e]编辑/[Esc]取消**(repl.rs:780-880) | `?` 前缀 → 建议条([✎ 编辑]/[✕])+ 块卡片([▶ 执行]/[✕ 取消]);**分步(s)未做**(062 已知边界) | **缺分步** |
| A2 | F4 chat 循环:持久会话(~/.auto-shell-ai-chat.json)、/clear /exit、工具事件流(⚙/←/⚠/💭)、回合恢复横幅(repl.rs:447-610) | `??` 前缀 → 块内 chat:多轮跨块、流式落块、`?? /clear` ✓;/exit 不需要(块模型);**无回合数横幅、回合中不可 Stop、无抽屉面板** | 大体齐 |
| A3 | suggest-next:命令后后台拉"接下来可能想"×3,下个提示符前展示;config `ai.suggest_next: true` opt-in(suggest.rs) | **无**(062 T13 延期,当时误判依赖 Ollama) | **缺** |
| A4 | smart NL 路由:`ash smart run "<nl>"` → nlu::route 本地模型选 smart 命令(smart_command/nlu.rs) | run_smart 仅按名;名字未命中即 Failed(062 T14 延期) | **缺** |
| A5 | AI 补全层(NL→命令名、AI 子命令、后台合并) | engine::complete 内生,worker 补全线程走它(062 T15 验证) | ✓ 齐 |
| A6 | 三模式锁 F1/F2/F3(`>`/`#`/`?`)+ Alt+1/2/3(repl_mode.rs) | 前缀范式(`?`/`??`)+ `#` 静态提示(C1);无模式锁/prompt `?` 态 | 范式等价,体验差 |
| A7 | `ai ask` CLI 子命令(ask.rs) | `??` chat 覆盖(等价形态) | ✓ 齐 |

## 2. 分阶段补全计划

> 实施约束沿用 062:优先零引擎(auto-shell 仓内);AI 功能全部静默降级
> (无 daemon → 提示不报错);测试走 ASH_FAKE_AI 假后端,不动真实服务。

### Phase 1 — 零引擎三件(可立即开工)

- **T1 suggest-next**(中件):
  - worker 侧:command_result 收尾处若 `ai::suggest::is_enabled()` 则调
    `suggest_next_async`(同 crate 直用,PENDING 槽零新代码);新增 api 端点
    `ai_next() str`(JSON 数组串,取后即清 —— 与 ai_pending 同款"槽位先落
    再发事件"模式;多值不能复用单值的 ai_pending)。
  - 前端:RefreshContext 拉 `ai_next` → 输入框上方 chips 行("💡 接下来:" +
    最多 3 个可点命令条);点击 = Pick 同语义填入输入框(store 注入,App 级
    handler 避开 child-callback 债)。
  - 验收:SN-01(config 开 → echo 后 chips 出现,fake)、SN-02(点击填入)、
    SN-03(config 关 → 无请求)。真 aaid 冒烟一轮。
- **T2 分步执行**(小件):
  - nl2cmd 线程已产 multi 标记与 split_steps 结果(062 T11);块卡片在
    multi=true 时按步渲染:每步一行命令 + 独立 [▶](复用 Rerun 桥,零引擎)
    + [▶▶ 全部执行](逐条派发);已执行步打灰。
  - 验收:ST-01(危险多步建议 → 分步渲染)、ST-02(单步执行后其余仍可执行)、
    ST-03(全部执行按序落块)。
- **T3 smart NL 路由**(中件):
  - run_smart 提交侧:名字未命中注册表时转 nl2cmd 线程同款的专用线程调
    `nlu::route`(client 注入走 aaid 池,062 T14 已确认可复用);路由命中 →
    按 smart 命令 spec 执行(现路径),未命中 → Failed 带建议。
  - GUI 入口沿用侧栏 SmartCommands 分区(有 smart 命令注册时自然出现) +
    命令行 `smart run "<nl>"`(与 CLI 同词法,worker 解析)。
  - 验收:SM-01(fake:NL → 命中注册的 smart 命令)、SM-02(未命中 → Failed)。

### Phase 2 — 引擎侧升级件(需 auto-lang 窗口,可后置)

- **T4 chat 抽屉面板**(062 T12 升级件原规格):AiChunk/AiToolCall/AiToolResult
  SSE 事件族 + 右侧抽屉 widget(流式文本 + 工具事件内联 + 会话历史滚动);
  块内形态保留为 fallback。引擎 master 现时空闲(444 已并),可立项;
  前置:auto-lang 侧新增 SSE 事件白名单(renderer/vue 链)。
- **T5 chat 回合可 Stop + 回合数横幅**(小件,随 T4):全局 cancel flag 与命令
  生命周期解耦(062 已知边界:竞态会错标 Cancelled);`??` 块头显示
  "第 N 轮"(ChatSession.turn_count 快照随事件携带)。

### 明确不做(附理由)

- **模式锁(F 键等价)**:前缀范式(`?`/`??`/`#` 自动检测)已达 CLI 三模式的
  功能覆盖,GUI 无 F 键肌肉记忆负担;若后续用户要求,加"AI 模式"开关即可,
  不单独立项。
- **Ollama 本地化**:aaid daemon 已服务全部 AI 通道(062 实证);不引入第二套
  本地推理依赖。
- **`ai ask` 等价命令**:`??` chat 即等价物。

## 3. 风险与对策

| 风险 | 对策 |
|---|---|
| suggest 后台线程与 worker 串行主循环的并发(PENDING Mutex) | 直接复用 crate 既有 PENDING(已线程安全);端点只读镜像 |
| ai_next 多值槽与 VM List 的 VmRef 读问题 | 端点返回 JSON 串,handler 侧 substr 解析或平行数组(062 T7 先例) |
| smart 路由整轮秒级阻塞 | 专用线程(nl2cmd 五件套同款),主 worker 零等待 |
| T4 动 auto-lang(引擎窗口再被占) | T4 独立成期,不阻塞 P1 验收 |

## 4. 验证与回归

- 基线:pytest 94-96 pass / 30-34 skip(057 Phase 5 水位);每任务零新增失败。
- 新用例:`tests/test_ai_parity.py`(SN/ST/SM 族,ASH_FAKE_AI 假后端 +
  config 注入);真 aaid 每相位冒烟一次。
- Vue 侧:Phase 1 全部走 .at + api.at 契约,重生成后 vue-tsc 须保持 0 错
  (444 后的新约束);建议 chips/分步卡片在 Vue 端同构验证。

## 5. 债账修正(随本计划落地)

- DEBTS 062-T13 条目:"依赖本地 Ollama(本机未装)" → 修正为"走共享 AiClient
  (aaid 可服务),opt-in config";T13 本身由本计划 T1 收口后销案。
- DEBTS 062-T14 条目:由本计划 T3 收口后销案。
