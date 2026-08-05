# Plan 042: ash-gui 统一后端架构 — 浏览器版与 Tauri 版行为一致

> **日期**: 2026-08-05
> **状态**: 📝 计划(待实施)
> **来源**: Plan 039-041 完成后的架构审查——浏览器版(Vue)与 Tauri 版(桌面 App)行为不一致
> **范围**: 新建 `ash-server` crate + 重构 `ash-gui-vue` 前端 API 层
> **前置**: Plan 041 已完成(补全引擎下沉,前端体验齐备)
> **预估**: M1-M5,~800 行 Rust + ~300 行 TS
> **参照**: `auto-lang/examples/ui/015-notes` 的 AutoUI 统一模型(`#[api]` → axum + tauri + TS 三目标 codegen)

---

## 0. 背景:为什么需要这个计划

### 问题

ash-gui-vue 当前有**两种启动方式**,但行为不一致:

```
npm run dev        → 浏览器打开 localhost:1420 → useShellMock()(硬编码假数据)
npm run tauri dev  → Tauri WebView            → useShell()(真实 Rust 后端)
```

`App.vue` 用 `'__TAURI_INTERNALS__' in window` 判断环境,选择 mock 或真实后端。浏览器版
无法运行 Rust Shell 引擎,只能用假数据——导致两版**行为不一致**(命令执行、补全、历史、
git 状态全是假的)。

### 根因

Plan 039 手写了 `#[tauri::command]` 函数(Tauri IPC 专用),跳过了 AutoUI 的统一 API 层。
AutoUI 的正规模型(见 `015-notes`)是:一份 `#[api]` 定义 → codegen 生成三个目标——
**Axum 路由**(HTTP server)、**Tauri command**(IPC)、**TypeScript client**(前端)——
让浏览器版连 HTTP、Tauri 版连 IPC,共用同一份业务逻辑。

我们没这么做是因为 Plan 039 是"先跑起来"的 MVP。现在 GUI 功能已齐备(Plan 040-041),
是时候补上架构债。

### 目标

浏览器版和 Tauri 版**行为和 UI 完全一致**:同一个 Shell 引擎、同一份 API 定义、同一个前端
`api.ts`,只是传输层不同(HTTP vs Tauri IPC)。`useShellMock` 删除。

---

## 1. AutoUI 统一模型(参照 015-notes)

`auto-lang` 的 `api/targets/` 有三个 codegen 目标:

| 目标 | 生成物 | 用途 |
|---|---|---|
| `AxumGenerator` | Rust axum 路由(`Router::new().route(...)`) | HTTP server,浏览器版连它 |
| `TauriGenerator` | Rust `#[tauri::command]` | Tauri IPC,Tauri 版连它 |
| `TypeScriptGenerator` | TS `fetch('/api/...')` client | 前端两边共用 |

一份 `#[api(method, path)]` 注解的函数,三个目标各取所需。`015-notes` 的 `src/back/api.at`
定义了 `list_notes`/`create_note`/...,前端 `api.ts` 用 `fetch('/api/notes')` 调用——
浏览器版打到 axum server,Tauri 版打到内嵌的同一个 server(或 Tauri command)。

### ash-gui 的特殊性

ash-gui 不是典型的 CRUD app(015-notes 是)。它的 API 是**交互式 Shell**——有流式输出
(command-output 事件)、取消(cancel)、长连接(命令可能跑很久)。所以不能简单套用
015-notes 的 request-response 模式,需要:

- **Request-response 命令**:`command_list`/`history`/`complete`/`prompt_context`/`run_smart_command`
- **流式命令**:`run_command` → 逐行 `command-output` 事件 + 最终 `command-result`(需要 SSE 或 WebSocket)
- **取消**:`cancel_command`(无返回,设置 flag)

---

## 2. 架构设计

### 总体结构

```
                        ┌─────────────────────────────────┐
                        │        ash-server (新 crate)      │
                        │  ┌─────────────────────────────┐ │
                        │  │  ShellApi trait(共享接口)    │ │
                        │  │  run / complete / cancel /   │ │
                        │  │  history / prompt_context    │ │
                        │  │  command_list / run_smart    │ │
                        │  └──────────┬──────┬───────────┘ │
                        │             │      │             │
                        │     ┌───────┘      └────────┐    │
                        │     ▼                       ▼    │
                        │  axum routes          tauri cmds  │
                        │  (HTTP + SSE)       (#[command])  │
                        │  + WS(流式)          + events      │
                        └─────────┬─────────────────┬──────┘
                                  │                 │
                    HTTP/SSE      │                 │  Tauri IPC
                 ┌────────────────┘                 └──────────────┐
                 ▼                                                 ▼
        ┌─────────────────┐                          ┌──────────────────────┐
        │  浏览器版         │                          │  Tauri 版             │
        │  npm run dev     │                          │  npm run tauri dev   │
        │  useShellHttp()  │                          │  useShellTauri()     │
        │  → fetch + SSE   │                          │  → invoke + listen   │
        └─────────────────┘                          └──────────────────────┘
                            ↑ 同一份 api.ts(类型 + 接口) ↑
```

### 新 crate: `ash-server`

位置:`ash-gui/ash-server/`(与 `ash-gui-vue` 平级)。

职责:
1. 定义 `ShellApi` trait——所有 Shell 操作的统一接口(不依赖 axum 或 tauri)
2. 持有 `ShellWorker`(复用现有 `shell_worker.rs` 的逻辑:Shell 线程 + channel)
3. 提供 axum 路由实现(HTTP/SSE/WS 端点,调 `ShellApi`)
4. 提供 Tauri command 实现(`#[tauri::command]`,调 `ShellApi`)

`ash-gui-vue/src-tauri` 改为依赖 `ash-server`,只保留 Tauri 启动壳(`lib.rs` 注册命令 +
spawn worker);`shell_worker.rs`/`commands.rs` 的逻辑移到 `ash-server`。

### 前端:统一 `api.ts`

`useShell.ts` 拆成:
- `api.ts`——纯类型 + 接口定义(所有前端共用的 `ShellApiClient` 接口)
- `useShellHttp.ts`——HTTP 实现(`fetch` + SSE/WS,浏览器版用)
- `useShellTauri.ts`——Tauri 实现(`invoke` + `listen`,Tauri 版用)
- `useShell.ts`——根据环境选一个(保留原入口,`App.vue` 不变)

`useShellMock.ts` 删除。

---

## 3. API 清单(从现有后端提取)

现有 8 个 `#[tauri::command]` + 2 个事件,映射到统一 API:

| API 方法 | 传输 | 现有来源 | 流式? |
|---|---|---|---|
| `command_list()` → `BootSnapshot` | request-response | `command_list` command | 否 |
| `history()` → `Vec<String>` | request-response | `history` command | 否 |
| `complete(line, cursor)` → `Vec<CompletionItem>` | request-response | `complete` command | 否 |
| `prompt_context()` → `PromptContext` | request-response | `prompt_context` command | 否 |
| `run_command(block_id, cmd)` | fire-and-forget | `run_command` command | 否(触发流式) |
| `run_smart(block_id, name, args)` → `String` | request-response | `run_smart_command` command | 否 |
| `cancel()` | fire-and-forget | `cancel_command` command | 否 |
| `open_path(path)` | fire-and-forget | `open_path` command | 否 |
| **`command-output` 事件** | **SSE / WS** | `command-output` emit | **是**(逐行 chunk) |
| **`command-result` 事件** | **SSE / WS** | `command-result` emit | **是**(最终结果) |

### 流式传输方案选择

`run_command` 的流式输出是核心挑战。两种方案:

**方案 A:SSE(Server-Sent Events)** — 推荐
- 前端 `new EventSource('/api/shell/stream')` 建立长连接
- 后端逐行推送 `command-output` / `command-result` 事件
- 浏览器和 Tauri WebView 都原生支持 SSE
- axum 有 `Sse` 响应类型;Tauri 版用 `listen` event
- 简单、单向(服务器→客户端)、自动重连

**方案 B:WebSocket** — 更灵活但更复杂
- 双向,适合未来做交互式终端(PTY)
- 当前不需要双向(命令通过 POST 发送,输出通过 WS 接收)
- 留待 Plan(真终端模拟)时考虑

**决策:用 SSE(方案 A)**。当前流式是单向的(服务器推输出到前端),SSE 足够且更简单。
Tauri 版继续用 `listen`(Tauri event),浏览器版用 SSE——传输不同,数据格式相同。

---

## 4. 详细设计

### M1: `ash-server` crate 骨架 + `ShellApi` trait

新建 `ash-gui/ash-server/`:
- `Cargo.toml`——依赖 `auto-shell`、`ash-core`、`axum`、`tokio`、`serde`
- `src/lib.rs`——`ShellApi` trait + `ShellWorker`(从 `shell_worker.rs` 提取)
- `ShellApi` trait:
  ```rust
  pub trait ShellApi: Send + Sync {
      fn command_list(&self) -> BootSnapshot;
      fn history(&self) -> Vec<String>;
      async fn complete(&self, line: String, cursor: usize) -> Vec<CompletionItem>;
      async fn prompt_context(&self) -> PromptContext;
      fn run_command(&self, block_id: usize, cmd: String);
      async fn run_smart(&self, block_id: usize, name: String, args: Vec<String>) -> SmartResult;
      fn cancel(&self);
      fn open_path(&self, path: String);
      /// 订阅流式输出(command-output / command-result 事件)。
      /// HTTP 版用 SSE;Tauri 版用 Tauri events。返回一个 receiver。
      fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ShellEvent>;
  }
  ```
- `ShellEvent` 枚举:`CommandOutput { block_id, chunk }` / `CommandResult { ... }`

验收:`ash-server` 编译通过,`ShellApi` trait 定义完整。

### M2: axum HTTP 路由实现

`ash-server/src/http.rs`:
- `create_api_router(api: Arc<dyn ShellApi>) -> Router`
- REST 端点(对应 API 清单的 request-response 项):
  - `GET  /api/command_list`
  - `GET  /api/history`
  - `POST /api/complete`       body: `{ line, cursor }`
  - `GET  /api/prompt_context`
  - `POST /api/run_command`    body: `{ block_id, cmd }`
  - `POST /api/run_smart`      body: `{ block_id, name, args }`
  - `POST /api/cancel`
  - `POST /api/open_path`      body: `{ path }`
- SSE 端点:
  - `GET /api/stream` → `Sse` 流,推送 `ShellEvent`(序列化为 SSE `event:` / `data:` 帧)
- `main.rs`——`ash-server` 二进制:启动 axum server(`0.0.0.0:3000`) + ShellWorker

验收:`ash-server` 二进制能启动,`curl localhost:3000/api/command_list` 返回 JSON。

### M3: Tauri command 实现(迁移)

`ash-server/src/tauri.rs`:
- 每个 `ShellApi` 方法包装成 `#[tauri::command]`
- `subscribe` 对应 Tauri events(`app.emit("command-output", ...)` / `app.emit("command-result", ...)`)
- `ash-gui-vue/src-tauri/src/lib.rs` 改为依赖 `ash-server`,注册这些命令

验收:Tauri 版行为不变(现有 Plan 040-041 功能全部正常)。

### M4: 前端 `api.ts` 统一 + HTTP 实现

- `src/lib/api.ts`——类型定义(`BootSnapshot`/`CompletionItem`/`PromptContext`/`ShellEvent` 等)
- `src/composables/useShellHttp.ts`:
  - `fetch('/api/command_list')` 等 REST 调用
  - `new EventSource('/api/stream')` 接收流式事件 → 更新 blocks
- `src/composables/useShellTauri.ts`:现有 `useShell.ts` 逻辑(改名)
- `src/composables/useShell.ts`:根据 `__TAURI_INTERNALS__` 选 http 或 tauri
- `vite.config.ts`:dev proxy(`/api` → `localhost:3000`),让浏览器版连 ash-server

验收:浏览器版 `npm run dev` 连 ash-server,`ls` 返回真实目录内容。

### M5: 删除 mock + 验证对等

- 删除 `useShellMock.ts`
- 验收矩阵(浏览器版 vs Tauri 版,逐项对比):

| 功能 | 浏览器版 | Tauri 版 |
|---|---|---|
| `ls` 真实目录 | ✅ | ✅ |
| `cat file` | ✅ | ✅ |
| 补全(flag/路径) | ✅ | ✅ |
| 历史导航 ↑↓ | ✅ | ✅ |
| Ctrl+R 搜索 | ✅ | ✅ |
| ghost text | ✅ | ✅ |
| git 分支显示 | ✅ | ✅ |
| 流式输出 | ✅(SSE) | ✅(Tauri event) |
| 取消命令 | ✅ | ✅ |
| SmartCommand | ✅ | ✅ |

两版行为完全一致。

---

## 5. 里程碑与验证

| 里程碑 | 内容 | 验证 |
|---|---|---|
| M1 | `ash-server` crate + `ShellApi` trait | crate 编译,trait 完整 |
| M2 | axum HTTP 路由 + SSE | `curl localhost:3000/api/command_list` 返回 JSON |
| M3 | Tauri command 迁移 | Tauri 版行为不变(Plan 040-041 全通) |
| M4 | 前端 api.ts + HTTP 实现 + proxy | 浏览器版 `ls` 返回真实目录 |
| M5 | 删 mock + 对等验证 | 浏览器版 = Tauri 版(逐项对比) |

M1-M3 是后端,M4-M5 是前端。建议 M1→M2→M3→M4→M5 顺序做。

---

## 6. 风险与回退

| 风险 | 缓解 |
|---|---|
| Shell 是 `!Send`(auto-lang VM 用 `Rc`),HTTP server 是多线程 | Shell worker 留在独立线程(现有架构),axum handler 通过 channel 与它通信(同 Tauri) |
| SSE 在 Tauri WebView 的兼容性 | Tauri 版不用 SSE(继续用 Tauri event);SSE 只服务浏览器版 |
| 流式输出的 block_id 关联(HTTP 无状态) | block_id 由前端生成,通过 `run_command` 传给后端,SSE 事件携带同一 block_id |
| ash-server 需要独立启动(浏览器版) | `npm run dev:web` 脚本同时起 ash-server + vite;或 vite plugin 自动起 |

---

## 7. 与现有计划的关系

- **Plan 039**(ash-gui-vue M1-M4):手写 Tauri 原型 → 本计划将其后端提取到 ash-server
- **Plan 040**(后端差距 M1-M6):`shell_worker.rs` 的逻辑 → 移入 `ash-server` 的 `ShellApi` 实现
- **Plan 041**(前端差距 M1-M8):前端组件不变,只换数据源(`useShellMock` → `useShellHttp`)

**不改动**:auto-shell/ash-core 的核心逻辑、Plan 041 的前端组件(PromptBar/BlockList/等)、
TUI/CLI 版本。

## 8. 参考文件

- `auto-lang/examples/ui/015-notes/src/back/api.at`(`#[api]` 定义范例)
- `auto-lang/crates/auto-lang/src/api/targets/{axum,tauri,typescript}.rs`(三目标 codegen)
- `ash-gui/ash-gui-vue/src-tauri/src/shell_worker.rs`(现有 Shell worker,逻辑来源)
- `ash-gui/ash-gui-vue/src-tauri/src/commands.rs`(现有 8 个 Tauri command)
- `ash-gui/ash-gui-vue/src/composables/useShell.ts`(现有 Tauri 前端接入)
- `ash-gui/ash-gui-vue/src/composables/useShellMock.ts`(待删除的 mock)
