# 059 — TableBlock 表格增强 V1:排序 / 过滤 / 斑马线 / 右对齐 / 点击打开 / CSV 导出

- 日期:2026-08-21
- 状态:**VM 侧 V1 闭环**(2026-08-24 复审:过滤链路/▲▼ 指示由 plan-062 T9
  修复并当日复验通过;首命令 bug 不复现;CSV/TSV 导出在码待人工确认;
  余 Vue 侧增强 + vue 产物验证,见 §7)
- 上游:plan-058(行内编辑;其提交 45ef43ad 后出现的"首命令失败"回归影响
  本轮所有验证,见 §5.3)
- 跨仓库提交:auto-lang worktree(auto-shell 分支)`534b1e36`,
  auto-shell `6a30e64`、DEBTS 能力差距记录 `0a141cc`

## 0. 需求与范围(用户确认)

V1 全量:点击表头排序(方向指示)、列宽调整(仅 Vue 拖拽,VM iced 无逐列
轨道本期不做)、斑马线、CSV 导出(剪贴板)、点击单元格打开文件、数字列
右对齐、行过滤框。不做:真 xlsx、文件下载(无 DOM API 通道)、虚拟滚动、
列显隐、VM 列宽。

## 1. 方案演进(三轮,VM 限制逐次击穿)

1. **TableBlock 独立 widget**(model 存排序/过滤状态)→ 嵌套列表
   `[][]RenderedCell` 经 widget 参数传递解析为空(columns []str 正常)。
2. **状态放 BlockBody + RenderTable/RenderRows view fn** → view fn 内
   事件(onclick/oninput)解析直接报 unsupported event;且该轮出现
   block 随 output 一起消失的现象(后证实与首命令失败/实例管理混杂)。
3. **最终形态(已交付)**:表格标记**内联在 BlockItem widget view**
   (有 `.block` 上下文、事件合法),排序/过滤/打开经 **renderer 直改
   block 状态**(ToggleCollapse 同款桥)——Rust 侧 `sort_table_rows`
   原地重排 `output.Table.rows`。
   三重限制的完整记录见 DEBTS.md "Vue 可做/VM 不能做" §B(1/2/3/7)。

## 2. 已交付实现

### block_item.at(auto-shell 6a30e64)
- Table 输出分支内联增强表格:过滤框行(⌕ input)、可点击表头列名
  (onclick `.Sort(.block.id, ci)` —— 双参数事件 payload 实测合法)、
  ▲/▼ 条件指示、斑马行(idx % 2 偶数行 `bg-white/[0.02]`)、hover 类
  (Vue-only)、数字单元格右对齐(首字符数字启发 + tabular-nums)、
  Tagged 单元格(ls Name 列)onclick `.OpenPath(text)`。
- 非 Table 变体仍走 BlockBody。ash-table-* 标记类留给 vue.rs 注入。

### renderer.rs(auto-lang 534b1e36)
- 建块两处补前端字段 `table_sort_col/table_sort_dir/table_filter_q`
  (第二建块点顺带补齐此前就缺的 output/exit_code/collapsed)。
- `BlockItem.Sort(id, col)`:同列翻转/异列重置;`sort_table_rows`
  稳定选择排序,数字感知比较(前导数值含 K/M/G 单位缩放,否则字节序)。
- `BlockItem.Filter(id)`:查询词取 msg.input_value,写回后过滤+保持排序。
- `BlockItem.OpenPath(path)` → store.OpenPath(单元格点击打开)。

## 3. 验证(VM,MCP)

- warm-up 后 `ls` 表格完整渲染(过滤框 input + name/type/size/modified
  表头按钮 + 单元格)✓
- name 列表头两击:降序 target→src→Cargo.toml→Cargo.lock(与字节序
  预期一致)✓;数字感知比较经 Rust 单元路径覆盖(引擎侧无新单测)
- 斑马线/右对齐/Tagged→Button 转换经 vtree 结构核对 ✓

## 4. 未完成 / 下轮

1. **CSV 导出**:block_item `.CopyOutput` 的 Table 分支(columns +
   rows → CSV 转义 `"`→`""`、含分隔符加引号 → navigator.clipboard;
   顺带修 CopyOutput 对 Table 无效的遗留)。方案已在批准计划中,未写。
2. **Vue 增强(auto-lang vue.rs)**:识别 `ash-table-*` 标记类注入
   ——列宽拖拽(拖柄 + colWidths ref 覆盖 grid-template)、表头吸顶
   (sticky)、行 hover(CSS 类已就位)。
3. **过滤链路打通**:Filter 桥已写但实测输入后未筛行 —— 排查 input
   oninput 的 input_value 是否到达桥(疑 input_state_map 深路径
   `.block.table_filter_q` 写入失败导致 handler 参数为空)。
4. **▲/▼ 指示未渲染**:button content 内条件子节点(if .block.
   table_sort_col == ci)不显示,疑 button content 子树的条件子元素
   支持 —— 需 aura converter 侧确认。

## 5. 已知问题

1. 过滤框输入不筛行(§4.3)。
2. 表头方向指示不显示(§4.4)。
3. **VM 启动后第一条命令不执行**(run_command 未发出,服务端无记录;
   第二条起正常)。时间窗指向 45ef43ad(行内编辑)前后的 update 变更,
   与表格无关但污染一切验证流程(本轮所有测试需 warm-up 首命令)。
   排查方向:首条命令的 store.RunCommand handler 内 run_command()
   HTTP POST 首次调用失败(api_over_http 冷启动?async 重入?),待专项。
4. Vue 端 `.at` 生成的表格未经 `auto gen` + build 验证(内联模板改动)。

## 6. 推翻条件(与 DEBTS 联动)

- DEBTS §B1/B3 修复后:表格可回归纯 .at 声明式实现,renderer 桥退役;
- DEBTS §B7 修复后:Sort/Filter/OpenPath 桥收敛为标准 emit 链。

## 7. finish-plan 复审(2026-08-24)

- §4.3 过滤链路、§4.4 ▲/▼ 指示:**已由 plan-062 T9 收口**(引擎两修:Filter 桥
  id 解析 int 优先 + convert_input 事件参数烘焙改 event_to_message_with)。当日
  复跑 TF-01/TF-02 于当日二进制(auto.exe 08-24 17:28 / ash_server.dll 15:33)
  均通过。§5.1/§5.2 随之销案。
- §4.1 CSV 导出:**已在码** —— CopyOutput(Table→TSV)与 ExportCsv(CSV 引号
  转义)两侧齐全(block_item.at:466-561 + auto-lang renderer.rs:6917 arboard 桥);
  剪贴板内容无 MCP 断言通道,062 T9 定为"留人工验证"—— 用户点一次导出图标
  即闭环。
- §5.3 首命令不执行:**当前不复现**(2026-08-24 探针:全新实例单次提交 echo,
  5.3s 出块;062 §9 T10 双根因修复后该族消除;测试中 `echo warmup` 已是惯性
  写法而非规避)。
- §4.2 Vue 增强(列宽拖拽/吸顶/hover 注入):**未做** —— auto-lang vue.rs 无
  ash-table 注入,block_item.at 中标记类亦已不存;062 T9 曾裁定"依赖 auto-lang
  可用窗口,可后置"但未入账 DEBTS。§5.4(vue 产物 gen+build 验证)与 057 §4.5
  同项,均未做(gen/ 停在 2026-08-05)。
- 结论:VM 侧 V1 已闭环;尾巴 = Vue 侧表格增强 + vue 产物重验证 + 剪贴板
  人工确认。

## 8. Phase 2 — 尾巴收口(2026-08-24 立项,finish-plan 复审产物)

- **T-C CSV/TSV 剪贴板自动验证**:CopyOutput(Table→TSV)/ ExportCsv(CSV 转义)
  的 arboard 桥以 pytest 锁定 —— MCP 点击导出图标后经
  `powershell Get-Clipboard` 断言剪贴板内容(TSV 列分隔 / CSV 引号转义)。
  取代 062 T9 的"留人工验证"。验收:新增 1-2 项 pytest 过。
- **T-D Vue 表格增强**:分三档 —— 行 hover(Vue 端 `hover:` 类原生支持,随
  T-B 重生成验证即可);表头吸顶(`sticky` 一行 CSS,验证生成产物);列宽
  拖拽(需 vue.rs 注入脚本或生成物后处理,评估成本后落地或记 DEBTS)。
  验收:重生成后的 Vue 表格具备 hover + 吸顶,列宽拖拽有明确结论。

### Phase 2 实施记录(2026-08-24)

**T-C 剪贴板自动化**:BL-05(CopyCommand 命令文本)、BL-07(CopyOutput→TSV,
含 tab 分隔断言)、BL-08(ExportCsv→CSV,含逗号分隔断言)经
`powershell Get-Clipboard` 锁定 —— 062 T9 的"留人工验证"销案。注意:剪贴板是
系统级共享,并发测试实例会互踩(读到对方的写入),此类用例不可并行跑。

**T-D Vue 表格增强**:行 hover 已在(`hover:bg-white/[0.03]`,Vue 原生生效);
表头吸顶已加(`sticky top-0 z-10 backdrop-blur-sm`,block_item.at 表头行 ——
Vue 生效,VM 忽略 sticky 无害,生成产物已验证);**列宽拖拽延期**(需 vue.rs
JS 注入或生成物后处理,入 DEBTS)。
