# 071 — 单一线性动态 CLI:线性输出+尾部动态架构(含 ash-tui 融合退役)

- 日期:2026-08-26
- **定位修正(用户裁定,2026-08-27)**:本计划不只是"tui 的合并"——crate 融合只是
  结构手段,真正的目标是**更好地实现「线性输出 + 尾部动态」架构**:CLI 主体是
  传统线性输出(结果固定、累计、可原生复制),尾部是 BlockTUI 形态的动态区
  (各种 Block 类型在运行中动态展示),运行完毕即冻结回归线性转录。shell 输出
  不做 GUI 式多窗口 Block 管理(Phase 1 已退役)。此方向的深化探索见 §6。
- 状态:**✅ COMPLETE(Phase 1-3 已实施,2026-08-27;实施记录见 §1/§7);§6 探索项为方向储备,逐个另立计划**
- 决策背景(用户裁定,2026-08-26):CLI 版随功能增长(文本编辑、模态)与 TUI 版
  边界模糊,而两者使用场景相同(终端环境)。auto-ai 029 已验证「线性输出 +
  底部动态」是终端最优形态:纯 CLI 交互太弱,自管滚动的复杂 TUI 易错且复制
  不便。ash 的 CLI 已天然线性(输出全走 ANSI 打印,表格一次性 Buffer→ANSI),
  block-tui 实验的自管 viewport 复杂度不再需要。故:退役 ash-tui 差异化前端,
  融合为单一 CLI 本体,形态对齐 auto-ai-cli。
- **用户二次裁定(2026-08-27,放行 Phase 1)**:不再单独搞 block-tui。「线性输出+
  尾部动态」= 传统 CLI 模式 + 尾部的 BlockTUI 形态 —— 各种 Block 类型在尾部
  **动态展示**(运行中),一旦运行完毕变成结果,就**回归固定输出、一直累计**。
  把 shell 输出也做成 Block(ash 变成和 GUI 程序一样的多小窗口管理)对 shell
  没有必要。
- Phase 1 实施记录(2026-08-27):
  - main.rs 删 `--block-tui` 预扫描(:57-59)/参数跳过臂(:108)/REPL 分支
    (:288-296);ash-tui/src 删 `block_tui.rs`(1787 行,含 072 M2 期间给它
    接的审批门 —— 模式退役,CLI 侧 repl.rs 的审批门为存活实现)与 `editor/`
    (M1 底行编辑器 5 文件,唯一消费者即 block_tui);lib.rs 删导出;
    block_header.rs 头注释去掉对 block_tui 的过期引用。
  - 独有能力核验(计划 §5.3 要求):block_tui 的交互清单(结构化块 insert_before
    渲染/Ctrl+R 历史/方向键历史/vi 检测/F 键模式切换/AI chat 流式)在 reedline
    主线均有等价物(renderer 线性 ANSI、reedline 菜单/hints、repl.rs 模式机、
    run_chat_loop);subprocess.rs(less/vim 交接)与 block_header.rs 按 D3 保留。
  - 回归:workspace 编译绿;ash-tui 123 测试全绿(原 153 中 30 个属 block_tui.rs/
    editor/,随文件删除);`ash -c` 冒烟正常,`--help` 无 block-tui 残留;
    editor_overlay 两处对 block_tui.rs 的注释引用改为指向 git 历史。

## 1. 决策

- **D1 单一前端**:退役 `--block-tui` 模式(block_tui.rs + editor/,038 实验,
  M1 编辑器未达标);reedline CLI 为唯一终端前端,070 的编辑器模态补足重交互。
  逃生门:若将来需要全屏形态,按 auto-ai 029 D4 的 `--mode fullscreen` 先例重建,
  历史可从 git 找回。
- **D2 crate 融合**:解散 ash-tui crate,模块并入 ash 本体(bin crate 直挂模块);
  **auto-shell 保持零终端依赖**(037 M2.2 的 crate 边界价值不变——消失的只是
  "两个终端前端"这个区分,而非纯逻辑边界)。RenderHook/PagerHook trait 仍在
  auto-shell,实现随模块迁入 ash。
- **D3 资产打捞清单**(block_tui 退役前吸收):
  - 终端生命周期 / Inline viewport / insert_before 经验 → 070 `editor_overlay/term.rs`
    的实现参考(知识层面,代码按 070 重写)。
  - `subprocess.rs`(038 M3 全屏交接:less/vim 需要拆卸重建 ratatui)→ **保留迁移**
    ——CLI 的 TuiPagerHook 同样可能进入 ratatui 上下文,交接代码通用。
  - `block_header.rs`(CLI 状态行)→ 保留(已是 CLI 所有)。
  - `render_block_header`(block_tui 内 ratatui 版)→ 随 block_tui 退役。
- **D4 reedline 保留为常驻输入引擎**:菜单/hints/vi/历史/abbr 是 CLI 核心资产;
  ratatui 只做按需模态(029 模态层)。不采用 auto-ai-cli 的"textarea 常驻接管
  尾部"路线——那等于重写 reedline 全部功能,收益不成比例;若未来遇硬阻塞再评估。

## 2. 事实核验(2026-08-26)

1. **依赖方向干净**:`ash(bin) → ash-tui(lib) → auto-shell → ash-core/auto-lang`。
   auto-shell 源码中的 `ash_tui` 字样均为注释(shell.rs:3877、prompt/mod.rs:14 等,
   已逐一核验);ash-tui 的外部使用仅 `ash/src/main.rs` + 自带 `tests/`
   (ls_render、renderer_golden、completion_runtime)。
2. **无 feature flag**:三个 crate 的 Cargo.toml 均无 `[features]` 段——
   历史上的 frontend-tui feature 已随 037 M2.2 拆分消失,融合无 cfg 清理负担。
3. **模块清点**(ash-tui/src,~行数含测试):
   - 保留迁移:`repl.rs`(~1100)、`prompt/`(engine+8 模块)、`menu/`、
     `renderer/`(tui+table+golden)、`term/`(color/highlight/hinter/prompt)、
     `completions_reedline.rs`、`commands.rs`+`commands_less.rs`、
     `block_header.rs`、`subprocess.rs`(159)。
   - 退役:`block_tui.rs`(1787)、`editor/`(M1 底行编辑器,completion/dispatch/
     hints/history,仅 block_tui 消费)。
4. **CLI 线性性核验**:TuiRenderHook 经 `rendered_to_ansi` 一次性转 ANSI 字符串
   由 Shell 线性打印(renderer/tui.rs:48-60)——CLI 无任何 viewport 持有,
   仅 reedline 内联区为动态尾部。
5. ratatui-textarea 0.9 已声明未用(070 启用);crossterm/ratatui 系依赖随迁移
   平移到 ash。

## 3. Phase 分解

- **Phase 0(前置)**:070 完成——编辑器模态落地于 ash-tui 内;模块按可搬迁原则
  编写(无跨模块私有依赖倒挂),本计划纯做搬迁。
- **Phase 1 退役 block-tui**:
  1. main.rs 删 `--block-tui` 分支/预扫描/帮助文案;
  2. 删 `block_tui.rs`、`editor/` 及其专属测试;
  3. 资产打捞确认(subprocess/block_header 无 block_tui 依赖交叉,可独立存活);
  4. 构建 + 全单测 + CLI 冒烟(REPL/表格渲染/less)。
- **Phase 2 crate 融合**:
  1. `git mv ash-tui/src/*` → `ash/src/frontend/`(repl/prompt/menu/renderer/term/
     completions/commands/block_header/subprocess/editor_overlay 平铺一层);
  2. ash/src/lib.rs 声明模块(bin 与 tests 共用;`tests/` 需 lib 目标);
  3. main.rs 的 `ash_tui::` 全量改 `frontend::`;模块内部 `crate::` 引用不变;
  4. `ash-tui/tests/` 三套 → `ash/tests/`(改 use 路径);
  5. Cargo.toml:ash 段并入 reedline/crossterm/ratatui*/ratatui-textarea/
     nu-ansi-term/rayon 等依赖;workspace members 删 ash-tui;删 crate 目录。
- **Phase 3 清理收尾**:
  1. 全仓 grep `ash_tui` 残引(含 auto-shell 注释)同步改写;
  2. docs(for-developers/for-agents/README/SKILL.md)架构描述更新为
     "auto-shell(纯逻辑)+ ash(线性动态 CLI)";
  3. 冒烟矩阵:REPL 全功能 / 070 编辑器模态 / less·vim 交接(subprocess)/
     -c / -s / script / --json;现有单测全量。

## 4. 验收标准

- workspace 两成员(auto-shell、ash);`cargo build`/`cargo test` 绿
  (存量失败除外:examples_parity 1 例、test_auto_expression_execution 1 例,
  均在册预存项)。
- 无 `ash_tui` 残引;`--block-tui` 入口消失且帮助文案同步。
- CLI 全功能回归:REPL、补全菜单、hints、历史搜索、F1/F2/F3 模式、070 编辑器、
  表格渲染、less/more、pager、AI 对话。
- git log `-M` 下模块搬迁可追溯(纯 mv + 路径改写,无逻辑变更混入)。

## 5. 风险与边界

1. **bin+lib 双目标**:ash 需加 lib.rs 供集成测试使用——bin 内 `main.rs` 与
   lib 的模块组织需一次定稳(建议 lib 只 re-export frontend,main 保持薄壳)。
2. **纯机械风险低但面积大**(~10 模块 + 3 测试套):Phase 2 单独成 PR,逐模块
   mv 后立即构建,不与任何逻辑改动混提。
3. **block_tui 退役的能力缺口**:其结构化块渲染 insert_before 形态由 CLI 的
   线性 ANSI 打印等价覆盖(037 已双轨并存数月,无已知只有 block-tui 才有的
   用户可见能力;Phase 1 前再核一遍 block_tui.rs 的独有交互清单)。

## 6. 方向储备:「线性输出 + 尾部动态」的深化探索(用户裁定 2026-08-27,逐项另立计划)

> 本节是本计划的真正主旨所在:融合/退役只是结构清理,以下是架构方向。
> 核心不变式:**线性转录永远固定、累计、可原生复制;尾部动态区只在"进行中"
> 存在,完成即冻结回归线性**。不回退成 block_tui 式全托管多窗口。

- **E1 尾部租约(tail lease)统一机制**:070 编辑器模态与 subprocess 交接是
  两个孤立先例;抽出统一的"尾部所有权"抽象——reedline `read_line` 间隙将
  尾部 Inline viewport 租给动态内容,结束即释放,term.rs 的 raw/恢复/panic
  兜底复用。这是 E2-E4 的公共地基。
- **E2 运行中命令的动态块**:外部命令流式输出目前逐行线性打印(打出去即
  固定);目标:运行中在尾部动态区渲染(限高滚动预览 + 耗时/spinner),
  完成后 `insert_before` 一次性冻结全文。GUI 侧 SSE 块流已是此模型,CLI
  对齐。
- **E3 结构化命令的渐进渲染**:表格类输出在数据到达时尾部动态增长(计数/
  预览),完成时渲染最终表格冻结——替代现在的"完成后才一次性打印"。
- **E4 AI 回合的动态渲染**:chat 流式文本在尾部重绘(markdown 渐进着色可行),
  回合结束整体冻结,替代逐字线性追加;审批卡(072 M2)同住动态区。
- **E5 长输出摘要冻结**:运行中尾部实时预览,完成后冻结"摘要 + 全文(截断
  或落临时文件提示路径)"——解决长构建刷屏淹没转录的问题。

## 7. Phase 2/3 实施记录(2026-08-27)

- **Phase 2(纯机械搬迁)**:`git mv ash-tui/src/* → ash/src/frontend/`
  (11 模块:repl/prompt/menu/renderer/term/completions_reedline/commands+
  commands_less/block_header/subprocess/editor_overlay);新 `ash/src/lib.rs`
  (lib+bin 双目标,`pub use frontend::*` 根级再导出使迁入模块内部
  `crate::` 路径零改写);唯一实质改写:commands.rs 内 `mod less` 的
  `crate::commands_less` → `crate::frontend::commands_less`(私有模块,
  glob 再导出不可达);三套集成测试迁 `ash/tests/`;依赖全量并入 ash
  Cargo.toml;workspace members 两成员;ash-tui 目录删除。
- **Phase 3(清理)**:auto-shell lib.rs 架构图/cmd.rs/completions.rs、
  ash main.rs/commands.rs 五处过期结构注释改写;README 项目结构图更新
  (两成员 + frontend/ 定位);`ash_tui` 残引清零(仅存 git 历史引用)。
- **回归**:workspace 编译绿;ash lib 108 单测 + 15 集成(ls_render 7/
  renderer_golden 6/completion_runtime 2)全绿;auto-shell 701 过 2 挂
  (在册预存,与融合前逐字一致);`ash -c` 冒烟正常;`ls | head -n 2` 的
  报错经 stash 对照确认为融合前既有行为(非回归,疑似 head 对结构化管道
  输入的既有边角,另册追查)。
4. **依赖平移**:auto-ai-client/auto-ai-agent 等 path 依赖随迁,版本对齐无冲突
   (同一 workspace 解析)。
