# 058 — VM 版 ash-gui 行内编辑完整实现:keymap/功能分离 + Emacs 全量 + Vim 基础版

- 日期:2026-08-20
- 状态:**已完成**(实现 + 18 单测全过 + 全量回归无新失败 + gen/pytest 冒烟;
  真实键盘键位矩阵待用户手测 —— MCP 注入到不了编辑器 key_binding 闭包)
- 上游:plan-057(其 Tab/ghost/Ctrl+F 系列修复为本文地基)
- 跨仓库提交:auto-lang worktree(auto-shell 分支)`45ef43ad`,
  auto-shell `a59637a`

## 0. 背景与目标

用户澄清 bash 语义(Ctrl+F=forward-char 逐字、Ctrl+E=end-of-line 全收)后指出:
input 并未完整实现 Emacs 行内编辑快捷键;要求

1. **快捷键与功能分离**(keymap 数据表),未来可切 Vim(不麻烦则一起做);
2. 行内编辑完整后,补全系统回头对齐 CLI(补全与所有键盘操作兼容)。

## 1. 调研结论(设计依据,iced 0.14.2 源码实证)

1. **`Binding<Message>` 枚举完全公开可构造**(Move/Select/Sequence/Custom…)——
   纯编辑键可在渲染层直接翻译为 iced 原生动作,不必绕 .at handler 往返。
   key_binding 闭包设置后**完全替代**默认表(未命中需自调 `from_key_press`)。
2. **`Content::cursor()/selection()/move_to()` 公开**——光标(行内字节偏移)、
   选区可读,光标可精确重设。kill/yank/transpose 的基石。
3. iced 默认表缺口(Windows/Linux):C-a(=全选≠行首)/e/f/b/n/p/d/h/k/u/w/y/t、
   M-b/f/d、Tab 全无映射。**无 undo API**(C-_ 不做)。IME commit 绕过
   key_binding——中文输入不受影响。
4. **cosmic-text 实测陷阱**:Select(motion) 选区锚点词形吸附(起点=光标+1);
   显式选区+Edit::Delete 也多删一个字形(Affinity::Before)→ kill 最终采用
   **手动拼接重建 + move_to 精确落光标**,完全确定性。
5. 现状短板:每次按键 (value,ghost) 边界移动都全量重建 content 并把光标钉到
   value 末尾——**行中编辑(C-b 后打字)光标被拽走**,必须先修。

## 2. 架构:三层分离(快捷键 ↔ 功能 ↔ 状态)

```
[模式 Keymap 表]  — 数据,渲染层持有,随 keymap prop 选择
   emacs / vi-insert / vi-normal
      ↓ LineEditOp(功能抽象,与键位无关)
[执行] Editor 原生 Binding(纯移动/删除)   — 渲染层直接执行
       Kill/Yank/TransposeChars(复合)    — Content 操作 + 合成 on_change
       Handler/SetMode(语义)             — 派发 .at handler(input_value 带参)
[.at 层] edit_mode 状态 + SetEditMode + 语义 handler(换模式零改动)
```

派发优先级(key_binding 闭包内):**.at onkeydown 声明(App 语义,含 ghost
门控)→ 模式表 → vi-normal 吞可打印字符 → iced 默认**。

## 3. 实现(全部已落地)

### 渲染层内核(renderer.rs,+~700 行)
- `LineEditOp`:Native/Kill(Motion)/Yank/TransposeChars/DeleteCharOrExit/
  DeleteForward/Handler/SetMode/Seq/BeginPending/Swallow
- `line_edit_keymap(mode)`:
  - **emacs 全量**:C-a/b/e/f/h/d/k/u/w/y/t、M-b/f/d/BS、enter;
    C-d=空输入派发 OnCtrlD(退出)/否则前删
  - **vi-insert** = emacs + Esc→vi-normal
  - **vi-normal**:h l 0 $ ^ w b e G x X D d(pending:dd/dw/db) i a I A
    enter;未映射可打印字符吞掉(Sequence(vec![]))
- 独立 kill-ring(16 条,连续 kill 合并,不污染系统剪贴板)
- kill:`le_kill_motion`(readline 语义自算边界:End=光标→行尾、Home=行首→
  光标、WordRight=跳空白吃一词、WordLeft=连空白带词向后)→ `le_kill_range`
  手动拼接重建 + 光标落删除起点
- **光标保持(地基)**:update 区分编辑器自身消息(置 `TEXTAREA_ECHO_KEYS`)
  与 keydown/外部变更(清 echo);`get_textarea_content_rich` 重建时 echo =
  保持光标字节偏移(钳回 value 区),否则钉 value 末尾
- 闭包重构:`with_textarea_keydown` 增 keymap/on_change 参数;vi pending
  (Rc<RefCell>)处理 d 前缀两键序列
- NumpadEnter 归一为 "enter"(iced 默认无此映射)

### View/转换层
- `View::Textarea` + `keymap: String`(缺省 "emacs");`convert_textarea`
  读 `keymap:` prop(支持 `.edit_mode` 绑定);`convert_view_messages` 透传

### prompt_bar.at(auto-shell)
- `var edit_mode str = "emacs"` + `.SetEditMode(m)`;textarea 挂
  `keymap: .edit_mode`
- 补绑 `ctrl.p/ctrl.n` 历史;`ctrl.d` 移交模式表(readline 语义)
- `var cursor_pos int = 0`:渲染层在编辑器消息派发前写入光标字节偏移
  (仅 .at 声明处存在时),`complete(.input, .cursor_pos)` 光标感知补全
- 纯编辑键(C-a/b/e/f/k/u/w/y/t、M-*)不出现于 .at —— VM 渲染层职责,
  Vue 浏览器原生支持,codegen 不生成 = 行为正确(gen 已验证)

## 4. 验证

- **18 个单元测试全过**(renderer.rs `line_edit_tests`):kill 四方向、
  transpose 行中/行尾/单字符 no-op、yank 往复原原、连续 kill 合并、
  DeleteForward、光标偏移往返(**含中文字节偏移**)、echo 重建保光标、
  外部变更钉 value 末尾、三张键位表完整性
- 全量套件:`--features ui-iced` 9 个失败与基线(776e2144 临时 worktree
  对照)**完全一致**=预存(plan370×5、route::discovery、layout_extract、
  vm_bridge×2);本次净增 15 通过、零新增失败
- `auto gen` 冒烟 ✓;pytest 55 过 + 1 已知时序 flake(pb04 单跑过)
- **待用户真实键盘验证**(MCP keyboard 走 key_bindings 订阅通道,只覆盖
  .at 声明键;模式表的键只有真实按键事件到达编辑器闭包):
  C-a/e/b/f、C-k→C-y、C-t、M-b/f、**C-b 后继续打字光标不跳**、
  C-n/C-p 历史;Vi:改 `edit_mode` 初值 `"vi-normal"` 测 h/l/w/b/x/dd/i/Esc

## 5. 已知限制 / 下轮

- undo/redo(iced 无 API)、yank-pop(M-y)、quoted-insert(C-v)、word-case
  (M-u/l/c)、Vim operator-pending(d3w/ciw)、visual、`:` 命令行、
  Vue 端 Vim 模式(需 JS 层)——表结构均留扩展位
- 候选行按钮 onclick 循环变量参数在 handler 派发时不可用(`this.s`),
  按钮点击补全在 VM 仍不生效(键盘 Tab 不受影响),待修
- block 卡片背景/边框(bg-card/60 + border-border)未绘制(057 遗留)
- VM 引擎 `str.split` 返回 i32 索引列表 + SET_ELEM 仅支持 ListData<Value>:
  系统性缺陷,其他 .at 代码用 split 会踩,值得引擎层根治

## 6. finish-plan 复审(2026-08-24)

- 实现与 18 单测仍在 auto-lang master(renderer.rs:138 `LineEditOp` / :431
  `line_edit_keymap` / :12764 `mod line_edit_tests`)。
- §5 遗留逐项:按钮点击补全 → 已由 plan-062 T7 收口(PickCompletionIdx 索引
  回传,CP-01/02 在跑);block 卡片背景/边框 → 已支持(auto-lang class.rs
  `card/N`→Surface+alpha、`border` 类,block_item.at:44 使用中);str.split/
  SET_ELEM → DEBTS.md B5/B6 在册。
- **仍开放**:①真实键盘键位矩阵的用户手测(MCP 通道到不了编辑器 key_binding
  闭包,agent 无法替代);②undo/redo 等 §5 限制项(iced 无 API,平台约束)
  已入账 DEBTS(2026-08-24 复审,"行内编辑平台限制"条)。
