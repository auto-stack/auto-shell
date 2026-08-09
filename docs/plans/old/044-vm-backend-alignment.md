# Plan 044: VM 后端对齐 — shell.at 从 mock 升级为 ash-core 真实后端

> **日期**: 2026-08-09
> **状态**: 🔲 **规划中**(计划已批准,待实施)
> **来源**: EDGE-16 修复链完成后(EDGE-15/16a-f + EDGE-04-B,auto-lang master),
> VM 版命令执行 + block 渲染 + computed + 布局全通。剩余两个对齐 vue 版的差距:
> 侧栏只有 1 命令(mock)、ls 输出纯 Text(非结构化 Table)。
> **范围**: shell.at 从 mock 升级为真实后端(接 ash-core 引擎),renderer 拆除短路
> **前置**: EDGE-16 全链已修(auto-lang master);VM 支持 use.rust FFI(已验证)
> **参照**: ash-server/src/worker.rs(vue 版后端)、back/api.at(契约)

---

## 0. 背景:为什么要做后端对齐

### 目标

ash-gui-auto 的 VM merged 模式当前用 `back/shell.at`(纯 .at mock)作后端。mock 太
简陋:command_list 只返回 1 命令(ls);run_command 是 no-op;命令执行被 renderer 短路
(直接 std::process,返回纯文本)。

vue 版用 ash-server(Rust,调真 ash-core),返回 80 命令 + 结构化 Table。

本计划目标:让 shell.at 通过 ash-core 引擎执行命令,返回完整命令列表 + 结构化
RenderedOutput(Table/Text/Code/Record/Error),完全对齐 vue 版。

### 架构(三者同一契约)

```
api.at (契约: #[api] 函数,调 shell.xxx())
  use shell
    ├─ VM merged:  shell.at(纯 .at,VM 直接执行)  ← 本计划升级目标
    ├─ HTTP:       生成 HTTP client → ash-server(调 ash-core)
    └─ Tauri:      生成 Tauri command → ash-server(同上)
```

### 当前短路问题

renderer.rs 的 `merged_exec_loop`(:2248)绕过 shell.at,直接 std::process 执行,
output 永远 null(:2373)。shell.at 的 run_command 是 no-op。store 用 streamed_text
作 Text(:165-171)。完整 RenderedOutput 渲染被有意跳过(:2362 注释)。

## 1. 技术选型(已确认)

- **执行架构**: 方案 C — 接 ash-core 引擎(`auto_shell::Shell`)
- **command_list**: 动态 — use.rust 调 ash-core registry

### 关键约束

VM 的 .at **无法实现 Rust trait**(RenderHook)。ash-server 用 `set_render_hook
(CaptureHook)` 捕获 RenderedOutput,但 CaptureHook 需 `impl RenderHook`——.at 做不到。
需要一层 **Rust 桥接函数**(M1)。

## 2. 工作分解(4 阶段)

### M1 — Rust 桥接层(前置依赖)

**问题**: .at 不能 impl RenderHook trait。

**方案**: 提供 Rust 封装函数(2 个):

1. `fn ash_execute(cmd: String, cwd: String) -> String`
   - Shell::new() + set_render_hook(CaptureHook) + shell.execute(cmd) + 取 RenderedOutput
   - 序列化 RenderedOutput 成 JSON 返回
2. `fn ash_command_list() -> String`
   - Shell::new() + registry().names() 构造 ToolEntry 列表
   - 序列化成 JSON 返回

**位置**: 新建 Rust 桥接(native shim 或独立 crate)。需确认 auto-lang 能否依赖 auto-shell。

**验证重点(M1 go/no-go)**: auto-lang 的 native shim 注册机制能否容纳 ash-core 依赖。
如不可行 → 降级方案 B(renderer 补渲染层)。

### M2 — command_list 对齐(侧栏 80 命令)

- shell.at::command_list() 改调 ash_command_list()(M1 桥接)
- 验证: 侧栏 80 个 ToolEntry(name + description)

### M3 — run_command 结构化执行(核心)

- shell.at::run_command 改调 ash_execute(cmd, cwd),同步返回 RenderedOutput JSON
- renderer merged_exec_loop 改为调 shell.run_command(或回调),拿 RenderedOutput
- update_block_in_state(renderer.rs:2556) + shell_store.at::RunResult(:153)按变体分发
- 流式: 内置命令瞬间完成(无流式);外部长命令保留 renderer std::process(M4 改进)
- 验证: ls 返回 {Table: {columns, rows}},block_body.at 渲染表格

### M4 — 流式 + 收尾

- 外部命令流式(channel FFI 或保留 renderer 路径)
- prompt_context / complete / read_history 对齐 ash-core
- vm vs vue 行为/视觉对比测试

## 3. 关键风险

1. **VM 阻塞**: ash-core Shell::execute 同步阻塞,VM 单线程调用阻塞 UI。
   缓解: renderer executor 线程执行(非 VM 线程)。
2. **RenderedOutput 序列化**: Rust enum → JSON → .at Obj 转换链。
   需确认 .at from_json 处理 tagged union({"Table":{...}})。
3. **auto-lang ↔ auto-shell 依赖**: M1 桥接需 auto-lang 调 auto-shell crate。
   当前不依赖,可能需 ash-gui-auto 侧加桥接。
4. **RenderHook trait**: .at 不能 impl,必须 Rust 桥接(M1)。

## 4. 非目标

- 不改 vue 版 / ash-server(参照基准)
- 不改 a2r Rust 生成路径(72 编译错误,独立问题)
- 不做 sandbox/security(ash-core 已有)

## 5. 执行顺序

```
M1(Rust 桥接)→ M2(command_list)→ M3(run_command 结构化)→ M4(流式+收尾)
```

M1 是 go/no-go 关卡。失败则降级方案 B。

## 6. 调研证据索引(关键 file:line)

**契约与 mock**
- 契约: `back/api.at:166-168`(command_list)、`:194-197`(run_command)
- mock: `back/shell.at:13-29`(command_list 只返 ls)、`:69-71`(run_command no-op)

**vue 版后端参照**
- ash-server worker.rs: `:321-348`(harvest_boot 80 命令)、`:166-219`(CaptureHook +
  Shell.execute)、`:407-438`(run_command)、`:525-559`(drain_stream 流式)
- RenderedOutput 枚举: `ash-core/src/renderer.rs:25-56`(6 变体)
- ls Table 形状: `ash-core/src/renderer.rs:329-361`(columns)、`fs.rs:328-348`(字段)

**renderer 短路(要拆除)**
- merged_exec_loop: `auto-lang renderer.rs:2248-2394`
- output:null: `renderer.rs:2364-2381`
- update_block_in_state 硬塞 Text: `renderer.rs:2556-2562`
- 两条提交入口: `renderer.rs:3815-3822`、`:3864-3870`

**015-notes 纯 .at 后端参照**
- db.at: `:18-25`(List 字面量)、`:47-115`(CRUD 同步返回)
- api.at: `:27-80`(薄包装 return db.X())

**VM FFI 验证**
- use.rust std::process::Command: `auto-lang test/vm/19_rust_std/010_process_command/`
- use.rust chrono: `auto-lang examples/playground-demo/20-datetime.at:3-5`

## 7. 关联

- EDGE-16 修复链: auto-lang master(EDGE-15/16a-f + EDGE-04-B,10 个提交)
- Plan 043: ash-gui-auto 反向生成(.at 源码来源)
- Plan 042: ash-server 统一后端(vue 版后端)
