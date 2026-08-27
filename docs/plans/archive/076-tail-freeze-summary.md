# 076 — 长输出摘要冻结(071 §6 E5):超限输出头尾摘要 + 落文件

- 日期:2026-08-28
- 状态:**✅ COMPLETE(M1-M3 全落地,2026-08-28;临时文件留存入 DEBTS)**
- 实施记录(2026-08-28):
  - **M1**:tail_cmd.rs 扩展 —— FreezeText(Full/Summary{excerpt, 行/字节
    统计})+ build_freeze_text(阈值 DEFAULT_FREEZE_MAX_LINES=100,
    ASH_TAIL_FREEZE_MAX 覆盖;头 8/尾 24,头尾钳制防 env 小阈值下溢 ——
    env 测试首跑即抓到此真 bug,已修)+ spill_full_output
    (`%TEMP%/ash-freeze-<毫秒>.log`);5 新单测。
  - **M2**:repl.rs try_execute_tail 的 Ok(Some)/Err 两臂统一走
    print_frozen_output —— Full 原样 print_command_output;Summary 打印
    摘要块 + `📄 完整输出(L 行 · B 字节): path` + `查看: show path`
    (spill 失败降级提示);**失败路径同样分级**(E2 把 stderr 并进 Err
    消息,长构建失败不再一次性倾倒)。snippet/(snippet, exit) 契约不变。
  - **M3**:回归 —— ash lib 129(+5)、auto-shell 701 过 2 挂(在册预存
    不变)、`ash -c` 冒烟正常;临时文件不主动清理入 DEBTS。
- 来源:071 §6 方向储备 E5(用户"继续"放行)。前置:074(E2 冻结全文是
  本计划的基线)。方向储备收官顺序建议 E5(小而实用)→ E3(表格渐进)。
- 目标形态:E2 的冻结从"全文"升级为分级 —— **阈值内**(默认 ≤100 行)原样
  冻结(与今天逐字节一致);**超限**冻结"头 8 行 + …省略 N 行… + 尾 24 行 +
  统计行(共 L 行 · B 字节)+ 完整输出临时文件路径(附 `show <path>` 查看
  提示)"。长构建不再淹没转录,全文随取随看。
- 连带修复:E2 把 stderr 并进了 Err 消息,失败路径 eprintln 会一次性倾倒
  全量 stderr(今天继承模式下 stderr 是流式出的)——超限失败同样走摘要
  分级,保持体验对齐。

## 1. M1 — 冻结策略(纯逻辑,扩展 `tail_cmd.rs`)

- 常量:`FREEZE_MAX_LINES=100`(env `ASH_TAIL_FREEZE_MAX` 覆盖)、
  `SUMMARY_HEAD=8`、`SUMMARY_TAIL=24`。
- `enum FreezeText { Full, Summary { excerpt, total_lines, total_bytes } }` +
  `build_freeze_text(output)`(按行数判定;空输出 Full);
  `spill_full_output(output) -> io::Result<PathBuf>`(temp_dir 下
  `ash-freeze-<毫秒>.log`,内容为将打印的原文)。
- 单测:阈值内全文;恰超限摘要结构(头/省略行/尾/统计);空;
  env 覆盖生效;spill 写读回环 + 文件名唯一。

## 2. M2 — repl 冻结打印改造

- `try_execute_tail` 的 Ok(Some)/Err 两臂统一走
  `print_frozen_output(text)`:Full → print_command_output 原样;
  Summary → println 摘要块 + 统计 + `完整输出: {path}` + `查看: show {path}`
  (spill 失败则提示原文未留存)。snippet(前 200 字符)与 (snippet, exit)
  契约不变。
- 不动:Ok(None)、live 回退路径、chat(E4 回合文本通常短,超限场景是命令
  输出;chat 摘要待真需求)。

## 3. M3 — 回归与记录

- 全量:ash lib / auto-shell / ash-core 不回归;`ash -c` 冒烟(非交互路径
  不受影响);临时文件留存策略(不主动清理,依赖系统 temp 清理)入 DEBTS。

## 4. 明确不做

- E3 表格渐进渲染(方向储备最后一项,另立)。
- chat 回合超限摘要、冻结文件数量上限/LRU 清理(有真实堆积投诉再做)。
- 配置文件化阈值(env 已够 v1)。
