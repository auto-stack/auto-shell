# ash-gui-auto

Auto(.at)版本的 ash-gui —— 用 [auto-lang](../../../../../auto-lang) 的 iced
原生渲染器跑 ash-gui。VM 模式(开发/测试)+ a2r 模式(可分发二进制,开发中)。

## 运行

### VM 模式(开发 + 测试,推荐)

VM 模式在运行时解析 `.at` → `DynamicComponent` → iced 窗口,热重载快,无需代码生成。

```bash
# 前置:auto-lang 已编译(auto 二进制可用)
# 默认 auto 二进制:auto-lang/target/debug/auto.exe
auto run -r vm
```

打开 iced 窗口(标题 "Auto - App"),MCP UI 服务端监听 `http://127.0.0.1:9247/mcp`。

> `pac.at` 的 `render: "vue"` 是默认值;VM 模式用 `-r vm` 命令行参数覆盖,不改 pac.at。

### A2R 模式(可分发二进制,开发中)

A2R 模式生成 Rust `main.rs` + `Cargo.toml` → `cargo run` → 独立 iced 二进制。

```bash
auto run -r rust
```

**当前状态(2026-08-07):不可用。** 生成的 Rust 代码有 ~99 个编译错误(a2r codegen
对 store-composable / 嵌套 struct / List 方法的系统性缺陷)。详见
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

**总计:49 pass + 47 skip + 3 xfail**。skip 主要是难档(M2 未做的
ghost/highlight/textarea/debounce)+ mock 数据空;a2r 修复后更多可转 pass。

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
