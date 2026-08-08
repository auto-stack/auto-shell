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

### 最大杠杆(原以为,实际更复杂)

修复 **EDGE-01**(keyboard onkeydown emit 模拟)原以为可解锁 ~20 个 skip → pass。
深度调研后发现 EDGE-01 不是简单的 emit 模拟,而是 **vm/rust 后端根本不支持元素属性
形式的 onkeydown**(如 `onkeydown.up: .HistoryOlder`)。详见 §5 EDGE-01。

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
| test_prompt_input.py | PB-01..15 | 7 | 8 | — |
| test_history_search.py | HS-01..13 | 3 | — | — |
| **总计** | | **56** | **43** | **0** |

skip 分类:
- **难档**(M2 未做):PB-ghost/highlight/textarea/debounce/autofocus/continuation/键绑定
- **mock 数据空**:TS/PB-hist(commands/smart_commands/history 为空)
- **需后端输出变体**:BB-02..13(Table/Code/Record/MemoryInfo)

## 4. a2r 路径缺陷(M4 诊断)

### 现状(2026-08-08 二次实测):不可用

`auto run -r rust --server rust` 能完成代码生成阶段(`main.rs` + `Cargo.toml`),
但随后 `cargo run` 编译失败,exit 101。两次实测:

| 日期 | 命令 | 错误数 | 说明 |
|---|---|---|---|
| 2026-08-07 | `auto run -r rust`(旧产物) | 94(前端 77 + 后端 17) | 后端 `ash-gui-auto-back` 独立编译,`Vec<(str,...)>` 等 17 错 |
| 2026-08-08 | `auto run -r rust --server rust`(重新生成) | **72**(全前端) | merged mode:后端 in-process,不单独编译后端 crate |

**关键变化(2026-08-08)**:`--server rust` 走 **merged mode**(日志 `✓ rust+rust merged
mode: backend runs in-process`),后端逻辑 inline 进前端二进制,**不再编译独立的
`ash-gui-auto-back` crate**。因此旧的后端 17 个错误(`str` 无大小 / 不可 Deserialize)
不是被修好,而是被绕过。72 个错误**全部集中在前端 `ash-gui`(main.rs, 932 行)**。

这是 a2r codegen 的系统性缺陷,不是源 .at 或过时产物的问题(重新生成前后错误数基本持平)。

### 已修复

- **auto-lang 路径**:compute_auto_lang_rel_path 不检查同级目录(auto-lang 兄弟仓库)
  → fallback 到硬编码错误路径。已修复(walk-up 每级检查 ../auto-lang)。
- **history_search.at parse**:text (括号表达式) 触发 a2r 解析器错误。改用 match_count
  model 字段。
- **nil→None**:block_body.at 用 `!= nil` 而 block_item.at 用 `!= None`。a2r 把 `None`
  正确生成,但 `nil` 当标识符生成 Rust `nil`(不存在)。统一改 `!= None`,错误 99→70。

### 剩余 72 个编译错误的模式分布(2026-08-08 重新生成实测)

全部集中在 `ash-gui-auto/examples/rust-workspace/ash-gui-auto/src/main.rs`。

| 错误类型 | 次数 | 典型表现 | 根因 |
|---|---|---|---|
| `View` 上不存在的变体/方法 (E0599) | ~12 | `View::thead()`/`tr()`/`th()`/`td()`/`tbody()`、`ViewTableBuilder.child()` | **a2r 误以为 `auto_lang::View` 有 HTML 表格 API**——block_body 的 Table 渲染整段直译失败 |
| 不存在的字段访问 (E0609) | ~15 | `String.kind`、`Value.git_status`、`Value.git_branch`、`self.b`(BlockList)、`Value.entry`、`Value.replacement` | store-composable / 嵌套 type 字段映射到弱类型 `serde_json::Value` 上 |
| 未定义符号 (E0425) | ~13 | `output`、`this`、`navigator`、`format_git_label`、`RenderedOutput` | 跨组件作用域泄漏:view fn 参数、computed helper、子组件引用未正确引入 |
| `PromptBar.history()` 缺失 (E0599) | ~6 | `&mut PromptBar` / `&PromptBar` 上找不到 `history()` | List / 子组件 store-composable 方法未生成 |
| 类型不匹配 (E0308) | ~10 | `Value` 当 `Vec<Value>`(`Self::new` 调用)、`String` 比整数 | AutoLang 动态类型 → Rust 静态类型翻译错 |
| 其他(move/Copy/Debug/trait) | ~16 | `self.cwd`/`self.git_label` move out(E0507×2)、`PromptBarMsg::Run` 不 Debug(E0277×3)、`String` 当函数调(E0618×2)、`on_change` trait bound(E0599×1) | 生命周期 / `#[derive]` 缺失 / 闭包类型未实现 trait |

**注**:数字为 grep `error[E0xxx]` 去重计数,部分错误属同一行连发(如第 172 行表格渲染
单行触发 6+ 条 E0599),故总数 72 与"逻辑错误点"少于 72。

### 修复方向(后续)

a2r codegen(auto-lang 侧 `trans/rust.rs` / `ui_gen/rust.rs`)需要系统性改进:
1. **`View` 表格 API 映射**:把 `thead/tr/th/td/tbody` 映射到 `auto_lang::View` 实际
   提供的表格构造方式(查 `ui/iced/renderer.rs` 确认 View enum 真实变体),而非直译
   HTML 标签名。
2. `None` → `Option::None` 或 `serde_json::Value::Null`(非裸 `nil`/`nil` 当标识符)。
3. view fn 参数作用域正确传递(`output.Table` 等需引入参数变量)。
4. store-composable / 嵌套 type 字段访问生成强类型 struct(非全部 `serde_json::Value`)。
5. computed 字段生成为 struct field 而非 method(已修部分,残留 `history()` 等)。

这是 auto-lang 层面的系统性改进,超出 ash-gui-native 范围——当前 VM 模式是唯一可用路径。

## 5. VM 模式已知限制(EDGE-01..16)

| 编号 | 限制 | 影响 | 状态 |
|---|---|---|---|
| EDGE-01 | MCP keyboard 发全局 key,非 input onkeydown | 阻塞 PB 键绑定 + HS 面板 | ✅ 已修(collect_onkeydown_bindings + tool_keyboard widget-aware 派发) |
| EDGE-02 | VM 无法 struct 作 handler 参数 | push_value 占位 0 | ✅ 已绕过(__sse_* 预置字段) |
| EDGE-03 | VM 嵌套 type 字段赋值崩溃(block.status = BlockStatus{}) | Stack Underflow | ✅ 已绕过(renderer Rust 构造) |
| EDGE-04-A | store model 嵌套 type 字面量初始化(eval_expr_to_value) | git_label 等访问崩 | ✅ 已修(eval_expr_to_value 加 Expr::Node/Call 物化到堆) |
| EDGE-04-B | store handler 调 back api 返回 type 实例崩 | boot 数据(command_list)空 | ⚠️ 静态值绕过(跨函数返回 type,更深问题) |
| EDGE-05 | VM handler 读 .blocks 为 nil | for 循环空操作 | ✅ renderer Rust 处理 |
| EDGE-07 | int+str 拼接 | duration/count 显示错 | ✅ 已修(.str()) |
| **EDGE-15** | **真实 Enter 不触发 Run(on_submit 的 input_value 缺失)** | **能打字但回车不执行——只能靠 MCP submit 绕过** | ✅ **已修(auto-lang renderer.rs:on_submit 补 input_value,真实键盘验证通过)** |
| **EDGE-16** | **store model 的 List<T>.new([]) 初始化为 Nil,handler push 失效** | **画面空白(blocks 有数据但 for 循环读空;`for b in .blocks` 渲染空)** | ✅ **已修(auto-lang vm_bridge.rs:`List<T>.new` 物化 GenName→VmRef,回归测试绿)** |

### EDGE-01 深度诊断:onkeydown.* 元素属性在 vm/rust 后端完全不支持

`.at` 的 `onkeydown.up: .HistoryOlder`(元素属性形式,非 `bind {}` 块)在 **vm 和 rust
后端都不会触发 handler**。4 个缺失环节:

1. **未收集**:`extract_key_bindings`(extract.rs:610)只扫 `bind {}` 块,不扫元素属性
   形式的 `onkeydown.*`。元素属性 `onkeydown.up` 进了 AuraNode.events(extract.rs:703),
   但从未被转入 `AuraWidget::key_bindings`(后者只含 bind 块)。
2. **View 层丢弃**:aura_view_builder.rs:1831-1876 `convert_input` 只读
   `onchange/oninput→on_change` 和 `onenter→on_submit`,完全忽略 `onkeydown.*`。
   View::Input(view.rs:257-265)也没有 keydown 字段。
3. **iced widget 限制**:iced 0.14 的 TextInput 只有 `on_input/on_submit/on_paste`,
   **没有 `on_key_press`**(iced_widget-0.14.2 text_input.rs:172-218)。renderer.rs
   的 Input 渲染(6697-6724)也只接 on_change/on_submit。
4. **聚焦态屏蔽 + key 名不匹配**:keyboard_subscription(renderer.rs:2588-2695)用
   `iced::event::listen_with` 全局监听,但 input 聚焦时 `Status::Captured`
   (renderer.rs:2611)会屏蔽。且 key 名是 `"ArrowUp"`(renderer.rs:2630)而非 `.at` 的 `up`。
   MCP keyboard 工具(mcp_server.rs:1523)发 `key_<lower>`,与 handler 名不匹配。

**对照**:Vue 后端完整支持(ui_gen/vue.rs:9007-9028 把 `onkeydown.up.prevent` 翻成
`@keydown.up.prevent`,测试 vue.rs:12730-12748 验证)。所以这是 vm/rust 后端的实现 gap。

**修复方向**(auto-lang 层面,超出 ash-gui-native 范围):
- extract 阶段把元素属性 `onkeydown.<suffix>` 并入 key_bindings(规范化 key 名)
- keyboard_subscription 在 Captured 检查前派发 input-scope 绑定
- 或用支持 on_key_press 的 widget / 包 keyboard_area

### EDGE-04 深度诊断:vm 嵌套 type 字段访问 + 字面量初始化缺陷

> 注意:Auto 用 `type` 关键字声明结构体(如 `type PromptContext { git_branch str; git_status GitStatusInfo }`),
> 没有 `struct` 关键字。这里的"嵌套 type"指 type 字段类型为另一个 type(如 git_status: GitStatusInfo)。

两个相关 vm 运行时缺陷(语法合法,vm 执行崩):

**缺陷 A:嵌套 type 字面量初始化未物化内层实例**

model 默认值 `var git_info PromptContext = PromptContext{ git_branch: "", git_status: GitStatusInfo{...} }`
语法合法,但 vm 把内层 `GitStatusInfo{...}` 字面量存成了 primitive(而非堆上的实例引用)。
导致 `.git_info.git_status` 取到的是 primitive,再访问 `.staged`(field_index>0)时,
GET_FIELD(engine.rs:3752-3758)对 primitive 报 "Field index out of bounds for primitive"。

**缺陷 B:shell.at 的 type 实例构造触发 Invalid instance ID**

`command_list()` 返回 BootSnapshot 时,`var snap BootSnapshot = BootSnapshot{}`;
`snap.commands = cmds` 等字段赋值触发 "Invalid instance ID: 0"(heap id 0 访问)。
可能是空字面量 `BootSnapshot{}` 的某字段初始化为无效 heap id。

**当前绕过**:Init 用静态值(cwd=".", 空 commands/history, git_label=format_git_label("",0,...))。
真实 boot 数据待 vm 修复嵌套 type 字面量初始化(engine.rs GET_FIELD / 字面量物化路径)。

### EDGE-15 深度诊断:真实 Enter 不触发 Run(on_submit 的 input_value 缺失)

> **症状(用户视角)**:VM 模式窗口里,点击输入框聚焦后**能正常打字**,但**按回车不执行**
> ——input 不清空、block 不创建、命令不跑。这是 ash-gui-auto 当前最大的可用性阻塞。
> 根因在 **auto-lang 的 VM 渲染器**(renderer.rs),不在 ash-gui 的 .at 源码。
>
> **诊断修正**:初判"input 无焦点、键盘全断"**部分错误**——字符输入(on_input)正常,
> 证明聚焦 OK。真正只有 Enter(on_submit)链路断。

**实测复现(2026-08-08,auto run -r vm + MCP)**:

```
autoui_type text=ls          → input: "ls"          ✓ 字符输入正常
autoui_keyboard key=enter    → input: "ls"(未清空) ✗ Run 未触发
                              blocks: nil, next_id: 0
```

对照:`autoui_action action=submit` → 命令正常执行。**真实 Enter 不行,MCP submit 行。**

**根因(auto-lang `crates/auto-lang/src/ui/iced/renderer.rs`)**:

`on_submit`(Enter)与 `on_input`(字符)的 msg 构造不同——**on_submit 不带 input_value**:

```rust
// renderer.rs:6707-6713  on_input —— 带 text
IcedMessage { ..., input_value: Some(text) }   // ✓ 有值

// renderer.rs:6719-6720  on_submit —— 裸 msg
input_widget.on_submit(msg);                   // ✗ msg.input_value = None
```

而 update 的 emit 模拟块依赖 input_value 才转发到 store(renderer.rs:3694-3705):

```rust
if widget_name == "PromptBar" && event_name == "Run" {
    if let Some(cmd) = saved_input_value.as_deref() {   // ✗ None → 整块跳过
        state.component.on_with_input_for("ShellStore", "RunCommand", Some(cmd));
        ...  // block 构造 + 执行器提交,全在这块里
    }
}
```

Enter → on_submit → msg.input_value=None → emit 模拟整块跳过 → 回车无反应。

**对照证据:MCP 路径补了这个值。** `mcp_server.rs:1963-1968`:
```rust
if action == UiActionType::Submit && input_value.is_none() {
    if let Some(v) = read_input_value(target_view) {
        input_value = Some(v.clone());   // ← MCP 手动补值,所以 MCP submit 能工作
    }
}
```
MCP 作者明确知道 submit 不自带 value 并补了;**真实 on_submit 路径没有等价补值逻辑**。

**为什么测试套件 56 pass 没发现**:测试全用 `autoui_type` + `autoui_action submit`
(`test_command_exec.py:52-70`),走 MCP 那条已补值路径。"真实回车"从未被测。

**修复(已实施 + 验证 ✅)**:auto-lang `crates/auto-lang/src/ui/iced/renderer.rs`,
在 update 的 `saved_input_value` 构造处(handler 执行**前**——因 PromptBar.Run 会清空
`.input`),当 msg 无 input_value 时从 `state.input` 抢救当前值:

```rust
let saved_input_value = msg.input_value.clone().or_else(|| {
    if widget_name == "PromptBar" && event_name == "Run" {
        state.component.read_state("input").ok()
            .map(|v| v.as_str().to_string())
    } else { None }
});
```

**验证(2026-08-08,真实键盘)**:窗口输入 `ls` + 真实回车 → 命令执行成功
(`status: Success`, output 有内容, block 创建)。详见 auto-lang
`docs/plans/archive/371-autoui-mcp-improvements.md` §19。

**遗留小问题**:回车后 `state.input` 仍显示 "ls"(PromptBar.Run 应清空但受
patch_input_values 回填影响),下次输入会覆盖,不影响功能。

### EDGE-16 深度诊断:store model List<T>.new([]) 初始化为 Nil

> **症状**:VM 模式画面空白——store 里 blocks 有数据(Success + output),但
> `for b in .blocks` 渲染不出 BlockItem。主内容区(BlockList 命令输出)完全空白。
>
> **诊断修正**:初判"VM 布局塌缩(row 交叉轴 fill 不传递)"**方向错误**——极简
> 静态示例(`examples/ui/021-block-static`)证明纯静态 block 能正常渲染,布局基本 OK。
> 真正根因是 **store model 字段的 List 初始化为 Nil,handler 的 push 静默失效**。

**实测复现 + 定位(2026-08-08)**:

```
store BlockStore Init handler:  .blocks.push(b1)  .blocks.push(b2)
read_state('blocks') 修复前 = Ok(Nil)         ← List<T>.new([]) 被求值为 Nil!
read_state('blocks') 修复后 = Ok(VmRef{id})    ← 物化成堆对象
```

**根因(auto-lang `crates/auto-lang/src/ui/vm_bridge.rs`)**:

`eval_expr_to_value`(求值 store/组件 model 字段初值)的 `Expr::Call` arm 之前
只认 `Expr::Ident` name。`List<BlockItem>.new([])` 的 AST 是
`Call { name: Dot(GenName("List<BlockItem>"), "new"), ... }`——name 是 `Expr::Dot`,
receiver 是 `GenName`(泛型),不匹配 → 落入 `Value::Nil` 兜底(vm_bridge.rs:1054)。

结果:`blocks` 字段槽存的是 `Value::Nil`,不是 List 对象。handler 的
`.blocks.push(b1)` 在 engine 里 `get_heap_object(list_id)` 对 Nil 返回 None,
push 整个跳过、静默失败(engine.rs:5481)。`read_state_as_vec("blocks")` 也因
Nil 返回 Err,view 的 `for b in .blocks` 读到空。

**对照**:纯逻辑层 `type` 的 `self.list.push()` 正常(走真正 VM codegen,
`List<T>.new` 被物化成真实 ListData 堆对象)。store/组件 model 字段初值
**绕过 VM codegen**,走 `eval_expr_to_value` 这个不完整的求值器。

**修复(auto-lang master,3 个提交)**:
- `c25e0888` — 回归测试(两层对照:纯逻辑层绿 + store 层红)+ `021-block-static` 示例
  + `build_example_component` helper 抽象
- `70f94e02` — `eval_expr_to_value` 加 `Expr::Dot(GenName/Ident, "new")` 分支:
  `List<...>.new(...)` → 物化空 `ListData<Value>` 堆对象返回 `VmRef`;
  `<Type>.new()` → 若注册类型走字面量物化
- `753ad4bc` — 示例 App 加 `.Init -> { store.Init() }`(测试载体本身缺触发)

**验证**:`block_static_store_tests` 2/2 全绿——`read_state_as_vec("blocks").len() == 2`,
push 持久化。

> 注:之前记录的"布局塌缩(vtree bbox 0×0)"现象仍可能存在(iced Row 交叉轴 fill
> 传递不完整),但**不是 ash-gui 画面空白的主因**——block 数据渲染不出来才是。
> 修复 List 物化后,block 数据能进 state,`for b in .blocks` 即可渲染 BlockItem。
> 布局问题如有可后续单独查。
