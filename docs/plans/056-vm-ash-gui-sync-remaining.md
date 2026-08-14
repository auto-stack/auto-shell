# 056 — VM(iced)版 ash-gui-auto:剩余问题与同步 Vue 改进的路线

- 日期:2026-08-13
- 状态:**未开始**(阻塞性问题已定位,待 auto-lang VM 侧修复后推进)
- 上游:plan-053(输入/面板打磨)、plan-055(jobs)、055 的 Phase 1 逻辑可移植化(commit `3f92d85`)
- 关联调研:`designs/ash-gui-native-plan.md` §9.7(三个 VM bug 的二分定位)

## 0. 背景与目标

Vue(Auto/vue)版已落地多项改进:表格网格列对齐、单元格按 kind 着色、最新块高亮、
block 输出限高+滚动、输入框去 pwd、Tab 循环补全、语法高亮(产语义 kind,逻辑层
target-neutral)。目标是把可同步的部分同步到 VM(iced)版的**显示**上,并用 AutoUI
MCP 工具(snapshot/screenshot)实测两版差距。

**结论先行:当前 VM 版起不来(link 失败),且即使起来也有结构性显示缺口。
本计划记录已修/已排除项、剩余阻塞、与分阶段路线。**

## 1. 本会话已修复(已提交 push)

| commit | 修复 |
|---|---|
| `5840c8a` | VM mock `shell.at` 补 `jobs`/`kill_job`(plan-055 加入 api.at 但 mock 缺失 → `Undefined symbol: shell.jobs`,VM link 失败) |
| `945ff89` | mock `subscribe()` 去掉 `return Stream<ShellEvent>.empty()` → `Undefined variable: Stream` 告警消失(`~Stream<T>` 是类型注解,非 VM 运行时类型) |

## 2. 剩余阻塞 A:VM 启动 link 失败(VM 内部 bug)

**症状**:
```
Error: VM UI error: DynamicComponent init failed: VmBridge init failed for 'App':
  invalid state: link failed for 'App': Undefined symbol:
  PromptBar_State.OnInputComplete in module App
```

**已逐一排除的假因**(都实测过,无效):
- ❌ Tab 循环把 `.OnInputComplete()` 挪进 if/else 分支(嵌套)→ 挪回顶层依旧错;
- ❌ `expose { .OnInputComplete … }` → expose 是 vue-codegen 专属,VM 不实现透传;
- ❌ §9.7 BUG-C「模板可见性」→ 加 `if false { button { onclick: .OnInputComplete } }`
  隐藏引用,依旧错;
- ❌ `~Stream` 连锁(Stream 告警已修掉,但 OnInputComplete 依旧错)。

**定性**:§9.7 BUG-C 一类的**真 VM bug** —— 子组件 state-struct 的 handler 符号
生成不完整(`PromptBar_State.OnInputComplete` 未生成,App 链接失败)。§9.7 建议的
修复位置:`auto-lang/src/ui/vm_bridge.rs`(handler dispatch / 符号生成)。

**修复方向(auto-lang 侧)**:
1. VM 为子组件生成 state 符号时,把「事件绑定(oninput/onkeyup/onkeydown/onenter
   等全部事件类型)+ handler 间调用(含嵌套 if/else 内)+ expose 声明」都纳入
   可见性判定,而不只是部分模板扫描;
2. 或实现 `expose` 的 VM 透传(其设计本意即解决此问题)。

## 3. 剩余阻塞 B:结构性显示缺口(即使启动后)

| Vue 改进 | VM 现状 | 定性 |
|---|---|---|
| 单元格按 kind 着色 / 最新块高亮 / 状态色 | 走 Tailwind `class:` + 共享 `color.rs` 调色板,理论上跨 vue/VM —— **待 VM 起来实测确认** | 可移植,预期生效 |
| 输入框语法高亮 + ghost(透明 textarea + absolute overlay) | iced 的 `TextInput`/`TextEditor` 不接受 styled spans/ghost 后缀;iced 忽略 absolute 定位(`iced_adapter.rs:178`) | **结构性**,需 renderer 扩展 |
| 表格网格列对齐(`display:grid` CSS)/ block 限高 `max-h-[400px]` / 提示符间距 `gap-2` | CSS,iced 忽略 | **结构性**,需 iced 原生布局或 renderer 支持 |
| 长输出内部滚动(AutoUI ScrollArea / .ash-scroll CSS) | scroll 节点 → iced 滚动区,但 max-h 是 CSS;需 iced 限高方案 | 部分可移植,待实测 |

另注意:VM 启动日志有 `Field 'exit_code'/'collapsed' not found in generic type 'Block'`
告警(store 给 Block 赋这两个字段),需核对 VM 的 Block 泛型字段表。

## 4. 分阶段路线

- **Phase 1(阻塞,auto-lang)**:修 VM 子组件 handler 符号生成(§2)→ `auto run -r vm`
  能启动。注意:auto-lang 工作区当前有**另一会话未提交的 Plan 409 iced 改动**
  (`ui/aura_view_builder.rs`、`ui/dynamic.rs`、`ui/iced/renderer.rs`、
  `ui/style/color.rs`)—— 动 vm_bridge.rs 前先协调/避开。
- **Phase 2(实测差距)**:VM 起来后用 AutoUI MCP(注意 9247 常被 widgets-gallery
  的 VM 占用,用 `AUTOUI_MCP_PORT=9248` 起 ash-gui-auto)snapshot/screenshot 对比
  Vue,确认 §3 表中「预期可移植」项是否生效、结构性缺口清单落地。
- **Phase 3(iced renderer,重活)**:输入高亮/ghost(扩展 `View::Input`/`Textarea`
  加 spans/ghost 字段 + 富文本输入控件;先做可行性 spike)、布局对齐(网格/限高)。
- **a2r(Rust)**:暂缓(README 记录 72 个编译错,系统性缺陷未修)。

## 5. 环境注意事项

- `auto run -r vm` 的 MCP 默认 9247;widgets-gallery 的 VM(另一会话自动化)常驻
  9247,起 ash-gui-auto 前用 `AUTOUI_MCP_PORT=9248` 避让,或先停掉占用进程。
- auto-lang 的 auto.exe 重建常被运行中的 `auto.exe` 锁住:把占用文件改名
  (`auto.exe.lockedN`)再 build(会话内已用多次,记得事后清理)。
