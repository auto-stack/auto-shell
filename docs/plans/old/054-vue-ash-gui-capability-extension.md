# Plan 054: vue 版 ash-gui 能力扩展(对齐 CLI)

> **日期**: 2026-08-12
> **状态**: ✅ 实施完成(M1-M7 全完成,端到端 + vite build 验证通过)
> **来源**: 自行对比 CLI ash 与 Auto/vue ash-gui 的功能差距
> **范围**: `ash-gui-auto/src/front/*.at`(AutoLang 源码)+ `ash-gui/ash-server/`(后端)+ 必要的 auto-lang codegen
> **核心目标**: 让 vue 版(ash-server HTTP 模式)真正可用,并补齐相对 CLI 的关键体验缺口
> **前置**: Plan 053(M1-M6 输入体验 + 渲染打磨)已完成

---

## 0. 对比结论

### 架构(关键):CLI 与 vue **共享同一后端引擎**

`ash-server`(vue 后端)直接复用 `auto_shell::Shell` + `auto_shell::completions::engine`(Plan 041 M7 明确"TUI 和 GUI 共用")。所以**命令库、解析、管道、补全逻辑、安全策略、脚本执行全部共享,vue 版不缺这些**。差异全在**前端 + worker 桥接**。

### vue 版相对 CLI 的缺口

**🔴 P0 — 严重 bug(vue 版当前不可用)**
- **M1 命令执行未接线**:`shell_store.at` 的 `RunCommand` 只设 `__pending_command_*`(VM renderer 桥接约定),**不调 `/api/run_command`**、**不 push block**。vue/HTTP 模式无 renderer 桥 → 按 Enter 不发请求、后端不执行、SSE 无事件。`Cancel` 同理不调 `/api/cancel`。SSE 接收端(EventSource + onmessage → RunOutput/RunResult)是通的,但发送端断了。

**🟡 功能缺口**
- **M2 无数字退出码**:`CommandStatus` 只有 `Success`/`Failed(msg)`(ash-server `types.rs:91-96`),worker 不读子进程 exit code,前端 status glyph 只 4 态。CLI 有 `$?`。
- **M3 输出复制/导出 + block 删除**:BlockItem 只有 CopyCommand(复制命令行)/Stop/Rerun/ToggleCollapse,无 copy-output / delete-block / pin。
- **M4 SSE 无重连 / 无连接状态 UI**:`es.onmessage` 有,`es.onerror` 无;ash-server 重启即永久失联(需刷新页面);无 loading/reconnecting 指示。

**🟢 体验缺口(Plan 053 §6 共同缺口)**
- **M5 主题切换 UI**:`.dark` CSS 变量已备好(`index.css:38`),无 toggle 按钮。

**⚙️ 质量(codegen bug)**
- **M6 三处 codegen bug**:
  - `BlockBody.vue:75` CodeView `'font-weight:' + span.bold ? 'bold' : 'normal'` —— 三元优先级低于 `+`,恒为 'bold'(italic 同 bug)。根因:ts_adapter 字符串拼接 + 三元生成。
  - `PromptBar.vue:183`(DoTokenize)`c == '|' || c == ';' || c == '&' && ...` —— `&&` 优先于 `||`,`&` 单字符落入 word 分支,operator 高亮不全。.at 层逻辑 bug。
  - `BlockBody.vue` MemoryInfo Progress `Number(parseFloat(...))` 不剥 `%`、无 `isFinite` 守卫(`"abc"→NaN` 传 `<Progress>`)。Plan 053 B4 未完成。

### 不做(范围外)
- **作业控制 / 信号 / 后台 `&`**(jobs/fg/bg/suspend、Ctrl-C 转 SIGINT、`cmd &`):worker 单线程串行,需重构执行模型(独立工作面)。
- **管道/重定向流式**:`a | b` 走一次性 `shell.execute`,流式需 worker 改造。
- **终端专属命令**(less/more/color):显式降级提示(需真终端)。
- **store 全 any 类型**(Plan 053 §6 C):codegen 类型透传,独立大计划。
- **a2r 二进制模式**(72 编译错误):系统性缺陷,独立工作面。
- **transport 抽象 / Tauri 路径**:auto 版仅 HTTP,Tauri 是另一前端。
- **VM 模式兼容**:本计划聚焦 vue/HTTP;VM 是 plan 053 §6 D 工作面。M1 改动尽量不破坏 VM(保留 `__pending_*` 兼容),若 VM 回归另议。

---

## 1. 里程碑

### M1: 命令执行接线(P0 bug)★

**目标**:vue/HTTP 模式按 Enter 真正发命令、收 SSE 结果、渲染。

**改动**(`ash-gui-auto/src/front/shell_store.at`):
1. `use back.api:` 加 `run_command, cancel`(当前缺)。
2. `.RunCommand(cmd)`:
   - push block(Running 状态,对齐 `RunSmart` 的 block 构造模式)—— HTTP 模式需 store 自己 push(无 renderer)。
   - 保留 `__pending_command_*` 赋值(VM renderer 兼容)。
   - 加 `run_command(id, cmd)` 调用(async)。
3. `.Cancel`:加 `cancel()` 调用(保留本地 for 循环改状态)。

**风险**:VM 模式 renderer 可能也 push block → double push。测试 VM;若 double,VM renderer 调整或 handler 加守卫(VM 是 §6 D,不阻塞 M1)。

**验收**:vue 模式输入 `ls` → block 出现 → SSE 流式输出 → 结果渲染(✓ + 文本/表格)。`Cancel` 真能停。

### M2: 数字退出码

**目标**:block 显示数字退出码(对齐 CLI `$?`),非 0 标红。

**改动**:
- `ash-server/src/types.rs`:`CommandStatus` 加 `exit_code: Option<i32>`(或 Success/Failed 都带 code)。
- `ash-server/src/worker.rs`:`run_command` 读子进程 exit code,填入 status。
- `shell_store.at`/前端:status glyph 映射退出码(0=✓ 绿,非 0=✗ 红 + 码)。

**验收**:`ls` → ✓(0);`ls /nonexist` → ✗(非 0)+ 码显示。

### M3: 输出复制 + block 删除

**目标**:BlockItem 加 copy-output(复制输出文本)、delete-block(移除)、pin(置顶)。

**改动**(`block_item.at` + 可能 `block_body.at`):
- 加 `CopyOutput` handler:复制 block.output 文本到剪贴板(复用 CopyCommand 的 clipboard 模式)。
- 加 `DeleteBlock` handler:从 blocks 移除该 block。
- 加 `TogglePin` handler:pin 字段 + 排序(pin 在前)。
- BlockItem 视图加 3 个按钮。

**验收**:点 copy-output → 剪贴板有输出;delete → block 消失;pin → 置顶。

### M4: SSE 重连 + 连接状态

**目标**:ash-server 重启后前端自动重连;显示连接状态(connected/reconnecting)。

**改动**(`shell_store.at` + codegen SSE dispatch):
- store 加 `connection_status` 字段(connected/reconnecting/disconnected)。
- EventSource `onerror` → 设 reconnecting + setTimeout 重连(退避)。
- `onopen`/`onmessage` → connected。
- 视图(标题栏)显示连接状态指示。

**风险**:codegen 的 SSE dispatch(EventSource 生成)可能需扩展支持 onerror。若 .at 表达不了,降级为产物手写或 codegen 小改。

**验收**:停 ash-server → 前端显示 reconnecting;重启 → 自动重连 + connected。

### M5: 主题切换 UI

**目标**:标题栏加主题 toggle(dark/light),CSS 变量已备好。

**改动**(`app.at`):
- 加 `dark_mode` 字段 + `ToggleTheme` handler(`document.documentElement.classList.toggle('dark')` 或 `useColorMode`)。
- 标题栏加按钮(☀/🌙)。

**验收**:点 toggle → 主题切换;刷新保持(localStorage)。

### M6: codegen bug 修复

**目标**:修 3 处 codegen bug。

**改动**:
- **CodeView style 优先级**:`block_body.at` 的 CodeView span style 拼接。根因:`.at` 里 `'font-weight:' + (span.bold ? 'bold' : 'normal')` 生成时三元没括号。改 .at 加括号 `'font-weight:' + (span.bold ? ... )`,或 codegen 修字符串+三元优先级。
- **PromptBar DoTokenize operator**:prompt_bar.at:183 的 `c == '&' && ...` 逻辑(应 `&&` 连接 `c == '&'` 和下一个字符判断)。改 .at 逻辑。
- **MemoryInfo NaN 守卫**:block_body.at 的 Progress 解析加 `ends_with("%")` 剥 + `to_float` 兜底(Plan 053 B4 补完)。

**验收**:`ls`(CodeView)→ 非 bold 字体不恒粗;`echo "a | b"` → operator `|` 高亮;MemoryInfo `usage_percent: "75%"` → 进度条 75 非 NaN。

### M7: 验证

1. **codegen**:regen vue 产物。
2. **类型/构建**:`vite build` 成功(vue-tsc store any 是 pre-existing,不阻塞)。
3. **端到端**:启动 ash-server + vue 前端,逐项跑 M1-M6 验收。
4. **回归**:plan 053 单测全通过;M1 不破坏 VM(若 VM double push,记录)。

---

## 2. 改动文件清单

| 文件 | 仓库 | 里程碑 | 改动 |
|---|---|---|---|
| `ash-gui-auto/src/front/shell_store.at` | auto-shell | M1/M2/M4 | RunCommand 接线 / 退出码 / SSE 重连 |
| `ash-gui-auto/src/front/block_item.at` | auto-shell | M3 | copy-output / delete / pin |
| `ash-gui-auto/src/front/block_body.at` | auto-shell | M2/M6 | 退出码 glyph / CodeView style / MemoryInfo NaN |
| `ash-gui-auto/src/front/app.at` | auto-shell | M5 | 主题 toggle |
| `ash-gui-auto/src/front/prompt_bar.at` | auto-shell | M6 | DoTokenize operator 逻辑 |
| `ash-gui/ash-server/src/types.rs` | auto-shell | M2 | CommandStatus exit_code |
| `ash-gui/ash-server/src/worker.rs` | auto-shell | M2 | 读子进程 exit code |
| `crates/auto-lang/.../vue.rs` | auto-lang | M4(若需) | SSE dispatch onerror 支持 |

---

## 3. 依赖与顺序

**M1(P0)独立优先** → M6(质量,小改)→ M2(后端+前端)→ M3(前端)→ M5(前端)→ M4(SSE,可能 codegen)→ M7。

M1 解锁 vue 版可用性(当前废)。M6 顺带修质量。M2/M3/M5 前端增强。M4 最后(SSE codegen 风险)。

---

## 4. 后续工作面(范围外)

- 作业控制 / 信号 / 后台命令(worker 单线程重构)
- 管道/重定向流式(worker drain_stream 扩展)
- store 强类型(Plan 053 §6 C)
- a2r 二进制模式
- transport 抽象(Tauri)
- VM 模式深度兼容(plan 053 §6 D)
