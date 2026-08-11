# Plan 050 附录：store handler 双模冲突——三方案深入分析

> **日期**: 2026-08-10
> **性质**: plan 050 调研报告的技术附录
> **问题**: shell_store.at 的 RunOutput/RunResult 是无参 + 读 `__sse_*` 预置字段
> （为 VM --merged 的 renderer 回流设计）。vue SSE 需要 EventSource dispatch。
> 同一份 .at handler 定义如何同时服务两种模式？

---

## 0. 问题精确化：到底冲突在哪？

先把"双模冲突"拆成具体的两个子问题，避免混淆。

### 子问题 A：handler 的"参数模式"

当前 .at 里两个回流 handler 是**无参**的：

```
// shell_store.at:141 / :156（无参）
.RunOutput -> { ... 读 .__sse_block_id / .__sse_chunk ... }
.RunResult -> { ... 读 .__sse_status / .__sse_output_text ... }
```

这是为 VM --merged 设计的：renderer（Rust 侧）在触发 handler **之前**，把事件数据
写入 `__sse_*` 预置字段，然后调无参 handler。原因：VM 无法把 struct 作为 handler
参数（`push_value` 对 Obj 推占位 0，见 shell_store.at:72-74 注释）。

vue SSE 的自然形态是**带参**：`EventSource.onmessage` 收到 `data` 对象，想直接
`RunOutput(data)` / `RunResult(data)`。手写版 useShellHttp.ts 的 applyOutput/applyResult
就是带参的（`:139` `function applyOutput(o: CommandOutputPayload)`、`:145` `applyResult(r)`）。

### 子问题 B：SSE 监听代码的"来源"

vue 需要 EventSource 监听代码。它从哪来？

- **来源①（codegen 自动注入）**：解注 api.at 的 stream 端点后，codegen 在 store
  composable 里注入 `new EventSource('/api/stream')` + dispatch（`vue.rs:10541-10575`）
- **来源②（手写）**：像 useShellHttp.ts 那样自己写 connectSSE()

**关键事实**：这两个子问题可以**独立**解决——参数模式（A）和监听来源（B）是正交的。

---

## 1. 三方案的真实改动面（基于已读源码）

### 方案①：vue SSE 桥模拟预置字段模式

**思路**：.at handler 保持无参不变；vue 侧手写 SSE 桥，`onmessage` 里先把 `data`
拆填进 `__sse_*` ref，再调无参 handler。

**源码改动**：

| 改动点 | 文件 | 内容 | 量 |
|---|---|---|---|
| 手写 SSE 桥 | `gen/front/vue/` 下新增或在 App.vue 加一段 | EventSource + 拆填 `__sse_*` + 调 `RunOutput()`/`RunResult()` | ~20-30 行 |
| store 的 .at | `shell_store.at` | **零改动** | 0 |
| api.at stream 端点 | `api.at:237-240` | **不解注**（保持注释，不走 codegen 注入） | 0 |
| VM --merged | — | 不受影响 | 0 |

**生成的 vue store 需要的修补**（R2 的 GAP）：
- GAP-A：Init 里 `git_info.value.git_status.staged` 加可选链（非 git 目录 null 崩溃）
- GAP-B：RunResult 失败分支 `st.message = __sse_status.value` 改成取 `.Failed`
- GAP-端口：vite proxy 改 3000

**手写 SSE 桥长什么样**（参考 useShellHttp.ts:162-178，适配无参 handler）：

```ts
// 在 App.vue 或单独文件
function connectSSE(store) {
  const es = new EventSource('/api/stream')
  es.onmessage = (ev) => {
    try {
      const data = JSON.parse(ev.data)
      if (data.event === 'command_output') {
        // 模拟 VM 的预置字段模式：先填 ref，再调无参 handler
        store.__sse_block_id = data.block_id
        store.__sse_chunk = data.chunk
        store.RunOutput()
      } else if (data.event === 'command_result') {
        const r = data.CommandResult ?? data
        store.__sse_block_id = r.block_id
        store.__sse_cwd = r.cwd
        store.__sse_status = r.status  // 注意 GAP-B：失败时取值要改
        store.__sse_output_text = r.output?.Text ?? ''
        store.__sse_duration_ms = r.duration_ms
        store.RunResult()
      }
    } catch { }
  }
}
```

**好处**：
- ✅ store 的 .at 零改动 → VM --merged 完全不受影响（符合用户约束）
- ✅ 不碰 auto-lang crate → 无外部依赖风险
- ✅ 改动面最小、最可控
- ✅ 手写桥能精细处理 GAP-A/B（类型擦除导致的 null/枚举问题）

**代价**：
- ⚠️ SSE 桥是**手写的**，脱离 codegen 的"类型驱动"——每次 SSE 契约变化要手改
- ⚠️ 手写桥塞进 gen/ 产物里，重新 codegen 会被覆盖——要么放非 gen 目录，要么约定
  作为 post-gen patch。**这是最大的工程瑕疵**
- ⚠️ `__sse_*` 字段暴露在 store 返回值里（现在已经是，见 `useShellStoreStore.ts:140-147`），
  前端组件理论上不该看见这些内部字段

---

### 方案②：codegen 支持 handler 模式分叉

**思路**：解注 api.at stream 端点，让 codegen 自动注入 SSE；同时让 codegen 对
vue target 生成带参 handler、VM target 生成无参 handler。

**但这里有个关键发现**：codegen 的 SSE dispatch 写的是 `RunOutput(data)`（`vue.rs:10568`），
与 handler 签名**无关**——它按 action 名硬调。所以要让 vue 路径带参，需要：

1. **改 .at 的 handler 签名为带参**——但这会让 VM --merged 的 renderer 回流失效
   （renderer 写预置字段、调无参 handler）。所以不能直接改 .at
2. **要么**改 codegen：对 vue target，无参 handler 也接受 dispatch 的 data 参数；
   对 VM target，保持无参。这就是"分叉"

**源码改动**：

| 改动点 | 文件 | 内容 | 量 |
|---|---|---|---|
| codegen 分叉逻辑 | `auto-lang/.../ui_gen/vue.rs:10541-10575` | dispatch 时对无参 handler 先填 `__sse_*` 再调（而非 `RunOutput(data)`） | 中（~20行 rust） |
| api.at stream 端点 | `api.at:237-240` | **解注** | 1 行 |
| shell.at subscribe | `shell.at:443-445` | **解注**（VM 路径，但要确认不触发 VM bug） | 1 行 |
| store 的 .at | `shell_store.at` | **零改动**（分叉在 codegen 里） | 0 |
| store 的 api_imports | `shell_store.at:9` | 加 `stream` 到 use 列表（否则 codegen 不注入，见 vue.rs:10467-10469） | 1 行 |
| VM bug 验证 | — | 确认解注 subscribe 不触发 BUG-A/B/C | 未知风险 |

**这条路其实可以"折中"**：不真改 codegen 做分叉，而是改 codegen 的 dispatch 方式——
让 SSE 注入对**所有** handler 都走"预置字段模拟"（先填 `__sse_*` 再调无参）。
这样 vue 和 VM 路径的 handler 调用方式一致。但这要改 auto-lang，且要保证不破坏
其他项目（forge 有 3 个 SSE stream，vue.rs:10453 注释提到）。

**好处**：
- ✅ SSE 完全自动注入，.at 改动极小（解注 + 加 import）
- ✅ 长期最干净，无手写桥的工程瑕疵
- ✅ 类型驱动，契约变化自动跟随

**代价**：
- 🔴 **要改 auto-lang crate**（外部依赖）——这是最大的代价
- 🔴 要验证解注 shell.at subscribe 不触发 VM BUG-A/B/C（R1 指出这些 bug 未在 master 修）
- 🔴 codegen 分叉逻辑要保证不破坏其他项目（forge 的 3 个 stream）
- 🔴 改了 auto-lang 后，auto-shell 依赖的 auto-lang 版本要同步升级，影响整个编译图
- ⚠️ GAP-A/B 仍要单独修（类型擦除问题与 SSE 注入无关）

---

### 方案③：统一改带参 handler

**思路**：.at 的 handler 改回带参（`RunOutput(data)` / `RunResult(data)`），
VM --merged 的 renderer 也改成推参而非写预置字段。

**源码改动**：

| 改动点 | 文件 | 内容 | 量 |
|---|---|---|---|
| store handler 改带参 | `shell_store.at:141,156` | `RunOutput -> { ... }` 改成 `RunOutput(data) -> { ... }`，body 改读 data | ~15 行 |
| store 去掉预置字段 | `shell_store.at:76-83` | 删 `__sse_*` 字段（或保留兼容） | ~8 行 |
| VM renderer 回流 | auto-lang `renderer.rs` 的 `update_block_in_state` + merged_exec_loop | 从"写预置字段"改成"构造 struct 推参"——**但 VM 不能把 struct 作为参数（原始约束）** | ❌ 受阻 |

**致命问题**：方案③在 VM 路径上**撞回原始约束**——VM 无法把 struct 作为 handler
参数（这正是当初设计预置字段模式的原因，shell_store.at:72）。让 renderer 推 struct 参
会重新触发 `push_value` 对 Obj 推占位 0 的崩溃。

**好处**：
- ✅ 概念上最统一（一份 handler、一种调用方式）

**代价**：
- 🔴 **VM --merged 路径直接受阻**——与用户"VM 维持现状"约束冲突
- 🔴 要改 auto-lang renderer（推 struct 参），且要绕开 VM 的 struct 参数限制
- 🔴 改动面最大（store + renderer + 去预置字段），回归风险最高
- ❌ **建议否决**

---

## 2. 三方案对比矩阵

| 维度 | 方案① 桥模拟 | 方案② codegen 分叉 | 方案③ 统一带参 |
|---|---|---|---|
| **store .at 改动** | 零 | 零（解注 api.at + 加 import） | 改 handler 签名 + 去字段 |
| **VM --merged 影响** | 无 | 需验证（解注 subscribe） | ❌ 受阻（struct 参限制） |
| **改 auto-lang crate** | 不需要 | ✅ 需要（dispatch 分叉） | ✅ 需要（renderer 推参） |
| **SSE 代码来源** | 手写桥 | codegen 自动 | codegen 自动 |
| **工程瑕疵** | 手写桥会被 codegen 覆盖 | 无 | 无 |
| **GAP-A/B 仍要修** | 是（桥里顺手） | 是（单独修） | 是（单独修） |
| **回归风险** | 低（隔离） | 中（动外部 crate + 其他项目） | 高（动 VM 路径） |
| **长期维护性** | 中（手写桥要跟契约） | 高（自动） | —（受阻） |
| **工作量** | 小（~30 行桥 + GAP 修补） | 中（改 codegen + 验证 VM + 升级依赖） | —（不可行） |

---

## 3. 一个被忽略的选项：方案①的变体

方案①最大的瑕疵是"手写桥会被 codegen 覆盖"。但有个变体能规避：

**方案①'：把 SSE 桥放在 App.vue 的 onMounted 里（gen 产物，但 App.vue 由 app.at 生成）**

如果 `app.at` 里有 `onMounted`（已有，App.vue:91-93），且 codegen 保留它，那么
SSE 桥可以写成 app.at 里的初始化逻辑——这样它是 .at 源码的一部分，codegen 会
**持续生成**它，不被覆盖。

但这要求 .at 能表达 EventSource + JSON.parse——这些是浏览器 API，.at 的 codegen
能否透传裸 JS？需要验证。如果 .at 支持 `use.js` 或 raw 表达式，这条路成立。

**若成立，方案①' 优于方案①**：手写桥成为 .at 源码的一部分，无覆盖瑕疵，且仍不碰
auto-lang 的 SSE 注入路径（store 不 import stream → codegen 不注入 → 桥由 app.at 提供）。

---

## 3.5 补充发现：方案⑤（带参 handler 简单参数）——后追加调研

在方案②/③ 分析后，追加了一项关键调研（VM handler 参数约束的真实边界），
发现了一个之前所有方案都漏掉的可能性。

### 核心发现：VM handler 能带多个简单类型参数，只是不能带 struct

`vm_bridge.rs:874-901` 的 `call_handler_for`：参数是 `&[Value]`，多参推送机制
现成。约束只在 `push_value`（`vm_bridge.rs:1003-1016`）：Obj/Array 走 `_ => ram.push_i32(0)`
占位分支，但 int/str/bool/double 正常推送。

**铁证**：同一 store 里 `Complete(str, int)`、`RunCommand(str)`、`NavigateHistory(bool)`
早就在 VM 跑通（shell_store.at:86-90）。还有 `decode_payload`（dynamic.rs:904-936）
的 `\u{1F}` 多参编码机制、`build_handler_args`（http_server.rs:1567-1678）的多参派发先例。

之前 plan 044/ash-gui-native-plan §10.1 的决策只对比了"传整个 struct" vs "无参预置字段"，
**漏掉了"多个简单参数"这个折中**。

### 方案⑤：带参 handler（简单参数）

```
.RunOutput(block_id int, chunk str) -> { ... }
.RunResult(cwd str, status str, output_text str, duration_ms int) -> { ... }
```

- VM target：renderer 改成调带参 handler（用 decode_payload 编码，机制现成）
- vue target：codegen 的 SSE dispatch 改成拆 data 为多参调用
- .at 源码：单一描述，无平台分叉，无 `__sse_*`

### 方案⑤的两个硬伤

1. **要改 VM renderer**：renderer.rs:3961-3982 当前对 command_output/command_result
   **短路了 handler**（直接调 `update_block_in_state` Rust 直写 block，不触发 handler）。
   改带参 handler 需重写这段 → **与"VM 维持现状"约束冲突**
2. **撞第②个 VM bug**：handler body 里 `b.status = BlockStatus{...}` 嵌套 struct 赋值
   在连续调用下 Stack Underflow（plan §10.3）。改带参 handler 不解决它 → handler body
   仍要避免构造嵌套 struct

### 方案⑤ vs ②-b 取舍

| | ②-b 预置字段模拟 | ⑤ 带参 handler |
|---|---|---|
| 符合"维持 VM 现状" | ✅ | ❌ 要改 renderer |
| 消除 __sse_* hack | ❌ | ✅ |
| .at 单一描述 | ❌（仍依赖 __sse_*） | ✅ |
| 通用性 | 差（硬编码映射） | 好（参数即数据源） |
| 风险 | 低 | 高（啃 VM bug） |

**结论**：方案⑤是"架构上最正确"的解，但要破"维持 VM 现状"约束 + 啃 VM bug。
方案②-b 是"务实"的解，符合所有硬约束，代价是 __sse_* hack 长期存在。

---

## 4. 推荐与决策建议

### 决策结果（2026-08-10）

经深入分析（含追加的方案⑤发现），**用户选择方案②（codegen 解决平台差异），
具体落实为 ②-b（预置字段模拟）**。理由：

1. 符合"codegen 解决平台差异"的设计哲学（用户明确主张）
2. 符合"VM --merged 维持现状"硬约束（VM 零改动）
3. 改动局部、风险低（~30 行 codegen + 解注端点）

方案⑤（带参 handler）虽架构更优，但要改 VM renderer + 撞嵌套 struct bug，
违反"维持现状"约束，**登记为技术债**，待未来 VM renderer 改造时统一处理。

**落地计划见 plan 051。**

### 需要用户决策的点

**A. 手写桥放哪里？**（方案① vs ①'）
- 放 gen/ 产物里手改（方案①）：简单，但有 codegen 覆盖瑕疵
- 放 app.at 源码里（方案①'）：干净，但要验证 .at 能否表达 EventSource

**B. 是否接受手写桥脱离 codegen 类型驱动？**
- 若接受 → 方案①
- 若强烈希望 SSE 自动化 → 只能走方案②（改 auto-lang，风险显著上升）

**C. 方案③是否彻底否决？**
- 从技术约束看应该否决（VM 受阻 + 违反"维持现状"）
- 除非未来 VM 修了 struct 参数限制（那是另一个计划）

---

## 附录：关键源码位置索引

### store 源码与产物
- `shell_store.at:141,156`（无参 RunOutput/RunResult handler）
- `shell_store.at:76-83`（`__sse_*` 预置字段定义）
- `shell_store.at:72-74`（无参 handler 设计原因注释）
- `gen/front/vue/src/stores/useShellStoreStore.ts:78,123`（产物里的无参 handler，`__sse_*` ref 无人填）
- `gen/front/vue/src/App.vue:91-93`（onMounted 调 store.Init，无 SSE）

### codegen SSE 注入逻辑（auto-lang）
- `ui_gen/vue.rs:10458-10472`（wire_sse 条件：stream_endpoints + api_imports 含 fn）
- `ui_gen/vue.rs:10541-10575`（EventSource + dispatch 注入，硬调 `RunOutput(data)`）
- `ui_gen/vue.rs:10554-10559`（legacy fallback: command_output→RunOutput / command_result→RunResult）
- `aura/extract.rs:1289-1291`（handler_params 来自 .at handler 签名，无参不记）

### 手写参考实现（vue 版）
- `ash-gui-vue/src/composables/useShellHttp.ts:162-178`（connectSSE 手写桥）
- `ash-gui-vue/src/composables/useShellHttp.ts:139-159`（带参 applyOutput/applyResult，正确处理 GAP）

### 契约
- `api.at:233-240`（stream 端点被注释）
- `shell.at:441-445`（subscribe 被注释）
