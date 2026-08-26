# 070 — 多行编辑重构:底部动态脚本编辑器(ratatui Inline 模态)

- 日期:2026-08-26
- 状态:**待实施**
- 决策背景(用户裁定,2026-08-26):AutoScript 锁定的内联多行(Enter=换行)语义正确但
  呈现不合格——续行指示符与提示符不对齐、无行号、无滚动视界。参照 auto-ai 029
  「线性输出 + 尾部动态 + 按需模态」三层模型,多行编辑属于重交互,应从 reedline
  内联切换为 ratatui 驱动的独立编辑器模态(029 §2.7 模态层形态)。
- 关联:071(ash-tui 融合退役)的前置;本文档的模块设计按"可搬迁"原则编写。

## 0. 动机与定位

ash CLI 当前已天然符合线性动态哲学,本计划只补第三层:

| 层 | 现状 | 本计划 |
|---|---|---|
| 线性归档 | 输出全走原生回滚区;表格经一次性 Buffer→ANSI 线性打印(renderer/tui.rs) | 不动 |
| 动态尾部 | reedline 内联编辑 + hints + 补全/历史菜单(reedline 所有) | 仅修对齐 bug |
| 按需模态 | 无 | **脚本编辑器(ratatui Inline viewport)** |

刻意不做的:不用 ratatui 接管常驻尾部(那意味着用 textarea 重写 reedline 的
菜单/hints/vi/历史,收益不成比例;reedline 仍是单行输入引擎)。编辑器模态在
**两次 `read_line` 之间**运行,终端所有权无重叠。

## 1. 事实核验(本机源码,2026-08-26)

1. `ratatui-textarea = "0.9"` 已在 ash-tui/Cargo.toml 声明,**零使用**(注释称留给
   block-TUI 编辑器,后走了 LineBuffer 路线)。
2. ratatui-textarea 0.9.2 **原生行号 gutter**:`set_line_number_style`(textarea.rs:1937)
   → 渲染路径 `hl.line_number`(textarea.rs:1657-1661);官方 `examples/editor.rs:114`
   即带行号编辑器。undo/redo、光标移动、搜索内置。
3. textarea 公开 API 为纯文本(`insert_str`/`insert_newline`/…),**无逐行彩色 spans
   渲染接口**(内部 highlight.rs 只管光标/选区样式)→ 语法高亮必须 M2 自渲染。
4. 语法高亮管线现成:`RenderedOutput::Code` 的 syntect→CodeSpan(RGB+bold/italic,
   ash-core/renderer.rs:47,Plan 042 M6),block_tui.rs:1057 有消费示例。
5. 终端状态交接参照:block_tui.rs(038,Inline viewport + insert_before)与
   subprocess.rs(038 M3,全屏拆卸/重建)。
6. 内联对齐 bug:`render_prompt_multiline_indicator` 硬编码 `"..> "`(4 列,
   prompt/engine.rs:167),锁定提示符 `▌# ` 为 3 列 → 续行错位 1 列。

## 2. 交互设计

### 2.1 进入/退出矩阵

| 触发 | 行为 |
|---|---|
| F2(锁定 AutoScript) | 尾部直接切换为脚本编辑器模态(不再走内联多行;`1+2` 类单行也在编辑器里,Ctrl+Enter 运行) |
| Ctrl+O(任意模式,新键绑定) | 弹出同一编辑器,预填当前内联行(长 shell 管道同样受益);运行按当前锁定/自动检测路由 |
| 编辑器内 Enter | 换行 |
| 编辑器内 Ctrl+Enter | 运行:脚本以暗色回显线性提交进回滚区 → 执行 → 输出线性打印 → 编辑器清空重开(保持锁定,worksheet 形态) |
| 编辑器内 Esc | 缓冲区非空:内容以暗色提交进回滚区 + 「已取消」标记(不丢、可复制),清空但保持锁定;缓冲区已空:退出模态回内联(**双 Esc 退出**) |
| 编辑器内 Ctrl+C | 同 Esc(空缓冲退出;非空先取消) |
| F1/F3/F4、Alt+1..4 | v1 简化:等同 Esc 退出后再按原语义处理(编辑器不直接解释) |

提交语义对齐 029 §2.3:进入回滚区的内容永不重绘;取消也要留痕(暗色 + 标记),
转录完整、可原生复制。

### 2.2 模态布局(固定高度上限 + 内部滚动)

```
┌ 尾部动态区(Viewport::Inline,H = min(行数+2, 14))────────┐
│  1 │ fn add(a, b) int {            ← 行号 gutter(DarkGray) │
│  2 │     a + b                     ← textarea 主体(M2:    │
│  3 │ }                              ←   syntect spans 自渲染)│
│  ▌# AutoScript · Enter 换行 · Ctrl+Enter 运行 · Esc 取消    │ ← 状态行
└───────────────────────────────────┘
```

- 高度上限 14(029 同款约束:Inline 高度创建时定死,变高需重建 Terminal);
  超限 textarea 内部滚动。
- 不进 alternate screen、不捕获鼠标——原生复制全程可用。
- 每帧 draw 后按 textarea 光标偏移 `MoveTo`+`Show` 硬件光标(IME 锚定,
  029 §2.4 的手工等价);非聚焦态 `Hide`。
- 流程:进入 `enable_raw_mode` + 建 Inline Terminal → 事件循环 → 退出时
  回显提交、恢复 cooked、光标落模态区下方 → 回 REPL 循环(下次 read_line
  由 reedline 重新接管)。
- 兜底:panic hook + Drop guard 双保险恢复终端(029 §2.8)。

### 2.3 模块结构(纯新增,不动 reedline 路径)

```
ash-tui/src/editor_overlay/
├── mod.rs     # run_editor(prefill, mode_hint) -> EditorOutcome{Run(String)|Cancelled(Option<String>)|ExitLock}
├── term.rs    # 终端生命周期(raw/Inline/光标/panic hook/Drop guard)+ commit()
└── view.rs    # textarea 装配(行号样式)+ 状态行渲染;(M2: spans 自渲染)
```

repl.rs 接线仅两处:F2 锁定分支与 Ctrl+O 前缀(`\x0f`,新键绑定)分派到
`run_editor`;`EditorOutcome::Run` 复用现有 `execute_with_header`(锁定路由
已由 2026-08-26 的 set_locked_mode 修复打通)。

### 2.4 内联对齐修复(独立小项,先行合入)

`AshPrompt` 在 `set_character_symbol` 时记录符号显示宽度;
`render_prompt_multiline_indicator` 返回等宽指示(如 `▌# ` → `·· ` 3 列)。
reedline 内联多行保留(Shell 模式长命令 / 语法续行 `·` 仍用)。

## 3. Phase 分解

- **Phase 1 骨架**:对齐修复;Ctrl+O 键绑定;空编辑器进出(终端生命周期 +
  panic 兜底);reedline↔ratatui 交接验证(重点:read_line 返回后的 raw 状态,
  必要时显式重置)。
- **Phase 2 语义**:F2 接入(锁定分支改调 overlay);预填(Ctrl+O 带当前行);
  运行回显 / 取消留痕;状态行文案随模式(Shell/AutoScript)。
- **Phase 3 质感**:行号样式;硬件光标定位(IME);resize 手测矩阵
  (Windows Terminal / VSCode 终端;归档不重排为既定语义)。
- **Phase 4(M2,可缓)**:syntect 高亮——textarea 转"状态机用法"
  (`Input` trait 驱动,`lines()`+`cursor()` 取态,渲染自管 Paragraph+gutter,
  光标自绘反色块),复用 CodeSpan 管线。

## 4. 验收标准

- 交互矩阵全部成立;运行输出与取消内容都线性落入回滚区,鼠标原生框选复制正确。
- 退出模态后 reedline 提示符/hints/补全菜单/历史搜索无异常;-c/-s/script 非 TTY
  路径零影响。
- 对齐:内联续行与首行输入起始列一致(Shell/AutoScript/锁定三态)。
- 单测:键路由、Esc 双击状态机、预填逻辑、指示符宽度计算;编辑器渲染 golden 可选。
- 健壮性:panic 后终端状态完好(raw 无泄漏);resize 不崩、尾部重绘正常。

## 5. 风险与边界

1. **reedline↔ratatui 终端交接**(核心风险):Phase 1 首项验证;若 read_line
   返回后仍持 raw,进入前显式 disable、退出后恢复 cooked,下次 read_line 由
   reedline 重建自身状态。
2. **Inline 高度固定**:固定 14 + 内部滚动规避(029 同款)。
3. **Windows IME**:硬件光标定位是 pi/auto-ai 验证过的路径;bracketed paste
   兜底长中文输入;真机手测项。
4. **textarea 无 spans API**:M1 纯文本+行号完全可用;M2 自渲染是已知工作量,
   不阻塞主线。
5. 编辑器内 F 键"先退再处理"的语义损耗(按键需按两次)——v1 接受,收集反馈。
