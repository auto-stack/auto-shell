# ash-gui-native 架构归档

> 本文档归档 ash-gui-native(M0..M4)的关键设计决策、实现细节、差异清单、测试矩阵
> 和已知限制。总计划见 `ash-gui-native-plan.md`。

## 1. SSE 流式桥设计(M1)

### 问题

ash-gui 的 `~Stream<ShellEvent>` SSE 契约(api.at:stream)在 Vue(EventSource)/
Tauri(listen)下消费,但 iced 原生路径(renderer.rs)无 SSE 客户端,也无 subscription 桥。
命令输出/结果永远到不了 UI,block 永远停在 Running。

### 架构(renderer.rs)

```
                    ┌─────────────────────────────────────┐
                    │  run_dynamic_iced(iced 进程,main 线程) │
                    │                                     │
  type ls + submit  │  update 闭包                         │
  ────────────────> │   PromptBar.Run → emit 模拟 →        │
                    │   store.RunCommand(记 __pending_*)    │
                    │   ↓ (RunCommand 拦截)                 │
                    │   构造 block(Value::Obj) + push      │
                    │   ↓                                  │
  ┌──────────────┐  │   提交命令到 ─────────────────┐     │
  │ 执行器线程    │  │                               │     │
  │ (std::thread)│ <┼───────────────────────────────┘     │
  │              │  │                                     │
  │ merged 模式: │  │  subscription 闭包(16ms poll)        │
  │ std::process │  │   shell_event_subscription           │
  │   ↓ stdout   │  │   drain SHELL_EVENT_RX               │
  │   ↓ exit     │  │   ↓ IcedMessage                      │
  │ HTTP 模式:   │  │   command_output/command_result 分支  │
  │ reqwest SSE  │  │   → update_block_in_state(Rust 更新) │
  │   /api/stream│  │                                     │
  └──────┬───────┘  │  view 闭包(渲染更新后的 block)        │
         │          └─────────────────────────────────────┘
         │ mpsc
         v
   SHELL_EVENT_RX(全局 OnceLock)
```

### 关键决策(用户确认)

| 维度 | 决策 | 理由 |
|---|---|---|
| 命令执行位置 | Rust 执行器线程(renderer 侧) | UI VM 不注入 ShellHost;复用 mcp_action_subscription 的「全局 channel + 后台线程」模式 |
| handler 参数 | 预置字段 + 无参 handler | VM `push_value` 对 struct 参数只推占位 0(vm_bridge.rs:929);renderer 用 write_state 写 `__sse_*` 字段 |
| merged vs HTTP | 两者都做 | merged(默认,broadcast/std::process)+ HTTP(`AUTO_BACKEND=...`,reqwest SSE 连 /api/stream) |

### VM 限制的绕过(5 个互补修复,见 plan §10.3)

1. **tool_type 支持 vnode_**:view_template 不展开 Component,改用 styled_vtree 找 input
2. **submit action**:keyboard 发全局 key,不触发 iced input on_submit
3. **input_state_map 递归子组件**:root view_tree 不含子组件 input 绑定
4. **emit 模拟**:handler_codegen 剥离子组件 callback prop(handler_codegen.rs:996)
5. **Rust 侧 block 构造/更新**:VM 嵌套 struct 赋值崩溃 + renderer↔vm Array 不同步

## 2. Vue→Auto 差异清单(M2,已完成 + 待做)

### 已修复(12 处,纯逻辑组)

| 编号 | 行为 | 文件 | 对齐 Vue 源 |
|---|---|---|---|
| BL-08..10 | duration badge(ms/s) | block_item.at | BlockItem.vue:36-41 |
| BB-08 | 仅 Dir/FileName 可点 | block_body.at | cellStyle.ts:22-26 |
| BB-12 | code bold/italic | block_body.at | CodeView.vue:16-24 |
| BB-11 | memory usage 回退 | block_body.at | RecordView.vue:17-26 |
| TS-01 | 侧栏描述 | tool_sidebar.at | ToolSidebar.vue:46-52 |
| HS-04 | 不敏感+倒序+cap50 | history_search.at | HistorySearch.vue:30-37 |
| HS-13 | 匹配计数 | history_search.at | — |
| PB-11 | Ctrl+L 清屏 | prompt_bar.at + renderer | PromptBar.vue:264-267 |
| PB-comp-07 | 建议描述 | prompt_bar.at | — |
| APP-05/06 | git_label | shell_store.at | App.vue:75-92 |
| CMD-06 | cancel 只停首个 | shell_store.at + renderer | useShellTauri.ts:112 |

### 待做(难档,依赖 iced 能力 / renderer 拦截)

| 编号 | 行为 | 阻塞原因 |
|---|---|---|
| PB-01 | autofocus | focus 不在 vnode 状态 |
| PB-02 | continuation 符号切换 | 需续行检测逻辑 |
| PB-03 | textarea multiline | 用单行 input |
| PB-05..06 | ↑↓ 历史导航 | 需 keyboard onkeydown emit 模拟(EDGE-01) |
| PB-08 | Tab 补全 | 同上 |
| PB-comp-01 | debounce 80ms | 需 iced timer subscription |
| PB-ghost-01..06 | ghost text 全套 | 需透明叠加层 + Ctrl+F/Right |
| PB-high-01..09 | 语法高亮 | 需 tokenize + 透明 textarea 叠覆盖层 |
| PB-inj-01 | injected emit/focus | 需 renderer emit 模拟 + focus |
| BL-01 | 自动滚动 | 需 iced scroll subscription |
| CMD-09/10 | smart 失败 + duration | 需 renderer 拦截(类似 RunCommand) |
| APP-11 | Ctrl+D window.close | 需 iced window::close task |

### 最大杠杆

修复 **EDGE-01**(keyboard onkeydown emit 模拟)可解锁 ~20 个 skip → pass:
所有 PB 键绑定(↑↓/Tab/Ctrl+R/L/C/D)+ HS 面板打开。

## 3. 测试覆盖矩阵(M3)

| 文件 | 行为编号 | pass | skip | xfail |
|---|---|---|---|---|
| test_smoke.py | 基础设施 | 6 | — | — |
| test_command_exec.py | M1 命令执行 | 2 | — | — |
| test_app_shell.py | APP-01..15 | 9 | 4 | — |
| test_command_lifecycle.py | CMD-01..12 | 7 | 5 | — |
| test_block.py | BL-01..18 + TS/git | 11 | 6 | — |
| test_blockbody.py | BB-01..14 | 3 | 10 | — |
| test_tool_sidebar.py | TS-01..05 | 2 | 3 | — |
| test_backend.py | BACK-01..12 | 6 | 7 | — |
| test_prompt_input.py | PB-01..15 | 3 | 12 | — |
| test_history_search.py | HS-01..13 | — | — | 3 |
| **总计** | | **49** | **47** | **3** |

skip 分类:
- **难档**(M2 未做):PB-ghost/highlight/textarea/debounce/autofocus/continuation/键绑定
- **mock 数据空**:TS/PB-hist(commands/smart_commands/history 为空)
- **需后端输出变体**:BB-02..13(Table/Code/Record/MemoryInfo)

## 4. a2r 路径缺陷(M4 诊断)

### 现状(2026-08-07):不可用

`auto run -r rust` 能生成 `main.rs` + `Cargo.toml`(代码生成阶段成功),但生成的
Rust 代码有 **~99 个编译错误**,是 a2r codegen 的系统性缺陷。

### 已修复

- **auto-lang 路径**:compute_auto_lang_rel_path 不检查同级目录(auto-lang 兄弟仓库)
  → fallback 到硬编码错误路径。已修复(walk-up 每级检查 ../auto-lang)。
- **history_search.at parse**:text (括号表达式) 触发 a2r 解析器错误。改用 match_count
  model 字段。

### 99 个编译错误的模式分布

| 错误类型 | 次数 | 根因 |
|---|---|---|
| E0425 cannot find value `nil` | 13 | `None` → `nil`(codegen bug,nil 非 Rust 关键字) |
| E0425 cannot find value `output` | 6 | view fn 参数作用域泄漏 |
| E0609 no field on serde_json::Value | 11 | struct 字段访问在 serde_json::Value 而非强类型 struct |
| E0308 mismatched types | 15 | 类型推断(Vec vs Value 等) |
| E0615 computed as method | 4 | duration_label/history computed 当方法调用 |
| E0277/E0599/E0061 等 | 40 | trait/方法/参数数 |

### 修复方向(后续)

a2r codegen(trans/rust.rs)需要:
1. `None` → `Option::None` 或 `serde_json::Value::Null`(非裸 `nil`)
2. view fn 参数作用域正确传递
3. struct 字段访问生成强类型(非全部 serde_json::Value)
4. computed 字段生成为 struct field 而非 method

这是 auto-lang 层面的系统性改进,超出 ash-gui-native 范围。

## 5. VM 模式已知限制(EDGE-01..14)

| 编号 | 限制 | 影响 | 状态 |
|---|---|---|---|
| EDGE-01 | MCP keyboard 发全局 key,非 input onkeydown | 阻塞 PB 键绑定 + HS 面板 | 待修(最大杠杆) |
| EDGE-02 | VM 无法 struct 作 handler 参数 | push_value 占位 0 | ✅ 已绕过(__sse_* 预置字段) |
| EDGE-03 | VM 嵌套 struct 赋值崩溃 | Stack Underflow | ✅ 已绕过(renderer Rust 构造) |
| EDGE-04 | boot api 触发 Invalid instance ID | TS/PB-hist 数据空 | ⚠️ 静态值绕过 |
| EDGE-05 | VM handler 读 .blocks 为 nil | for 循环空操作 | ✅ renderer Rust 处理 |
| EDGE-07 | int+str 拼接 | duration/count 显示错 | ✅ 已修(.str()) |
