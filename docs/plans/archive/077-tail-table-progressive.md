# 077 — 表格渐进渲染(071 §6 E3,方向储备收官)

- 日期:2026-08-28
- 状态:**✅ COMPLETE(M1-M3 全落地,2026-08-28;071 §6 方向储备至此收官)**
- 实施记录(2026-08-28):
  - **M1**:ShellContext trait 增 `emit_tail`(默认 no-op 零破坏),Shell
    实现转发 live 通道;通道字段更名 external_tail_tx→live_tail_tx、
    setter→set_live_tail(外部分支与结构化命令共用);find 插桩 ——
    find_recursive 增 emit 参数,命中入列处发射显示路径,run() 闭包桥接。
  - **M2**:tail_cmd.rs 增 TailKind(External|Structured)+
    INSTRUMENTED_COMMANDS=["find"]+is_instrumented;repl.rs should_tail
    返回 Option<TailKind>,try_execute_tail 按 kind 差异化 —— Structured
    不擦命令行(表格上方保留命令,同今天)、冻结直印(print_command_output
    原样,不套 E5 摘要);lease/渲染线程/通道包夹/退出码与 E2 完全共用。
  - **M3**:回归 —— find 新增发射测试(通道收命中路径+冻结输出不变)、
    is_instrumented 门测试;auto-shell 702 过 2 挂(在册预存)、ash 130、
    `ash -c "find ... -name ..."` 表格冒烟正常。实施中一次字符串替换误伤
    display_path 生产行(已精确恢复,git diff 复核仅剩插桩行)。
- 来源:071 §6 最后一项(用户"继续"放行)。前置:073 租约 / 074 E2 管线。
- 目标形态:长跑结构化命令(v1:`find`)扫描期间,尾部动态区显示
  `⏳ find · 耗时 · N 行` + 最近命中路径预览;完成后冻结为**完整表格**
  (与今天逐字节一致 —— 表格是漂亮成品,不套 E5 摘要)。
- **关键简化(2026-08-28 核验)**:`format_output` 经 RenderHook 把表格转
  ANSI **字符串返回**,真正打印在 REPL 层 —— 执行期间无任何终端输出,
  无需捕获/延迟渲染机制,E3 几乎复用 E2 的租约管线。

## 1. M1 — engine:通用渐进发射通道

- `ShellContext` trait 新增 `fn emit_tail(&mut self, _line: &str) {}`
  (默认 no-op,零破坏);`Shell` 实现发送到 live 通道。
- 通道字段更名 `external_tail_tx → live_tail_tx`、`set_external_tail →
  set_live_tail`(外部分支与结构化命令共用同一通道;074 一日龄内更名
  无兼容负担)。
- 插桩 `find`(v1 唯一消费者):`find_recursive` 增 `emit: &mut dyn
  FnMut(&str)` 参数,命中入列处发射显示路径;`run()` 以闭包桥接
  `shell.emit_tail`。grep/du 等后续按同一行模式补(明确不做节)。

## 2. M2 — frontend:结构化分支

- tail_cmd.rs:`INSTRUMENTED_COMMANDS = ["find"]` + `is_instrumented()`。
- repl.rs:`should_tail` 返回 `Option<TailKind>`(External | Structured);
  `try_execute_tail(kind)` 两处差异 —— **不擦命令行**(命令行留在表格
  上方,同今天)、**冻结直印**(print_command_output 原样,不套 E5 摘要);
  其余(lease/渲染线程/按键排空/通道包夹/退出码)与 E2 完全共用。

## 3. M3 — 回归

- 单测:find 发射(temp 目录建文件 → 通道收到路径);is_instrumented;
  既有全量不回归;`ash -c "find"` 输出不变(非交互无租约)。

## 4. 明确不做

- grep/du 插桩(机制就绪后按需各加一行发射);表格超限摘要(E5 不套
  表格,溢出场景待真实投诉);表格行的结构化预览(视口只显路径文本);
- run_atom_streaming 路径(管道输入场景,闸已挡)。
