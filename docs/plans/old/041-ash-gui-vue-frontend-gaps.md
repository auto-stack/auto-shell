# Plan 041: ash-gui-vue 前端差距 — 输入体验与提示模块

> **日期**: 2026-08-04
> **状态**: 📝 计划(待实施)
> **来源**: Plan 039(ash-gui-vue M1-M4)完成后的差距分析——纯前端可做的体验增强
> **范围**: `ash-gui/ash-gui-vue/src/`(纯前端,无需后端改动;个别项依赖 040 的后端数据)
> **前置**: Plan 039 已提交;建议在 040 之后或并行
> **预估**: M1-M7,~900 行 Vue/TS

---

## 0. 背景

Plan 039 交付了基本交互(输入、命令名补全、↑↓ 历史)。但对比 TUI/CLI ash 的输入体验,还差一批**纯前端可实现**的能力。这些不改后端,但会显著提升"日常使用"的舒适度。

## 1. 目标

为 PromptBar/BlockList 补齐 TUI 已有的输入体验:

1. **M1**:Ctrl+R 模糊历史搜索
2. **M2**:自动建议(ghost text + 模糊回退)
3. **M3**:多行输入 + 续行提示
4. **M4**:语法高亮
5. **M5**:prompt 模块(git 分支/状态、退出码、耗时)
6. **M6**:键盘快捷键(Ctrl+F/Ctrl+E/Ctrl+D/Ctrl+L)
7. **M7**:补全质量(描述 + flags)

## 2. 详细设计

### M1: Ctrl+R 模糊历史搜索

**TUI 参考**:`repl.rs:169-173` — Ctrl+R 打开 fzf 风格历史弹窗;`block_tui.rs:281-287` — 内联反向搜索子循环。

**实现**:
- 前端新增 `HistorySearch.vue`:一个弹层(popover/dialog),列出历史命令,输入过滤(fuzzy),↑↓ 选择,Enter 执行。
- 数据来源:040 M6 的历史持久化(`history()` command);040 未完成前先用手头 blocks 派生。
- 打开:`Ctrl+R` 快捷键(全局 keydown)。

验收:按 Ctrl+R 弹出历史,输入 `ls` 过滤出含 ls 的历史,回车执行。

### M2: 自动建议(ghost text)

**TUI 参考**:`repl.rs:258-277` — `AshHinter`:前缀匹配 + 模糊前缀-子序列回退(如 `gcm` → `git commit -m`);Ctrl+F 接受整条,Ctrl+Right 接受下一个词。

**实现**:
- PromptBar 里输入框后方渲染灰色 ghost text(推荐项),计算逻辑:前缀匹配 commandNames + 历史中最长匹配。
- Ctrl+F 接受整条,Ctrl+Right 接受下一个词。

验收:输入 `ls -` 出现灰色 `-al` 之类建议,按 Ctrl+F 接受。

### M3: 多行输入 + 续行提示

**TUI 参考**:`repl.rs:900-929` — 未闭合的 `{ } ( ) [ ] " '` 或尾随 `\` 触发续行,提示符变 `·`。

**实现**:
- PromptBar 判断输入是否未闭合(简单括号/引号计数),若是,Enter 换行而非执行;提示符从 `❯` 变 `·`。
- 输入框改为多行 textarea(或保持单行但支持 shift+enter 换行)。

验收:输入 `for i in (1..3) {` 按 Enter 换行继续,提示符变 `·`,补上 `}` 后 Enter 执行。

### M4: 语法高亮

**TUI 参考**:`repl.rs:279-284` — `AshHighlighter`。

**实现**:
- 前端用一个轻量 tokenizer(命令名/字符串/注释/变量着色),或引入 `prismjs`/`shiki` 的 shell 语言支持。
- 输入行 + Text block 的输出都套用。

验收:输入 `ls $HOME # comment` 时命令/变量/注释颜色不同。

### M5: prompt 模块(git 分支/状态)

**TUI 参考**:`prompt/modules/` — `git_branch`、`git_status`(`+N !N ?N ~N ⇡N ⇣N`)、`cmd_duration`、`status`。git 信息由后端缓存(`auto-shell/src/prompt/context.rs:46-92`)计算。

**实现**:
- 后端 040 或本计划加一个 `prompt_context()` command,返回 git 分支/状态/最近退出码/耗时。
- 前端 PromptBar 的 cwd 旁渲染 `⎇ main +2 !1` 之类。

验收:cwd 旁显示当前 git 分支和改动计数。

### M6: 键盘快捷键

**TUI 参考**:`repl.rs:139-241`。

| 键 | 功能 | 实现 |
|---|---|---|
| Ctrl+F | 接受整条建议 | PromptBar keydown |
| Ctrl+Right | 接受下一个建议词 | PromptBar keydown |
| Ctrl+E | 打开当前行到外部编辑器 | 需 `open_path` 或编辑器集成(可降级为提示) |
| Ctrl+D | 退出应用(无输入时) | window 级 |
| Ctrl+C | 清空当前输入 / 取消 | 与 040 M5 联动 |
| Ctrl+L | 清屏(归档 blocks) | App 级,清空 blocks 数组 |
| F1-F4 | 模式切换(Shell/AutoScript/AI) | 后续(AI 后) |

验收:Ctrl+L 清屏、Ctrl+F 接受建议、Ctrl+C 清输入。

### M7: 补全质量(描述 + flags)

**TUI 参考**:`completions_reedline.rs:285+` — 补全菜单显示命令签名、描述、flag。

**实现**:
- 后端 `command_list` 已有 `commands[]`(name + description)。前端补全项渲染为 `name + 描述` 两列。
- 可选:调 `get_command_signature(name)`(新增 command)拿 flags/参数做二级补全。

验收:补全建议带描述;有条件的支持 flag 补全。

## 3. 里程碑与验证

| 里程碑 | 内容 | 验证 |
|---|---|---|
| M1 | Ctrl+R 模糊历史搜索 | Ctrl+R 弹窗、过滤、选择执行 |
| M2 | 自动建议 ghost text | 输入出现灰色建议,Ctrl+F 接受 |
| M3 | 多行输入 + 续行提示 | 未闭合括号换行,提示符变 `·` |
| M4 | 语法高亮 | 命令/变量/注释异色 |
| M5 | prompt 模块 git 分支/状态 | cwd 旁显示 `⎇ main` |
| M6 | 键盘快捷键 | Ctrl+L 清屏、Ctrl+F 接受等 |
| M7 | 补全质量 | 建议带描述 |

各里程碑相互独立,可随意排序。M1/M2 依赖历史数据,建议 040 M6 之后做(或先用内存 blocks)。

## 4. 依赖与联动

- **M1**(历史搜索)与 **040 M6**(历史持久化)联动。
- **M5**(git prompt)需要后端暴露 `prompt_context()`(可加在 040)。
- **M6** 的 Ctrl+C 取消与 **040 M5** 联动。
- 其余纯前端,无后端依赖。

## 5. 参考文件

- `ash-gui/ash-gui-vue/src/components/input/PromptBar.vue`(输入体验主战场)
- `ash-gui/ash-gui-vue/src/composables/useShell.ts`(blocks/history 数据源)
- `ash-tui/src/repl.rs`(快捷键 139-241、hinter 258-277、高亮 279-284、多行 900-929)
- `ash-tui/src/prompt/modules/`(git_branch/git_status/cmd_duration)
