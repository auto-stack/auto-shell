# Plan 041: ash-gui-vue 前端差距 — 输入体验与后端能力复用

> **日期**: 2026-08-04
> **状态**: ✅ **完成（归档 2026-08-05）** — M1-M8 全部交付。cargo check + vue-tsc 通过;ash-core 79/79、ash-tui 13/13、auto-shell 698/698 测试不回归。
> **来源**: Plan 039/040 完成后的差距分析——对比 TUI/CLI ash 的输入体验 + 后端能力复用
> **范围**: `ash-gui/ash-gui-vue/src/`(前端)+ `src-tauri/`(必要的后端桥接)+ `auto-shell`/`ash-tui` 下沉重构
>
> **⚠️ 架构变更(Plan 042)**: 本计划的前端组件(PromptBar/BlockList/HistorySearch/等)不受
> 影响——Plan 042 只换数据源(`useShellMock` → `useShellHttp`,真实后端)。本计划 M7 下沉的
> 补全引擎(`auto_shell::completions::engine`)继续被 `ash-server` 复用。`useShellMock` 将在
> Plan 042 M5 删除。
> **前置**: Plan 040 已完成(shell.execute 完整复用)
> **预估**: M1-M8,~1100 行 Vue/TS + ~300 行 Rust

---

## 0. 背景:复用 vs 重写的分界线

ash 三个版本(CLI / TUI / GUI)应**共用同一套后端核心**(`auto-shell` + `ash-core`)。
Plan 040 已让命令执行层三版统一(都走 `shell.execute()` → `execute_inner`)。本计划处理
**剩余的复用缺口**:补全、命令覆盖、提示上下文。

经代码核查(`completions_reedline.rs` / `completions/provider.rs` / `shell.rs:execute_inner`),
补全/命令的复用边界如下:

| 层 | 能否三版复用 | 现状 |
|---|---|---|
| **补全引擎**(`CompletionProvider` + `get_completions_with_context`) | ✅ 纯逻辑,无终端依赖 | CLI/TUI 用了;**GUI 没接** |
| **补全编排**(上下文快照/AI 合并/排序/光标判断) | ✅ 能复用,但需从 `ShellCompleter` 下沉 | 目前混在 ash-tui,绑了 reedline |
| **命令执行** | ✅ 已复用(Plan 040) | 三版都走 `shell.execute()` |
| **命令覆盖**(b/up/alias/…硬编码 builtin) | ✅ 已复用(走 execute_inner) | GUI 全覆盖(见 §M8 核查) |
| **终端专属命令**(less/more/color) | ❌ 不可复用(crossterm raw mode) | GUI 需替代方案(见 §M8) |
| **渲染/触发** | ❌ 不可复用(各前端各做) | 必然差异 |

## 1. 目标

为 PromptBar/BlockList 补齐 TUI 已有的输入体验,**并修复后端能力复用缺口**:

1. **M1**:Ctrl+R 模糊历史搜索
2. **M2**:自动建议(ghost text + 模糊回退)
3. **M3**:多行输入 + 续行提示
4. **M4**:语法高亮
5. **M5**:prompt 模块(git 分支/状态、退出码、耗时)
6. **M6**:键盘快捷键(Ctrl+F/Ctrl+E/Ctrl+D/Ctrl+L)
7. **M7**:补全复用(下沉 `CompletionProvider` + 编排逻辑,三版共用)★
8. **M8**:命令覆盖对齐(b/up/alias/… 已全覆盖核查;less/more/color 给替代方案)★

> ★ M7/M8 是 Plan 041 修订时新增的"后端复用"项,源自对"三版应共用同一套后端"的架构核查。

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

### M7: 补全复用(下沉 CompletionProvider + 编排逻辑)★

**问题根因**:GUI 补全目前是前端 `startsWith` 前缀过滤(`PromptBar.vue:32-36`),只用了
boot 快照里的命令名列表(`commandNames`)。而 CLI/TUI 用的是 `CompletionProvider`(能补
命令名 + 文件路径 + 参数 + flag + 子命令 + AI 翻译 + context-aware 排序)——**这套能力
是纯逻辑、无终端依赖的,本可三版共用,但 GUI 没接**。

差距来源:`ShellCompleter`(`ash-tui`,839 行)把**补全编排逻辑**(上下文快照/AI 合并/
排序/光标判断)和**reedline 绑定**(`impl Completer → Vec<Suggestion>`)混在了一起。
reedline 绑定不可移植,但编排逻辑应该下沉到核心层,让三版共用。

**方案(三步)**:

1. **下沉编排逻辑到 `auto-shell`**:新增 `auto_shell::completions::complete_with_context()`,
   返回 `Vec<Completion>`(核心类型,无终端依赖)。把 `ShellCompleter::complete()` 里的
   编排逻辑(ensure_spec → provider.resolve → AI 合并 → get_completions_with_context →
   ranking)搬过来,返回 `Vec<Completion>` 而非 `Vec<Suggestion>`。
   - 签名:`pub fn complete(line: &str, cursor: usize, ctx: &CompletionCtx) -> Vec<Completion>`
   - `CompletionCtx`:cwd / last_command / last_exit_code / history / aliases / command_executor

2. **`ShellCompleter` 变薄**:只保留 `Completion → reedline::Suggestion` 类型转换 + reedline
   接口实现(`impl Completer`)。内部调下沉后的 `complete_with_context()`。CLI/TUI 行为不变。

3. **GUI 后端加 Tauri command** `complete(line, cursor)`:
   - worker 持有 `CompletionProvider` + `CompletionSignature[]`(boot 时从 registry 收集,
     与 TUI `repl.rs:89-90` 一致)。
   - 调 `complete_with_context()`,返回 `Vec<Completion>`(序列化 `{display, replacement,
     description, kind}`)给前端。
   - 前端 PromptBar 调它,渲染带描述/类型的补全候选(M2 ghost text 也复用此数据源)。

**为什么这样而不是搬 ShellCompleter**:reedline 的 `Suggestion`/`Span` 是终端渲染概念,
搬到 webview 无意义。下沉逻辑、保留各自前端的渲染层,与命令执行的复用模式一致
(三版共用 `shell.execute()`,各做各的输出渲染)。

**CLI/TUI 影响**:零行为变化——`ShellCompleter` 改为调下沉函数,返回的 `Completion` 转
`Suggestion` 的映射不变。已有测试(`completions_reedline.rs` 的 839 行含测试)保护回归。

验收:
- GUI 里输入 `ls -` 补全出 `-a`/`-l` 等 flag(来自 CompletionSignature)。
- 输入 `cat <Tab>` 补全文件路径(来自 CompletionProvider 的路径补全)。
- 补全候选带描述。
- CLI/TUI 补全行为不回归(839 行测试全过)。

### M8: 命令覆盖对齐(核查 + 终端命令替代)★

**核查结论**(2026-08-04 代码验证):GUI 走 `shell.execute()` → `execute_inner`,**所有
硬编码 builtin 已全覆盖**——不需要为 `b`/`up`/`alias`/`pushd`/… 做任何额外工作:

| 命令类别 | 在哪实现 | GUI 是否覆盖 | 说明 |
|---|---|---|---|
| `b`/bookmark | `execute_inner:709` → `execute_bookmark_command` | ✅ | 走 execute,全覆盖 |
| `up`/`u` | `execute_inner:708` → `execute_up_command` | ✅ | 同上 |
| `alias`/`unalias`/`source`/`pushd`/`popd`/`dirs`/`jobs`/`fg`/`bg`/`def`/`hook`/`abbr`/`config`/`bind` | `execute_inner:608-627` | ✅ | 全部走 execute_inner |
| `set`/`export`/`unset`/`use` | `execute_inner:697-705` | ✅ | 同上 |
| 注册命令(ls/cat/grep/find/...) | registry → `run_atom` | ✅ | Plan 040 M1 已修(走 execute) |

**用户报告 `b` 不工作的真实原因**:**不是执行不支持,而是补全没提示它**。`b` 是
`execute_inner` 硬编码 builtin,不在 registry 里,而 GUI 的 boot 快照 `commands[]` 只从
registry 收集(`harvest_boot` → `reg.names()`)——所以命令列表/补全里看不到 `b`/`up`/
`alias` 等。**这是 M7(补全复用)要一并解决的**:补全数据源要包含硬编码 builtin,而非只
从 registry 取。M7 下沉 `get_completions_with_context` 后,这些 builtin 已在其覆盖范围
内(`completions/mod.rs` 内置了 builtin 清单)。

**真正不能复用的是终端专属命令**(需 crossterm raw mode + alternate screen):

| 命令 | 终端依赖 | GUI 替代方案 |
|---|---|---|
| `less` | `commands_less.rs`:crossterm raw mode + 键盘事件 + 滚动 | GUI 已有 BlockList 滚动;长输出本就流式显示(M4)。`less` 在 GUI 里降级为 no-op + 提示"输出已在上方滚动区" |
| `more` | 同 less | 同上 |
| `color` | `commands.rs:40-140`:24-bit 真彩渐变 | GUI 走 webview CSS,无终端颜色概念;降级为显示 webview 的 color profile |
| `show --pager` | `PagerHook`(crossterm) | GUI 用代码高亮面板(Plan 041 M4 同源)替代翻页器 |

**实现**:
- GUI worker 不调 `register_commands(terminal_commands())`——这些命令在 webview 无意义。
- 但需要让用户输入 `less file` 时不报"command not found",而是给出友好提示
  ("GUI 内置滚动,无需 less")。可在 `is_shell_builtin` 白名单加这几个名字,
  worker 检测到时返回提示而非尝试 spawn。
- `show --pager` 在无 `PagerHook` 时已有降级(`shell.rs` format_output 的 fallback),
  GUI 不注入 hook 即可——输出走正常结构化渲染。

验收:
- `b add foo` / `b foo` / `b list` 在 GUI 里正确工作(走 execute_inner,无需额外代码)。
- `less somefile` 不报 not found,给出友好提示。
- 注册命令 + 硬编码 builtin 都在补全候选里出现(M7 联动)。

## 3. 里程碑与验证

| 里程碑 | 内容 | 验证 | 类型 |
|---|---|---|---|
| M1 | Ctrl+R 模糊历史搜索 | Ctrl+R 弹窗、过滤、选择执行 | 前端体验 |
| M2 | 自动建议 ghost text | 输入出现灰色建议,Ctrl+F 接受 | 前端体验 |
| M3 | 多行输入 + 续行提示 | 未闭合括号换行,提示符变 `·` | 前端体验 |
| M4 | 语法高亮 | 命令/变量/注释异色 | 前端体验 |
| M5 | prompt 模块 git 分支/状态 | cwd 旁显示 `⎇ main` | 后端桥接 |
| M6 | 键盘快捷键 | Ctrl+L 清屏、Ctrl+F 接受等 | 前端体验 |
| M7 | 补全复用(下沉 CompletionProvider) | flag/路径补全 + 描述,CLI 不回归 | **后端重构★** |
| M8 | 命令覆盖对齐 | b/up/alias 工作;less 友好降级 | **核查 + 桥接★** |

各里程碑相互独立。**M7 是其他项的数据基础**(M1 历史搜索/M2 ghost text 都依赖补全
引擎的完整数据源),建议优先做 M7。M8 的执行覆盖已确认无需额外工作(走 execute_inner),
只需补全联动 + 终端命令降级提示。

## 4. 依赖与联动

- **M7**(补全复用)是 **M1/M2/M8** 的数据基础——下沉后的 `complete_with_context()`
  提供完整补全数据(含硬编码 builtin),M1 历史搜索/M2 ghost text/M8 命令覆盖都依赖它。
  **建议优先做 M7。**
- **M7** 涉及 `auto-shell`/`ash-tui` 下沉重构(把编排逻辑从 `ShellCompleter` 搬到
  `auto_shell::completions`),CLI/TUI 必须零行为回归(839 行测试保护)。
- **M8** 的命令执行覆盖已确认走 `execute_inner` 全覆盖,无需额外后端工作;只需
  补全联动(M7)+ 终端命令(less/more/color)友好降级提示。
- **M1**(历史搜索)与 **040 M6**(历史持久化)联动。
- **M5**(git prompt)需要后端暴露 `prompt_context()`(可加在 040)。
- **M6** 的 Ctrl+C 取消与 **040 M5** 联动。
- M3/M4/M6 纯前端,无后端依赖。

## 5. 参考文件

- `ash-gui/ash-gui-vue/src/components/input/PromptBar.vue`(输入体验主战场)
- `ash-gui/ash-gui-vue/src/composables/useShell.ts`(blocks/history 数据源)
- `ash-tui/src/repl.rs`(快捷键 139-241、hinter 258-277、高亮 279-284、多行 900-929)
- `ash-tui/src/prompt/modules/`(git_branch/git_status/cmd_duration)
