# 075 — AI 回合动态渲染(071 §6 E4):对话流式尾部重绘 + 回合冻结

- 日期:2026-08-28
- 状态:**✅ COMPLETE(M1-M3 全落地,2026-08-28)**
- 实施记录(2026-08-28):
  - **M1**:新 `tail_chat.rs` —— TurnTailState(到达序行累积:完整行
    Vec<(text,kind)> + partial 未完行 + chars 计数;push_delta 按 
 切分、
    push_line 先 flush partial 保交错序、frozen_lines 消费式取走)+
    LineKind(Reply/Tool/Error → ratatui Style 与 ANSI 前缀双映射)+
    draw_chat_frame(状态行 `⏳ AI · N 字 · Ts` + visible_tail(8),超宽行
    截断)+ print_frozen_turn;7 单测(交错序/partial 合并/flush 语义/边界)。
  - **M2**:handle_chat_turn 重构 —— sink 双态(Some(state)=Tail / None=Live
    逐字打印回退);**不擦输入行**(问题留转录,冻结块只含回复);lease →
    into_parts → 渲染线程(done 旗标 + 80ms 帧 + 按键排空,退出前
    freeze_viewport);block_on_async 返回 → 置位 → join → drop guard 回
    cooked → frozen 打印 → 传输错误提示(移到冻结块之后)→ save →
    072 审批门照旧。闸:ASH_NO_TAIL=1 / lease 失败 → Live。
  - **M3**:回归 —— ash lib 124(+7)、auto-shell 701 过 2 挂(在册预存
    不变)、`ash -c` 冒烟正常;chat 流式真终端观感留日常人工验证。
- 来源:071 §6 方向储备 E4(用户"继续"放行)。地基:073 尾部租约 + 074 渲染
  线程先例。**建议顺序中的下一项**(AI 回合是最高频的"进行中"内容)。
- 目标形态:CLI chat(`?` 模式)一个回合期间,流式回复与工具事件在尾部动态
  视口**重绘**(状态行 + 最近几行,滚动预览);回合结束整体**冻结**落线性
  转录(到达序保持:工具行/回复文本交错)。用户输入行**不擦除** —— 聊天的
  问题要留在转录里(区别于 E2 擦除命令行,因为 E2 冻结块头会复述命令,而
  chat 的冻结块只含回复)。

## 0. 现状(2026-08-28 核验)

`Repl::handle_chat_turn`:on_event 回调直接 print!/println!(Delta 逐字
线性追加、工具行带前导换行);回合收尾仅补一个换行 + save。回调在 REPL
线程上被 `block_on_async(send_turn_streaming)` 驱动 —— 与 E2 同构:REPL
线程阻塞、事件经回调到达、渲染交给独立线程。

## 1. M1 — 回合状态(纯逻辑,新 `tail_chat.rs`)

- `TurnTailState`:`lines: Vec<(String /*text*/, Style4)>` 完整行 +
  `partial: String` 当前未完行 + `chars: usize`;`push_delta(str)`(按 \n
  切分推进完整行/累积 partial)、`push_line(text, kind)`(先 flush partial
  再入列)、`flush_partial()`、`visible_tail(n)` 取尾部渲染行、
  `frozen_lines()` 冻结打印序。kind ∈ {Reply, Tool, Warn, Error} ——
  视口渲染映射 ratatui Style,冻结打印映射 ANSI 前缀。单测:交错序保持、
  partial 跨 delta 合并、flush 语义、空回合。

## 2. M2 — REPL 接线

- `handle_chat_turn` 重构:构造 **sink**(`Live` = 今天的行为,回退用;
  `Tail(state)` = 推入状态);on_event 统一格式化到 sink(Delta→delta,
  Tool/Warning/Error→line,Done/Thinking/Cancelled 维持现语义 —— Cancelled
  作为 line)。
- 尾部模式:不擦输入行,`TailLease::acquire(CHAT_TAIL_HEIGHT=10)` →
  `into_parts` → 渲染线程(80ms 循环 + done 标志 Arc<AtomicBool>;帧 =
  状态行 `⏳ AI · N 字 · Ts` + visible_tail(8);**不排空按键**(chat 无
  子进程,但为防泄漏同 E2 处理));`block_on_async` 返回 → done 置位 →
  join(渲染线程退出前 freeze_viewport)→ drop guard → 冻结打印
  frozen_lines()(工具行带 ANSI 暗色,回复原样)→ save。
- 闸:非 tty / `ASH_NO_TAIL=1` / lease 失败 → Live 回退,行为与今天逐字
  一致。Err(网络等)路径:eprintln 提示照旧(在冻结之后)。

## 3. M3 — 回归与记录

- 单测:TurnTailState 全套;既有 chat/编辑器/审批门测试不回归;`ash -c`
  不受影响(非交互无 chat)。真终端 chat 流式观感留日常人工验证(在册
  限制:markdown 渐进着色不做,纯文本 v1)。

## 4. 明确不做

- markdown/syntect 渐进着色(视口纯文本;着色管线已有,接入待真需要)。
- `ash ask` 的流式改造(独立形态,低频;后续顺手)。
- 思考(Thinking)事件展示(今天也忽略;显示需折叠交互,另议)。
- 回合中取消(CLI chat 无取消通道,与 E2 同源的既有 DEBT)。
