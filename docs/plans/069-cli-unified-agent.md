# 069 — CLI 统一 agent:F3 翻译流退役(068 Phase 2)

- 日期:2026-08-26
- 状态:**已实施,验收通过**(plan-069 worktree 分支)
- 决策(用户裁定,2026-08-26):smart 已撤、GUI 已统一(`?` 唯一入口,068),
  CLI 的一次性 NL 翻译(F3/ask_ai + 审批卡)没有存在理由 —— 三端全部
  收敛为「普通命令模式 + AI 模式(对话)」。
- 实施:
  - **repl.rs(reedline 主 REPL)**:F3(`\x13`/Alt+3)分支整体替换为
    `run_chat_loop()`(与 F4 同一入口);ask_ai / run_steps_interactively
    死代码删除。
  - **block_tui.rs(实验块 TUI)**:F3/Alt+3 并入 is_ai_chat_key 判定;
    handle_ai_suggest / ask_ai / NL 审批渲染删除。
  - **保留**:auto-shell crate 的 validate_suggestion / split_steps
    (底件,暂无调用方);`ash ask` 子命令(NL→AutoLang 脚本,独立形态)。
- 验收:cargo build -p ash-tui/-p ash 绿;ash-tui 138 单测过;auto-shell
  695 单测中 694 过(唯一失败 test_auto_expression_execution 为 DEBTS 在册
  引擎预存项,主仓同挂,与本次改动零交集);`ash -c "echo …"` 冒烟正常。
