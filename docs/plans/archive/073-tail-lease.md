# 073 — 尾部租约:线性输出+尾部动态的统一机制(071 §6 E1)

- 日期:2026-08-27
- 状态:**✅ COMPLETE(M1-M3 全落地,2026-08-27)**
- 实施记录(2026-08-27):
  - **M1** `frontend/tail.rs` 新模块:TailLease(erase_inline_row/acquire/
    draw/begin_freeze/terminal + Drop 恢复 cooked,panic 安全;Frame 路径为
    本版 ratatui-core 的 `terminal::Frame`)+ TailBuffer(keep-last-N 有界
    缓冲 + dropped/total 计数 + header_note 摘要头,capacity 0 收敛为 1)。
  - **M2** 070 编辑器迁移:删 `editor_overlay/term.rs`(TerminalGuard/
    exit_modal 职责并入 TailLease);run_editor_inner 改 erase→acquire→
    loop{draw}→begin_freeze,lease 随作用域 Drop;view.rs 的终端类型改
    `tail::TailTerminal`;键路由/回显/硬件光标逻辑零改动。
  - **M3** 回归:workspace 编译绿;ash lib 114 单测全绿(108 既有 + 6 个
    TailBuffer 新测);`ash -c`/管道冒烟正常;编辑器模态真终端交互保持
    既有单测覆盖(键路由/回显渲染),键位冒烟留日常使用验证。
- 来源:071 §6 方向储备首项(用户放行"开始实施这个计划")。E1 是 E2-E4 的
  公共地基 —— 先把"占用尾部 → 动态渲染 → 冻结回线性 → 释放"抽象成统一机制,
  并以 070 编辑器模态为存量消费者验证抽象成立。
- 核心不变式(071 §6):线性转录永远固定、累计、可原生复制;尾部动态区只在
  "进行中"存在,完成即冻结回归线性;不进 alternate screen、不捕获鼠标。

## 0. 现状与抽象点

两个孤立先例(生命周期代码互不复用):

| 先例 | 占用 | 动态 | 冻结 | 释放 |
|---|---|---|---|---|
| 070 编辑器(editor_overlay/term.rs) | erase 内联行 + raw + Inline(H=12) | textarea 事件循环 | exit_modal 清视口行,调用方 println 回显 | Drop 恢复 cooked |
| 038 subprocess.rs | 拆卸 ratatui | (外部进程自有终端) | (外部程序自打) | 重建 |

共性 = **租约四段**:`acquire(占尾) → draw(动态帧) → begin_freeze(清视口) → Drop(释放)`。
差异在冻结内容(回显框/流式日志/表格),归消费者。

## 1. M1 — `frontend/tail.rs` 新模块

- **TailLease**:`erase_inline_row()`(静态,reedline 提交行擦除)、
  `acquire(height)`(raw + Inline viewport + RawGuard RAII,panic 安全)、
  `draw(f)`(一帧,返回绘制区 Rect 供光标定位)、`begin_freeze()`(光标移视口
  顶、Clear FromCursorDown —— 冻结内容随后由消费者在 cooked 模式线性打印)、
  `terminal()` 访问器。Drop = disable_raw_mode。
- **TailBuffer**(纯逻辑,E2 的数据结构先行):有界行缓冲,keep-last-N +
  溢出计数(dropped/total),`header_note()` 生成"…已滚动 K 行(共 N)"摘要头。
  单测覆盖:未满/满/溢出计数/空/单行。

## 2. M2 — 070 编辑器迁移(行为保持)

- 删 `editor_overlay/term.rs`(TerminalGuard/exit_modal 职责并入 TailLease);
  view.rs 的 `EditorTerminal` 改 `crate::frontend::tail::TailTerminal`;
  run_editor_inner 改走 `TailLease::erase_inline_row → acquire → loop{draw} →
  begin_freeze`。**纯机械等价,无行为变更**(键路由/回显/硬件光标不动)。

## 3. M3 — 回归与收尾

- 构建 + ash lib 单测全绿(070 既有键路由测试不动即绿);TailBuffer 新单测;
  `ash -c` 冒烟;编辑器模态真终端交互无法无头自动化 —— 依赖既有单测 + 人工
  冒烟(记入验收注)。

## 4. 明确不做

- E2(运行中命令动态块)/E3/E4 的消费者接线 —— 各自另立计划(074+)。
- 用 insert_before 实现冻结 —— 070 v1 已验证"清视口 + 调用方 println"语义
  等价且更简单;insert_before 路线留给 E2 实测需要时再评估。
- reedline 常驻接管(D4 既定:reedline 是单行输入引擎,不动)。
