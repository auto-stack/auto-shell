# Plan 050: ash-gui-auto 前后台分离 — 可行性调研报告

> **日期**: 2026-08-10
> **性质**: 调研型（产出可行性结论，不直接改实现）
> **状态**: ✅ R1-R3 调研完成；推荐方案待用户拍板
> **背景**: vue 模式下 `ls` 无响应（shell.at run_command 是 no-op）。
> 用户提出：不做 mock，而是把后台命令调用改成走 api.at → ash-server（真实执行），
> 实现 ash-gui-auto 的前后台分离。vue 模式和 VM --no-merge 都走 HTTP；
> VM --merged 维持现状（renderer 拦截，不改 auto-lang crate）。
> **前置探查**: 详见本报告各节证据（文件:行号）

---

## 0. 关键事实纠正

在调研前，先纠正三个影响方案判断的常见误解：

### 0.1 `--merged` / `--no-merge` 标志在本仓库**不存在**

全仓 grep 结果为 0。实际的"模式判别"机制是三者叠加：

| 机制 | 位置 | 作用 |
|---|---|---|
| `pac.at` 默认 `render: "vue"` | `ash-gui-auto/pac.at:4` | a2r codegen 生成 TS/Vue（走 HTTP） |
| `auto run -r vm` / `-r rust` | 命令行 | VM 合并模式（进程内 iced）/ Rust 合并模式 |
| `AUTO_BACKEND` 环境变量 | README 环境变量表 | 空=in-process 合并；有值=HTTP 分离地址 |

**含义**：用户的"前后台分离"语义，在现有机制里对应 `AUTO_BACKEND` 有值（HTTP）；
"VM --merged 维持现状"对应 `auto run -r vm` 且 `AUTO_BACKEND` 为空。
本计划若要引入显式的 `--merged`/`--no-merge` 语义，是**新建**，不是复用。

### 0.2 "auto-shell-core" 实际是两个 crate

| crate | 路径 | 职责 |
|---|---|---|
| `ash-core` | `ash-core/` | 纯逻辑（parser/pipeline/renderer），**不执行命令** |
| `auto-shell` | `ash/auto-shell/` | Shell 引擎 + 80+ 命令（`Shell::execute`） ← 真正执行 `ls` |

`auto-shell/src/lib.rs:14` 把 ash-core re-export 为 `core`，故口语里 "core" 指二者。
**真正执行命令的入口是 `auto_shell::shell::Shell::execute`**（`ash/auto-shell/src/shell.rs:421`）。

### 0.3 ash-server 已经是前后台分离的现成范本

`ash-gui/ash-server/src/worker.rs` 是**前端无关**的 Shell worker：
- 依赖 `ash-core` + `auto-shell`（`Cargo.toml:20-21`）
- Shell 跑在独立 OS 线程，输出经 `tokio::sync::broadcast` 广播 `ShellEvent`
- `http.rs:54,160-172` 提供真实 SSE 端点 `GET /api/stream`（**生效的**，不像 api.at 里被注释）
- `ash-gui-vue` 已通过它实现真正的前后台分离（`useShellHttp.ts:162-178`）

**结论**：分离方案的"后端"不需要新建——ash-server 已就绪。工作集中在**前端接线**。

---

## 1. R1 — codegen 的 SSE 生成能力：✅ 可行（vue 路径无阻塞）

### 核心结论

**解注 api.at 的 SSE 端点后，a2r/Vue codegen 能正确生成可用的前端 SSE 代码，无阻塞。**
类型驱动的 SSE codegen 已于 2026-08-06 合入 auto-lang master（`0f8054af` + merge `c1b05e48`）。

### 证据

**(a) codegen 对 `~Stream<T>` 三目标分流**

| target | 处理 | 位置 |
|---|---|---|
| TS 前端 | stream 端点**不生成 fetch fn**，只生成注释占位；类型 `~Stream<T>` 折叠为 `T` | `auto-lang/.../api/targets/typescript.rs:76-85,405-431` |
| Vue store | **EventSource 注入**到 import 了 `stream` 的 store composable，带 per-path guard 防重复连接 | `auto-lang/.../ui_gen/vue.rs:10458-10575` |
| Axum 后端 | 生成 `Sse<impl Stream<Item=Result<sse::Event,Infallible>>>` handler | `auto-lang/.../api/targets/axum.rs:41-47,187-235` |
| Tauri | `~Stream<T>` 提取内部类型，走 Channel + emit | `auto-lang/.../api/targets/tauri.rs:34-39` |

**(b) api.at 注释归因不准确**

api.at:234-236 写"VM 无法链接 ~Stream<T>"。但 `designs/ash-gui-native-plan.md §9.7`
三轮二分定位真因是 **3 个独立的 VM codegen 缺陷**（BUG-A/B/C），与 `~Stream` 无关：
- BUG-A：store 的 `use back.api` 不透传到 App 作用域（已用 `app.at:13` 自己再 import 绕过）
- BUG-B：store handler 互调时 state-struct 符号未生成（真 VM bug，`vm_bridge.rs`）
- BUG-C：子组件 `expose` + 内部调用的 handler 符号不生成（真 VM bug）

VM **能**处理 `~Stream<T>`：`vm/codegen.rs:1182-1194`、`vm/engine.rs:1349-1374` 有生成器支持；
`plan341_tests.rs:44-70` 验证 VM 内 SSE 消费可用。

**由于 VM --merged 维持现状，这些 VM bug 不进入本计划工作面。**

**(c) ash-server 的 SSE 端点是真实生效的**

`ash-server/src/http.rs:54` 路由 `.route("/api/stream", get(stream_sse))`；
`:160-172` `stream_sse` 调 `state.shell.subscribe()` 拿 broadcast receiver，
`BroadcastStream` + `.filter_map` 包成 SSE frame，`Sse::new(...).keep_alive(...)`。
帧格式：`data: <ShellEvent JSON>\n\n`。

### ⚠️ R1 发现的关键约束（影响实施形态）

**store handler 的"双模冲突"**（详见 §4 核心难点）：

当前 `shell_store.at` 的 `RunOutput`/`RunResult` 是**无参** handler（读 `__sse_*`
预置字段），为 VM --merged 的 renderer 回流设计。而 codegen 的 vue SSE 注入默认做
**带参** dispatch（`RunOutput(data)` / `RunResult(data)`）。两者不匹配。

**若直接解注 api.at stream 端点**：codegen 会生成带参 dispatch 的 EventSource，但
store 的 handler 是无参的 → 运行时不匹配。需要在解注同时处理这个模式分叉。

---

## 2. R2 — api.ts vs ash-server gap：⭐ 小到中等修补（无架构性 gap）

### 核心结论

8 端点的**路径/方法/payload 三要素零差异**（反向生成的必然）。7 个非流式端点基本
直接可用；gap 集中在 SSE 桥缺失 + 类型擦除（Option/枚举），均为已知技术债。

### 逐端点对比（api.ts vs http.rs）

| 端点 | api.ts | http.rs | 形状 | 可用性 |
|---|---|---|---|---|
| command_list | L122-129 GET | L60 | — | ✅ |
| history | L131-138 GET | L67 | — | ✅ |
| complete | L140-148 POST | L77 | `{line,cursor}` | ✅ |
| prompt_context | L150-157 GET | L87 | — | ⚠️ GAP-A 会崩 |
| run_command | L159-166 POST | L100 | `{block_id,cmd}` | ⚠️ 结果靠 SSE |
| run_smart | L168-176 POST | L116 | `{block_id,name,args}` | ✅ |
| cancel | L178-184 POST | L129 | — | ✅ |
| open_path | L186-193 POST | L139 | `{path}` | ✅ |
| **stream** | **无（被注释）** | **L160 GET** | SSE | 🔴 缺失 |

### Gap 清单（按严重度）

**GAP-SSE（🔴 核心，阻断 run_command）**：
api.ts 和生成 store 都没 EventSource 逻辑。`run_command` POST 成功，但结果经
`/api/stream` 回流，前端无人监听 → block 永远停在 Running。
- 需手写 SSE 桥（约 20-30 行，参考 `useShellHttp.ts:162-178`）
- 或解注 api.at + 解决 §4 的 handler 模式问题，让 codegen 自动注入

**GAP-A（🔴 会崩）：PromptContext 可选字段被擦成必填**
- api.at:102-105 / api.ts:74-77：`git_branch: str`, `git_status: GitStatusInfo`（非可选）
- types.rs:52-57：`git_branch: Option<String>`, `git_status: Option<...>`
- 真实 JSON：非 git 目录 → `{"git_branch": null, "git_status": null}`
- 后果：store `useShellStoreStore.ts:43` 直接 `git_status.staged` → null 解引用
- 修补：加可选链 `?.`（手写版 `useShellHttp.ts:31` 用初始值 + try/catch 处理了）

**GAP-B（🔴 丢错误消息）：CommandStatus 枚举擦成 str**
- api.at:120-132：`status: str`（.at 无法表达 `string | object` 联合）
- types.rs:91-96：`#[serde(rename_all="PascalCase")]` extern-tagged 枚举
  → 成功是裸 `"Success"`，失败是对象 `{"Failed":"msg"}`
- 后果：store 失败分支把整个 `{Failed:"msg"}` 塞进 message，取不到真消息
- 修补：失败分支改 `__sse_status.value.Failed`（手写版 `useShellHttp.ts:154` 正确处理）

**GAP-C（🟡 丢语义）：RenderedOutput::Empty 变体无法表达**
- api.at:70-76 用 nullable-key 联合（`Table?: | Text?: | ...`）
- renderer.rs:25-56 是 externally-tagged enum，`Empty` 是 unit variant → 裸字符串 `"Empty"`
- 前端把 Empty 当成"所有字段 null"，能跑但语义丢失
- 另：Table/Record 缺 `atom_type` 字段（renderer.rs:28,37 有，api.at 漏了）

**GAP-端口（🟡 配置）**：
- api.ts 全用相对路径 `/api/...`，无 baseURL，依赖 dev server 代理
- ash-server 监听 `0.0.0.0:3000`（`bin/ash-server.rs:18`）
- gen 的 `vite.config.ts:33` proxy 默认 `8080`，**不是 3000**
- 且 gen dev server 自身默认监听 `3000`（`vite.config.ts:24`），与 ash-server **撞端口**
- 修补：设 `AUTO_HTTP_PORT=3000` + `AUTO_FRONT_PORT=1420` 错开（参考 ash-gui-vue 的 `vite.config.ts:31-36`）

### 工作量定性

**非流式端点：几乎零配置**（7/8 直接通）。
**run_command + SSE：中等修补**（codegen 缺一块，需手写桥或解注+解决 handler 模式）。
**不存在架构性/契约性 gap**——api.at 是从 ash-server 反向生成的，根上对得上。

---

## 3. R3 — 循环依赖：✅ 完全不是阻碍

### 核心结论

**前后台分离方案下，循环依赖完全不是阻碍。** 依赖图本就是单向 DAG，前端与后端
跨进程跨编译单元由 HTTP 解耦。

### 证据

**(a) 依赖方向是单向 DAG，无环**

```
ash-server → auto-shell → auto-lang → auto-val
                                        ↑
                                     ash-core（被 auto-shell re-export 为 core）
```

- `auto-shell/Cargo.toml:16-17`：依赖 auto-lang / auto-val（正向）
- `auto-lang` 全 crate grep `auto-shell`：**零反向依赖**
  （唯一命中 `auto-lang/Cargo.toml:17-18` 是注释掉的 workspace 成员残留）
- `ash-server/Cargo.toml:20-21,37-38`：独立 workspace，依赖 ash-core + auto-shell

**(b) 前端完全不碰 Rust crate**

`gen/front/vue/` 整目录**零个 Cargo.toml**；`package.json:8,11-23` 纯 JS 工具链
（vue/vite/typescript）。`src/lib/api.ts:122-193` 全是 `fetch('/api/...')`。
循环依赖的概念在跨进程边界根本不适用。

**(c) plan 044 的循环依赖痛点被锁死在 VM --merged 路径**

plan 044 M1 原计划让 `.at`（auto-lang VM 进程内）通过 **Rust native shim** 调
`auto_shell::Shell`。shim 要注册进 auto-lang 的 FFI bridge（`ffi.rs:99,127`），
即 shim 代码写进 auto-lang crate 并 `use auto_shell` → 而 auto-shell 已 `use auto_lang`
→ **同一编译单元内互引用 = 循环**。

VM 模式本质进程内（.at 由 auto-lang VM 在同一 iced 进程解释），**无独立 ash-server 进程**，
无法用进程分离规避。plan 044 的出路是降级方案 B（renderer 自解析 stdout）。

**HTTP 链路在 plan 044 中从未被触碰**（`:238` 明确非目标）——因为 vue/HTTP 路径
本就走 ash-server，调 auto-shell 是 plan 042 已验证的能力，没有循环问题。

---

## 4. 核心难点：store handler 的"双模冲突"

这是本方案唯一的真正设计难点。当前 `shell_store.at` 的两个回流 handler 是为
VM --merged 的"renderer 预置字段"模式设计的：

```
// shell_store.at 现状（无参 handler，读预置字段）
.RunOutput {
    // 读 __sse_block_id / __sse_chunk，追加到对应 block 的 streamed_text
}
.RunResult {
    // 读 __sse_status / __sse_output_text / __sse_duration_ms，终结 block
}
```

VM --merged 模式下，renderer（Rust 侧）在触发 handler 前把事件数据写入这些 `__sse_*`
字段（`shell_store.at:71-83` 预置 8 个字段），handler body 无参读它们。
**原因**：VM 无法把 struct 作为 handler 参数（renderer `push_value` 对 Obj 推占位 0）。

但 vue SSE 的事件数据天然是**带参**的（EventSource `onmessage` 收到 `data` 对象，
需要传给 handler）。codegen 的 SSE 注入默认生成 `RunOutput(data)` / `RunResult(data)`
的带参 dispatch（`vue.rs:10541-10575`）。

**同一个 .at handler 定义只有一份，如何同时服务两种模式？**

### 三条出路

**方案①（推荐）：vue SSE 桥模拟预置字段模式**
- vue 侧手写 SSE 桥，`onmessage` 里先把 `data` 写入 `__sse_*` ref，再调**无参** `RunOutput()` / `RunResult()`
- 等于把 VM 的预置字段模式"移植"到 vue 桥
- store 的 .at 定义**零改动**；VM --merged 不受影响
- 不解注 api.at stream 端点（不走 codegen 的 SSE 注入），改用手写桥
- **代价**：SSE 桥是手写的（约 20 行），脱离 codegen 的"类型驱动"——但 R2 已证明这块本就要手写

**方案②：codegen 支持 handler 模式分叉**
- 让 auto-lang codegen 对 vue target 生成带参 handler、VM target 生成无参 handler
- 最"干净"，SSE 完全自动注入
- **代价**：要改 auto-lang crate（外部依赖），风险高，且触及 §1 提到的 VM bug 区

**方案③：统一改带参 handler**
- store 改回有参 handler，VM --merged 的 renderer 也改成推参而非写预置字段
- **代价**：动了 VM 路径——与"VM --merged 维持现状"的用户约束冲突，否决

**推荐方案①**：改动面最小，不碰 VM 路径，不碰外部 crate。代价（手写 SSE 桥）
是无论哪种方案都绕不开的（codegen 的 SSE 注入要么不靠它走手写，要么靠它但要先
改 auto-lang + 解决 handler 模式）。

---

## 5. 推荐方案与下一步

### 推荐：建立实施计划 plan 051，聚焦 vue 模式前后台分离

基于 R1-R3 结论：
- ✅ 技术上可行（SSE codegen 已支持，gap 是小到中等修补）
- ✅ 无循环依赖阻碍
- ✅ 后端（ash-server）已就绪，工作集中在前端接线
- ✅ VM --merged 维持现状，不碰外部 crate

### plan 051 建议范围（供用户确认后展开）

| 任务 | 内容 | 依赖 | 风险 |
|---|---|---|---|
| **M1: 端口与代理配置** | gen 的 vite proxy 指向 ash-server（:3000）；dev server 端口错开 | — | 低 |
| **M2: 非流式端点对接** | 验证 command_list/history/complete/run_smart/cancel/open_path 能直连 ash-server | M1 | 低 |
| **M3: GAP-A 修补** | PromptContext 可选字段加可选链（防 null 解引用崩溃） | M2 | 低 |
| **M4: GAP-B 修补** | CommandStatus 失败分支取值修正 | M2 | 低 |
| **M5: SSE 桥（方案①）** | 手写 EventSource 桥，模拟预置字段模式，dispatch 到无参 handler | M2-M4 | 中 |
| **M6: 端到端验证** | vue 模式下 `ls` 等命令真实出结果（Table/Text 渲染） | M5 | — |
| **(可选) M7: GAP-C/D** | RenderedOutput::Empty 容错；Table 补 atom_type | M6 | 低 |

**验证标准**：vue 模式启动后，输入 `ls` → ash-server 真实执行 → 结构化输出回流渲染。

### 待用户决策

1. **是否建立 plan 051**（本报告只调研，不实施）
2. **store 双模选哪个方案**（本报告推荐方案①）
3. **是否在本报告后再开 plan 051**，还是先把 050 归档、051 另起

---

## 附录：关键文件索引

### 契约与生成产物
- `ash-gui-auto/src/back/api.at`（契约：类型 L24-158，stream 端点注释 L233-240）
- `ash-gui-auto/src/back/shell.at`（VM mock 后端：run_command no-op L424-426，subscribe 注释 L441-445）
- `ash-gui-auto/src/front/shell_store.at`（store：`__sse_*` 字段 L76-83，无参 handler L141-192）
- `ash-gui-auto/gen/front/vue/src/lib/api.ts`（生成 TS client，8 个 fetch fn）
- `ash-gui-auto/gen/front/vue/src/stores/useShellStoreStore.ts`（生成 store，SSE ref 无人填）
- `ash-gui-auto/gen/front/vue/vite.config.ts`（proxy 默认 8080，需改 3000）

### 真实后端
- `ash-gui/ash-server/src/http.rs`（router L42-56，run_command L100，stream_sse L160-172）
- `ash-gui/ash-server/src/types.rs`（PromptContext L52-57，CommandStatus L91-96，ShellEvent L115-122）
- `ash-gui/ash-server/src/worker.rs`（前端无关 Shell worker + broadcast）
- `ash-gui/ash-server/src/bin/ash-server.rs:18`（监听 :3000）

### 渲染与命令执行
- `ash-core/src/renderer.rs`（RenderedOutput enum L25-56，RenderedCell L120-136）
- `ash/auto-shell/src/shell.rs:421`（Shell::execute 主入口）

### 可参考的完整手写实现（vue 版）
- `ash-gui/ash-gui-vue/src/composables/useShellHttp.ts`（SSE 桥 L162-178，所有 gap 已处理）
- `ash-gui/ash-gui-vue/vite.config.ts:31-36`（proxy 已配 3000）

### codegen 源码（auto-lang，外部 crate）
- `auto-lang/crates/auto-lang/src/api/targets/typescript.rs:76-85,405-431`（Stream→TS）
- `auto-lang/crates/auto-lang/src/ui_gen/vue.rs:10458-10575`（EventSource 注入）
- `auto-lang/crates/auto-lang/src/api/targets/axum.rs:41-47,187-235`（Stream→Axum）
- `auto-lang/crates/auto-lang/src/vm/codegen.rs:1182-1194`（VM 生成器检测）

### 历史教训
- `docs/plans/old/044-vm-backend-alignment.md:14-17,43-49,74-82,117-120`（循环依赖场景）
- `designs/ash-gui-native-plan.md §9.7`（BUG-A/B/C 定位）
