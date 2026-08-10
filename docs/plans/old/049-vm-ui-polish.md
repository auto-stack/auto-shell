# Plan 049: ash-gui VM 版 UI 完善

> **日期**: 2026-08-10
> **状态**: ✅ 完成(P1 input 清空 / P2 自动滚动 / P3 block 折叠 均已验证)
> **来源**: Plan 044-048 的延续。VM 版 ash-gui 经过多轮修复已基本可用,
> 本计划汇总所有已完成的改动 + 剩余改善需求。

---

## 一、已完成改动汇总(Plan 044-048)

### Plan 044: VM 后端对齐(shell.at mock → 真实数据)

| 阶段 | 内容 | 提交 |
|------|------|------|
| M1 | renderer stdout→Table 解析(parse_output_to_structured) | auto-lang 727a059e |
| M2 | shell.at 79 命令(签名提取自 ash-core registry) | auto-shell 4f0d1e1 |
| M3 | 传递性 view fn 注册(RenderTable 展开) | auto-lang 44135ebb |
| M4 | complete() 前缀过滤 + prompt_context 真实 git + read_history FFI | auto-shell d4e206f |

**关键发现**:
- 循环依赖(auto-shell→auto-lang)阻止了 Rust 桥接层方案,降级为 renderer 自解析
- VM 的 `use.rust` FFI 在 handler 里可用(Command.args() 曾是 no-op bug,已修)

### Plan 045: show 命令代码高亮(Code 变体)

| 内容 | 提交 |
|------|------|
| merged_exec_loop 拦截 show,读文件→{Code:{lines,language}} | auto-lang 075d6186 |

- show 是 ash 内置命令,不走 std::process(spawn 前拦截)
- MVP 纯色 span(白色),后续可引 syntect 做真实高亮
- RenderCode(block_body.at)已实现逐 span 渲染

### Plan 046: ls 命令修复 — 拦截执行 + Table 渲染

| 内容 | 提交 |
|------|------|
| 拦截 ls,用 std::fs::read_dir 直接构造 Table(消除 powershell 闪现) | auto-lang d130d6e5 |
| RenderTable 用 col/row(替代 HTML table 标签) | auto-shell c6367e1 |
| 回退绕过方案,改回标准 RenderedCell 二维数组 | auto-shell 8f75114 |

**关键发现**:
- VM 的 aura_view_builder 不支持 HTML table 标签(fallback 成 Column)
- VM 嵌套 for 循环 bug:ForLoop 解析 iterable 时忽略 bindings(内层 for cell in row 不迭代)

### Plan 046(续): VM 嵌套 for 循环 + input 清空

| 内容 | 提交 |
|------|------|
| ForLoop 4 处分支先查 bindings 再 read_state(嵌套 for 修复) | auto-lang 37775995 |
| PromptBar.Run 后 input_values.remove("Run")(input 清空) | auto-lang 37775995 |

### Plan 047: VM 版三项修复 — Table 布局 + input focus + 颜色

| 内容 | 提交 |
|------|------|
| convert_row flatten ForLoop(cells 横向排列) | auto-lang aa37f255 |
| input 固定 Id + Run 后 widget::operation::focus(第二次 Enter 修复) | auto-lang aa37f255 |
| color.rs 加 Sky 调色板(sky-50..900) | auto-lang aa37f255 |
| class.rs 支持 /N 透明度修饰符(parse_color_with_alpha) | auto-lang aa37f255 |
| application 加 .theme(Theme::Dark) | auto-lang aa37f255 |

### Plan 048: 表格样式 + 滚动条

| 内容 | 提交 |
|------|------|
| convert_column overflow-y-auto → View::Scrollable(滚动条) | auto-lang 41a68529 |
| Scrollable 传 style clone(flex-1 height(Fill),输入框贴底) | auto-lang 346f61ea |
| RenderTable 加 border 网格(每个 cell border border-border) | auto-shell f0be3cb |

---

## 二、剩余改善需求(本计划主体)

### ✅ P1: 输入命令后 Enter,input 不清空
**现象**: 输入 `ls` 回车,命令执行了但 input 框仍显示 `ls`。
**根因**: Plan 046 加了 `input_values.remove("Run")`,但 `on_with_input_for` 在
handler 前把 input_value 写入 state.input(根状态),handler 清的是 widget 本地 .input,
两者不同步。refocus 触发 view 重建后,input value 可能从根状态读到旧值。
**修复**: auto-lang 45e55871(on_with_input_for 写根状态 + handler 清空路径对齐)。

### ✅ P2: 自动滚动到底部
**现象**: 多条命令超出窗口高度后,滚动条出现但停在最顶部,需手动滚到底部。
**期望**: 新命令执行后自动滚动到最底部(最新 block 可见)。
**修复**: auto-lang 45e55871(snap_to_end 自动滚动)。

### ✅ P3: Block 标题点击折叠
**现象**: 每个 block 的标题行(❯ command + status)点击无反应。
**期望**: 点击标题行可折叠/展开 block body(方便多个结果时查看)。
**修复方向**: block_item.at 加 model { var collapsed bool = false },
标题行加 onclick 切换 collapsed,body 用 if !collapsed 条件渲染。

---

## 三、执行计划(状态更新)

### ✅ P1/P2(依赖 auto-lang 45e55871)
- input 清空 + snap_to_end 自动滚动已在 auto-lang 45e55871 完成。

### ✅ P3 block 折叠(本计划主体,2026-08-10)

| 步骤 | 内容 | 位置 |
|------|------|------|
| 1 | `block_item.at` 加 `model { var collapsed bool = false }` + `collapse_glyph` computed + `if !.collapsed` 条件渲染 body + 标题行 `onclick: .ToggleCollapse` | auto-shell f72dca8 |
| 2 | VM 修复 **`!` 前缀条件**:`eval_condition_with` 识别 `! .collapsed`(此前落入 resolve_binding_path → 恒 false → ls 结果默认全隐藏) | auto-lang(本计划) |
| 3 | VM 修复 **子组件 model var 默认值**:`render_child_widget` 把子组件 state_var 默认值(如 `collapsed=false`)种入统一 root state;`VmBridge::read_state/write_state` 增加实时 heap 对象按名回退(动态写入的字段不再 FieldNotFound → 修 `${collapse_glyph}` 字面量回退) | auto-lang(本计划) |
| 4 | VM 修复 **传递性 widget 的 handler 编译**:`register_transitive_widgets` 同步把孙组件(BlockItem)的 WidgetDecl 收进 child_decls,`handler_BlockItem_ToggleCollapse` 才存在(此前点击静默 HandlerNotFound) | auto-lang(本计划) |
| 5 | 模板 workaround **row onclick 被丢**:`convert_row` 无 onclick 字段,把折叠切换从 row 移到 `text` 元素(glyph + command,text 带 onclick 转无边框 Button,VM 已支持) | auto-shell(本计划) |
| 6 | 回归单测:`test_conditional_negation_*` / `test_text_onclick_becomes_toggle_button` / `test_child_model_var_default_seeded_into_state`(**28/28 过**);顺手修 vnode_converter 4 处过期 `on_right_click` 测试初始化(HEAD 上 test 构建一直编译不过) | auto-lang(本计划) |

**验证**(MCP):
- `ls` 后 body(表格)默认可见,glyph 显示 `▾`(不再 `${collapse_glyph}`)
- 点击命令文本 → `collapsed: true`,body 隐藏,glyph 变 `▸`;再点展开
- `python -m pytest tests/`:54 passed / 2 failed(均为既有问题:`test_mcp_server_responds` 工具数 12→13 陈旧断言、`test_pb04` 偶发,单独跑即过)

**已知限制**:VM 统一 root state 下,`collapsed` 是全局字段 —— 点击任一 block 会同时折叠/展开全部 block。逐 block 折叠需按 block id 做 per-instance state(后续计划)。

### P4: 其他发现(本计划)
- `handler_codegen` 对 `navigator.clipboard.writeText`(BlockItem.CopyCommand)软失败(WARN),复制按钮在 VM 模式为 no-op,Vue 路径不受影响。
- 3 个既有失败单测(`repro_selectday_panic`/`test_calendar_init_builds_42_cells`/`test_sizing`)在 HEAD 上因 vnode_converter 编译错误从未能跑,本计划修编译后确认仍为陈旧失败(016-calendar 处理器/数组约定 + style 解析),不属本计划范围。

## 四、关联

- Plan 044: VM 后端对齐(shell.at mock → 真实数据)
- Plan 045: show 命令(Code 变体)
- Plan 046: ls 命令 + 嵌套 for 循环
- Plan 047: Table 布局 + focus + 颜色
- Plan 048: 表格样式 + 滚动条
