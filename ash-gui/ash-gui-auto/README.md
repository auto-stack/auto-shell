# ash-gui-auto

Auto(.at)版本的 ash-gui。三种运行形态:

- **VM+HTTP 模式**(日常使用,推荐)—— iced 原生窗口 + [ash-server](../ash-server) 真 ash-core 后端,`run_vm.ps1`/`run_vm.sh` 一键启动;
- **VM merged 模式**(开发/测试)—— 单进程,`#[api]` 走 `back/shell.at` mock,命令由 renderer 侧 `std::process` 执行(无 ash-core 语义);
- **Vue/浏览器模式** —— `auto gen` 生成 Vue 前端,浏览器里跑,连同一个 ash-server;
- **a2r 模式**(可分发二进制,开发中)—— 生成 Rust → 独立 iced 二进制。

## 运行

### VM merged 模式(日常使用,推荐 —— Plan 061 外部后端)

**一条命令**:`run_vm.ps1` / `run_vm.sh` → `auto run -r vm`。pac.at 的
`back: { project: "../ash-server" }` 让宿主把 back.* 契约链接解析到外部
后端项目并装载其 cdylib(api.at 10 端点直连 ash-core,进程内,无 HTTP)。
命令执行、补全、历史、git 标签、jobs 全部走真 ash-core 会话,结构化输出
(Table/kind 着色)由 ash-server 真实渲染。前置:auto-lang 主检出与
ash-server cdylib 已构建(脚本会自动构建后者)。

### 旧:VM+HTTP 模式(Plan 057,仍在)

```powershell
# Windows
powershell -ExecutionPolicy Bypass -File run_vm.ps1
# 复用已在跑的 ash-server / 指定端口 / 指定 auto 二进制
.\run_vm.ps1 -NoServer -Port 3000 -AutoBin D:\autostack\auto-lang\target\debug\auto.exe
```

```bash
# Linux/macOS/Git Bash
./run_vm.sh            # ./run_vm.sh -p 3000 -n(复用 server)
```

等价的手动方式(脚本做的事):

```bash
# 终端 1:ash-server(真 ash-core 后端)
cd ash-gui/ash-server && cargo run            # → http://localhost:3000

# 终端 2:VM,#[api] 调用直连 ash-server
cd ash-gui/ash-gui-auto
AUTO_BACKEND=http://127.0.0.1:3000 auto run -r vm
```

Plan 057 起 `AUTO_BACKEND` 非空即为**一等模式**:VM codegen 把 `#[api]` 裸名调用
(run_command/complete/history/prompt_context/jobs/…)编译为 HTTP(原先仅
`--no-merge` 启用且 URL 硬编码),renderer 只做 SSE 泵。job 事件(job_started/
job_done)、cwd 回写、git 刷新、真实 exit_code 均经 SSE/桥接层生效。
MCP UI 服务默认 `:9247`,冲突时用 `AUTOUI_MCP_PORT` 避让。

### VM merged 模式(开发 + 测试)

VM 模式在运行时解析 `.at` → `DynamicComponent` → iced 窗口,热重载快,无需代码生成。

```bash
# 前置:auto-lang 已编译(auto 二进制可用)
# 默认 auto 二进制:auto-lang/target/debug/auto.exe
auto run -r vm
```

打开 iced 窗口(标题 "Auto - App"),MCP UI 服务端监听 `http://127.0.0.1:9247/mcp`。
此模式下补全/git/jobs 是 `back/shell.at` 的静态 mock,命令执行不经过 ash-core
(别名/管道 DSL/结构化渲染均无)—— 仅适合 UI 迭代;要真 shell 能力用上面的
VM+HTTP 模式。

> `pac.at` 的 `render: "vue"` 是默认值;VM 模式用 `-r vm` 命令行参数覆盖,不改 pac.at。

### Vue/浏览器模式(连 ash-server 真后端)

`pac.at: render: "vue"` 时,`auto gen` 把 Vue 前端生成到 `gen/front/vue/`。浏览器版与 [ash-gui-vue](../ash-gui-vue) 共用同一个后端 —— **[ash-server](../ash-server)**(独立 HTTP 服务,包装 ash-core,默认监听 `:3000`)。

> ⚠️ **不要用 `auto run` 跑浏览器版真后端**:`auto run`(默认 vue)会自动构建并运行 codegen 生成的桩后端 `examples/rust-workspace/ash-gui-auto-back`(只返空默认值的脚手架,**不执行命令**),并把 vite 的 `/api` 代理到它(默认 `:8080`)。要跑真命令,必须手动起 ash-server,并让 vite 代理过去。

**前置**:`auto gen` 已生成 `gen/front/vue/`,且装好依赖(在 `gen/front/vue/` 跑 `npm install` 或你用的包管理器)。

两个终端启动:

```bash
# 终端 1 — 真 ash-core 后端(ash-server)
cd ash-gui/ash-server && cargo run            # → http://localhost:3000

# 终端 2 — Vue 前端开发服务器(vite),/api 代理到 ash-server
cd ash-gui/ash-gui-auto/gen/front/vue
AUTO_FRONT_PORT=5173 AUTO_HTTP_PORT=3000 npm run dev   # → http://localhost:5173
```

浏览器打开 **http://localhost:5173** 。`.at` 改动后重跑 `auto gen` 刷新 `gen/`(vite HMR 只热更 gen 产物,不会重新生成)。

`/api` 代理由 `gen/front/vue/vite.config.ts` 控制(运行时读环境变量,**无需重生成**):

| 变量 | 默认 | 说明 |
|---|---|---|
| `AUTO_FRONT_PORT` | `3000` | vite 端口。ash-server 已占 3000,浏览器版务必改成别的(如 `5173`) |
| `AUTO_HTTP_PORT` | `8080` | `/api` 代理目标端口。连 ash-server 设为 `3000` |
| `AUTO_HTTP_PROXY` | (空) | 完整代理目标 URL,优先于 `AUTO_HTTP_PORT`(如 `http://localhost:3000`) |

> ash-server 是手写的独立 crate(`ash-gui/ash-server`),不在 `auto gen` 产物范围;改后端逻辑直接改它源码再 `cargo run` 即可。

### A2R 模式(可分发二进制,开发中)

A2R 模式生成 Rust `main.rs` + `Cargo.toml` → `cargo run` → 独立 iced 二进制。

```bash
auto run -r rust --server rust   # merged mode: 后端 in-process
```

**当前状态(2026-08-08 二次实测):不可用。** codegen 阶段成功,但 `cargo run`
编译失败(exit 101),**72 个错误全在前端 `main.rs`**。`--server rust` 走 merged mode,
后端 in-process 不再单独编译(旧的后端 17 错被绕过非修复)。根因是 a2r codegen 系统性
缺陷:误译 `View` 表格 API(`thead/tr/td` 在 `auto_lang::View` 不存在)、store-composable/
嵌套字段映射到弱类型 `serde_json::Value`、跨组件符号作用域泄漏。详见归档文档 §4 与
`designs/ash-gui-native-plan.md` M4 备注。VM 模式是当前唯一可用路径。

## MCP UI 服务端

VM 模式启动后,iced 进程内嵌一个 HTTP MCP 服务端(JSON-RPC 2.0):

- **地址**:`http://127.0.0.1:9247/mcp`
- **12 个 `autoui_*` 工具**:snapshot / inspect / action / check / screenshot /
  state / wait / type / keyboard / vtree / find / exists

用于 AI agent 或测试套件观察/操控 UI。

### 快速验证连通性

```bash
# 列出工具(应返回 12 个 autoui_* 工具)
curl -s -X POST http://127.0.0.1:9247/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/list","params":{},"id":1}'
```

## 测试

测试套件用 pytest + MCP 客户端(`tests/desktop_mcp.py`)驱动 VM 模式 UI。

### 运行测试

```bash
cd ash-gui/ash-gui-auto

# 用 worktree 编译的 auto(auto-lang 改动在此 worktree)
AUTO_BIN=/path/to/auto-lang/.worktrees/auto-shell/target/debug/auto.exe \
  python -m pytest tests/ -v

# 只跑命令执行测试
AUTO_BIN=... python -m pytest tests/test_command_exec.py -v

# 只跑 smoke(最快,验证 vm 启动 + MCP 连通)
AUTO_BIN=... python -m pytest tests/test_smoke.py -v
```

### 环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `AUTO_BIN` | `auto-lang/target/debug/auto.exe` | `auto` 二进制路径 |
| `AUTOUI_MCP_PORT` | `9247` | MCP 服务端口 |
| `AUTO_RUN_TIMEOUT` | `30` | MCP 启动等待秒数 |
| `AUTO_BACKEND` | (空) | HTTP 模式后端地址(如 `http://127.0.0.1:3000`);空=merged in-process |

### 测试覆盖(99 用例)

| 文件 | 行为编号 | 结果 |
|---|---|---|
| test_smoke.py | 基础设施 | 6 pass |
| test_command_exec.py | M1 命令执行 | 2 pass |
| test_app_shell.py | APP-01..15 | 9 pass + 4 skip |
| test_command_lifecycle.py | CMD-01..12 | 7 pass + 5 skip |
| test_block.py | BL-01..18 + TS/git | 11 pass + 6 skip |
| test_blockbody.py | BB-01..14 | 3 pass + 10 skip |
| test_tool_sidebar.py | TS-01..05 | 2 pass + 3 skip |
| test_backend.py | BACK-01..12 | 6 pass + 7 skip |
| test_prompt_input.py | PB-01..15 | 3 pass + 12 skip |
| test_history_search.py | HS-01..13 | 3 xfail |

**总计(2026-08-23 实测,R16 桥修复后):63 pass + 44 skip,零失败**(59 既有 +
4 项 test_block_interactions 新回归;skip 含 MCP 键盘每实例竞态项,真实键盘
已验证正常 —— 见 plan 060 第十六轮)。skip 主要是难档(M2 未做的
ghost/highlight/textarea/debounce)+ mock 数据空;a2r 修复后更多可转 pass。

> 注:测试矩阵表按文件分项仍为历史数据;以 `pytest -q` 实测总计为准
> (2026-08-23:63 pass + 44 skip,零失败;端口避让 `AUTOUI_MCP_PORT`)。

## 架构

```
src/
  front/          # UI 组件(.at)
    app.at          根布局(侧栏 + 标题 + BlockList + PromptBar)
    shell_store.at  Shell 状态管理 store(命令执行/流式/git)
    prompt_bar.at   命令输入栏
    block_list.at   block 列表
    block_item.at   单个 block
    block_body.at   输出渲染器(Table/Code/Text/Error/Record)
    history_search.at  Ctrl+R 历史搜索
    tool_sidebar.at 侧栏工具列表
    types.at        类型定义
  back/           # 后端契约 + mock
    api.at          API 端点定义(#[api] codegen 契约)
    shell.at        VM 模式 in-process mock 后端
```

### SSE 流式桥(VM 模式命令执行)

VM 模式无法执行系统进程(.at 是纯逻辑),命令执行在 **renderer 侧的 Rust 执行器线程**:
`type ls + submit` → store.RunCommand(记 pending)→ renderer 构造 block + 提交执行器
(std::process::Command)→ command_output/command_result 事件回流 → renderer 更新 block。

详见 `designs/ash-gui-native-plan.md` §10。

## 设计文档

- [`designs/ash-gui-native-plan.md`](../../designs/ash-gui-native-plan.md) — 总计划(M0..M4)
- [`designs/ash-gui-native-archived.md`](../../designs/ash-gui-native-archived.md) — 架构归档(SSE 桥 / 差异清单 / 测试矩阵 / a2r 缺陷)
