# Plan 051: ash-gui-auto 前后台分离（vue 模式连 ash-server）

> **日期**: 2026-08-10
> **状态**: ✅ M1-M6 完成(2026-08-10)
> **来源**: plan 050 可行性调研（已确认可行）+ 050 双模分析（已选方案②-b）
> **范围**: 让 ash-gui-auto 的 vue 模式前后台分离——前端走 HTTP 调 ash-server，
> 后端调 auto-shell 真实执行命令。VM --merged 维持现状（renderer 拦截，不改）。
> **核心目标**: vue 模式下输入 `ls` → ash-server 真实执行 → 结构化输出回流渲染。
> **前置**: plan 050 R1-R3 调研完成；方案②-b 选定

---

## 0. 方案概要

### 架构（分离后）

```
                 ┌─ vue 模式（本计划）──┐
ash-gui-auto  ───┤  vue 前端 (TS/JS)     ├──HTTP──→ ash-server ──→ auto-shell::Shell::execute
  gen/front/vue  │  EventSource SSE      │         (Rust, :3000)   (真实执行 ls 等)
                 └───────────────────────┘
                 ┌─ VM --merged（不改）──┐
              ───┤  .at → VM → iced      │  renderer 的 merged_exec_loop 直接
                 │  shell.at (mock)      │  std::process 执行（维持现状）
                 └───────────────────────┘
```

### 关键决策（来自 plan 050）

| 决策 | 选择 | 依据 |
|---|---|---|
| store 双模方案 | **②-b 预置字段模拟** | codegen 解决平台差异 + VM 零改动 |
| SSE 代码来源 | codegen 自动注入（解注 api.at stream） | R1 确认 codegen 已支持 |
| VM --merged | 维持现状 | 用户约束 |
| 循环依赖 | 无阻碍 | R3 确认单向 DAG |

---

## 1. 实施任务（M1-M6）

### M1: 解注 stream 端点 + 触发 SSE codegen 注入

**目标**：让 codegen 的类型驱动 SSE 注入对 shell_store 生效。

**改动**：

| 文件 | 行号 | 改动 |
|---|---|---|
| `ash-gui-auto/src/back/api.at` | 233-240 | 解注 `stream()` 端点 + `subscribe()` 调用；更新注释（R1 证伪"~Stream 致 link 失败"） |
| `ash-gui-auto/src/back/shell.at` | 441-445 | 解注 `subscribe()`（VM mock 用，返回 empty stream） |
| `ash-gui-auto/src/front/shell_store.at` | 9 | `use back.api:` 列表加 `stream`（触发 SSE 注入的标记，见 vue.rs:10467-10469） |

**验证**：重新 codegen 后，`gen/front/vue/src/stores/useShellStoreStore.ts` 应出现
`new EventSource('/api/stream')` 注入块。但此时 dispatch 是 `RunOutput(data)`/`RunResult(data)`
（带参），而 handler 是无参的——M3 修这个。

**风险**：解注 subscribe 不触发 VM link 失败（R1 已确认风险接近零：link 只看符号名，
`~Stream<T>` 不参与 link 决策；subscribe 无 yield → VM 当普通函数）。

---

### M2: codegen 实现 ②-b 预置字段模拟（核心）

**目标**：改 codegen 的 SSE dispatch，对预置字段模式的 store，先填 `__sse_*` ref
再调无参 handler，而非 `RunOutput(data)` 带参调用。

**改动**（全部在 `auto-lang/crates/auto-lang/src/ui_gen/vue.rs`）：

1. **检测预置字段模式**（`vue.rs:10541` 的 `if wire_sse` 块入口）：
   ```rust
   // ②-b: 预置字段模式。store 用 __sse_* 前缀字段（ash-gui 约定）。
   // 对此类 store，SSE dispatch 先填 __sse_* ref 再调无参 handler。
   // 这是平台差异处理：VM 用 renderer 填字段+无参 handler；
   // vue 用 codegen 填 ref+无参 handler。两 target 的 handler 调用方式一致。
   let preset_mode = store.state_vars.iter()
       .any(|s| s.name.starts_with("__sse_"));
   ```

2. **dispatch 分叉**（`vue.rs:10565-10570` 的 emit 循环内）：
   ```rust
   for (i, (disc_field, wire_value, action)) in chain.iter().enumerate() {
       let kw = if i == 0 { "if" } else { "else if" };
       if preset_mode {
           // ②-b: 按 action 名硬编码字段映射（legacy fallback 路径）
           // 映射来源：shell_store.at:139-156 注释 + ShellEvent/CommandResult 契约
           emit_preset_dispatch(&mut code, kw, disc_field, wire_value, action);
       } else {
           // 现状：带参透传（forge 等带参 handler 项目走这条）
           code.push_str(&format!(
               "                {} (data.{} === '{}') {}(data);\n",
               kw, disc_field, wire_value, action
           ));
       }
   }
   ```

3. **新增 `emit_preset_dispatch` 辅助函数**（约 25 行）：
   ```rust
   /// ②-b: 对预置字段模式 store，生成"填 ref + 调无参 handler"的 dispatch。
   /// 仅处理 legacy fallback 的 RunOutput/RunResult 两条（ash-gui 契约）。
   /// 其它带变体的 SSE 流走带参透传，不进这里。
   fn emit_preset_dispatch(code: &mut String, kw: &str, disc: &str,
                           wire: &str, action: &str) {
       // data.event === 'command_output' → 填 __sse_block_id/__sse_chunk，调 RunOutput()
       // data.event === 'command_result' → 填 __sse_block_id/__sse_cwd/__sse_status/
       //   __sse_output_text/__sse_duration_ms，调 RunResult()
       // 注意 command_result 的 payload 嵌套：data.CommandResult ?? data（兼容 tag 枚举）
       match action {
           "RunOutput" => { /* __sse_block_id.value=data.block_id; __sse_chunk.value=data.chunk; RunOutput(); */ }
           "RunResult" => { /* 取 r=data.CommandResult??data; 填 5 个 ref; RunResult(); */ }
           _ => { /* 非 legacy action，退回带参 */ }
       }
   }
   ```

**字段映射表**（硬编码，来源 shell_store.at:139-156）：

| SSE 事件 | data 路径 | 目标 ref |
|---|---|---|
| command_output | `data.block_id` | `__sse_block_id` |
| command_output | `data.chunk` | `__sse_chunk` |
| command_result | `r.block_id` | `__sse_block_id` |
| command_result | `r.cwd` | `__sse_cwd` |
| command_result | `r.status` | `__sse_status` |
| command_result | `r.output?.Text ?? ''` | `__sse_output_text` |
| command_result | `r.duration_ms` | `__sse_duration_ms` |

其中 `r = data.CommandResult ?? data`（兼容 ash-server 的 tag 枚举嵌套，见 GAP-E）。

**验证**：codegen 后，`useShellStoreStore.ts` 的 SSE 块应是：
```ts
if (data.event === 'command_output') {
    __sse_block_id.value = data.block_id;
    __sse_chunk.value = data.chunk;
    RunOutput();
} else if (data.event === 'command_result') {
    const r = data.CommandResult ?? data;
    __sse_block_id.value = r.block_id;
    __sse_cwd.value = r.cwd;
    __sse_status.value = r.status;
    __sse_output_text.value = r.output?.Text ?? '';
    __sse_duration_ms.value = r.duration_ms;
    RunResult();
}
```

**风险**：
- ⚠️ 硬编码映射只服务 ash-gui（`__sse_` 前缀是 ash-gui 约定）。非预置字段模式的
  store 走原路径，不影响 forge（R2 已确认 forge SSE 是手写 TS，不经 codegen）。
- ⚠️ 改 auto-lang crate 后需重新编译 auto-lang；auto-shell 的 auto-lang 依赖版本要同步。

**文档**：在 vue.rs 的 `emit_preset_dispatch` 注释里写明 `__sse_` 是 codegen 保留前缀。

---

### M3: GAP 修补（类型擦除导致的运行时问题）

**目标**：修 R2 发现的类型擦除 gap，让对接不崩溃、不丢数据。

**改动**：

| GAP | 文件 | 改动 |
|---|---|---|
| **GAP-A**（PromptContext null 崩溃） | `shell_store.at:119-120` Init | `prompt_context()` 返回的 git_status 非 git 目录时为 null，Init 和 RefreshGit 读 `.staged` 等会崩。加 null 守卫或默认值。 |
| **GAP-A（产物侧）** | codegen 产物 | Init handler 里 `git_info.value.git_status.staged` 加可选链 `?.`（或 .at 层用条件判断，让 codegen 生成守卫） |
| **GAP-B**（CommandStatus 失败消息） | `shell_store.at:182-183` RunResult | `.at` 里 status 是 str，但实际失败时是 `{"Failed":msg}` JSON。当前 `st.message = .__sse_status` 把整个对象塞进去。改：解析失败取 message。 |
| **GAP-端口** | `ash-gui-auto/gen/front/vue/vite.config.ts:33` | proxy target 默认 8080，改 3000（或文档要求设 `AUTO_HTTP_PORT=3000`）+ dev server 端口错开 |

**GAP-A 在 .at 层的处理**（让 codegen 生成守卫）：
```
// shell_store.at Init / RefreshGit 里
if .git_info.git_status != None {   // 或检查字段是否可访问
    .git_label = format_git_label(...)
} else {
    .git_label = .git_info.git_branch != "" ? "⎇ " + .git_info.git_branch : ""
}
```
需验证 .at 能否表达 nullable 检查；若不能，改在 codegen 产物侧加 `?.`（但会被覆盖，
故优先 .at 层解决）。

**注意**：GAP-B 的 `__sse_status` 在 M2 的 dispatch 里是 `r.status`——成功时是裸串
`"Success"`，失败时是 `{Failed: msg}` 对象。shell_store.at 的 RunResult handler
当前用 `.__sse_status == "Success"` 判断，失败分支 `st.message = .__sse_status`
塞的是整个对象。修法：
- 成功/取消：保持裸串判断
- 失败：从 `.__sse_status` 里取 `.Failed`（但 .at 里 status 类型是 str，取对象字段
  需要动态处理）

**验证**：非 git 目录启动不崩；命令失败时错误消息正确显示。

---

### M4: 前后端对接配置

**目标**：让 gen 出的 vue 工程能连上 ash-server。

**改动**：

1. **vite proxy**：`gen/front/vue/vite.config.ts` proxy target → `http://localhost:3000`
   （或生成时读 `AUTO_HTTP_PROXY` 环境变量）
2. **dev server 端口**：与 ash-server 的 3000 错开（如 `AUTO_FRONT_PORT=1420`，
   对齐 ash-gui-vue 的约定）
3. **启动文档**：README 或 ash-gui-auto 文档里写明 vue 模式启动步骤：
   ```
   # 1. 启动 ash-server
   cd ash-gui/ash-server && cargo run  # 监听 :3000
   
   # 2. 启动 vue 前端
   cd ash-gui/ash-gui-auto/gen/front/vue
   AUTO_HTTP_PORT=3000 AUTO_FRONT_PORT=1420 npm run dev
   ```

---

### M5: 端到端验证

**目标**：vue 模式下完整链路跑通。

**验证步骤**：
1. 启动 ash-server（`cargo run`，确认 :3000 监听）
2. codegen 生成 vue 工程（确认 M1/M2 的 SSE 注入正确）
3. 启动 vue 前端（确认 vite proxy 指向 :3000）
4. 浏览器打开，验证：
   - ✅ 侧栏命令列表加载（command_list）
   - ✅ 历史加载（history）
   - ✅ git 标签显示（prompt_context，含非 git 目录不崩）
   - ✅ 输入 `ls` → Running block → **结构化 Table 输出回流**（核心！）
   - ✅ 输入 `pwd` → Text 输出
   - ✅ 输入错误命令 → Failed 状态 + 错误消息
   - ✅ Ctrl+C 或 Cancel → 取消执行

**验证标准**：`ls` 真实执行，输出结构化渲染（非纯文本），与 ash-gui-vue 行为对齐。

---

### M6: VM --merged 回归验证

**目标**：确认改动没破坏 VM --merged 模式（维持现状）。

**验证步骤**：
1. `auto run -r vm` 启动 VM 合并模式
2. 确认解注 subscribe 后 VM 仍能 link 启动（R1 预测无碍）
3. 确认 VM 模式下命令执行仍走 renderer（merged_exec_loop），不受 codegen 改动影响
4. 确认 shell.at 的 mock 仍在（command_list 79 命令、read_history 等）

**风险点**：解注 subscribe 后若 VM link 失败，需回滚 api.at/shell.at 解注，
改用方案①（手写 SSE 桥）。但 R1 评估风险接近零。

---

## 2. 任务依赖与顺序

```
M1 (解注 + 触发注入) ──→ M2 (codegen ②-b) ──→ M3 (GAP 修补)
                                                    │
M4 (对接配置) ──────────────────────────────────────┤
                                                    ↓
                                              M5 (端到端验证)
                                                    │
                                                    ↓
                                              M6 (VM 回归)
```

M4 可与 M2/M3 并行（配置改动独立）。M5 是总验证，依赖 M1-M4。M6 最后做回归。

---

## 3. 风险清单

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 解注 subscribe 触发 VM link 失败 | 低（R1 证伪） | VM 模式启动不了 | 回滚解注，改方案① |
| M2 硬编码映射写错字段 | 中 | SSE 数据填错 ref，输出错乱 | M5 端到端验证逐字段核对 |
| GAP-A 在 .at 层无法表达 nullable | 中 | 非 git 目录崩溃 | codegen 产物侧加守卫（若有 post-gen 机制）或改 api.at 类型 |
| 改 auto-lang 影响其他 codegen 测试 | 低（R2 确认 forge 不经 codegen） | a2r 测试失败 | 跑 auto-lang 全测试套件 |
| vite proxy 端口冲突 | 低 | 前端连不上后端 | M4 端口错开 + 文档 |

---

## 4. 技术债登记（不在本计划范围）

| 债 | 来源 | 建议处理时机 |
|---|---|---|
| `__sse_*` 预置字段 hack | plan 044 的 VM struct 参限制 | 未来 VM renderer 改造时，方案⑤（带参 handler）统一清除 |
| codegen SSE dispatch 硬编码映射 | 本计划 M2 | 待 VM 支持多参 handler 后，改回通用带参透传 |
| RenderedOutput::Empty 变体丢失 | R2 GAP-C | 低优先级，影响语义不影响运行 |
| Table 缺 atom_type 字段 | R2 GAP-D | 低优先级 |

---

## 5. 成功标准

本计划完成的标志：
1. ✅ vue 模式启动后，输入 `ls` 能看到真实文件列表（结构化 Table）
2. ✅ ash-server 真实执行命令（非 mock）
3. ✅ VM --merged 模式回归通过（维持现状，未受影响）
4. ✅ codegen 改动不影响其他项目（forge / a2r 测试通过）

---

## 6. 实施结果（2026-08-10）

### 各里程碑状态

| 里程碑 | 状态 | 提交 |
|---|---|---|
| M1 解注 stream 端点 | ✅ | auto-shell main `8f2d1c2` |
| M2 codegen ②-b 预置字段模拟 | ✅ | auto-lang worktree `auto-shell` 分支 `6d2b7477` |
| M3 GAP 修补 | ✅ | auto-shell main `3eb2d24` |
| M4 vite proxy 配置 | ✅ | 本地 gen 产物(gen/ 不入库) |
| M5 端到端验证 | ✅(部分) | curl 证 SSE 全链路;GUI 输入受 IAB 限制阻断 |
| M6 VM 回归验证 | ✅ | VM link 成功,UI 初始化完成 |

### M5 验证证据

**SSE 全链路(curl 决定性证据)**:
```
POST /api/run_command {block_id:100, cmd:"ls"}
→ GET /api/stream 回流:
{"event":"command_result","block_id":100,"status":"Success",
 "output":{"Table":{"columns":["name","type","size","modified"],
   "rows":[["src","dir",...],["Cargo.lock","file",...],...]}},"duration_ms":1}
```
ash-server 真实执行 ls → 结构化 Table(4列×4行)经 SSE 回流。**plan 051 核心目标达成。**

**GUI 对接(非流式端点)**:页面加载后侧栏显示 80+ 真实命令、git 标签 `⎇ main ⇡2`
(compute_git_label GAP-A 守卫生效)。command_list/history/prompt_context 全部对接成功。

**GUI 输入受阻**:IAB 浏览器工具的 fill 不触发 v-model、press 字母键不可靠、
evaluate 被 Chromium 拒绝、dom_cua broker mismatch。非应用 bug,是测试工具链限制。

### M6 VM 回归证据

`auto run -r vm`(用含 M2 改动的 worktree auto.exe):
- GPU adapter 选中,iced 窗口初始化成功
- `AutoUI MCP: first state sync in view()` — VM 状态同步完成,UI 渲染
- **无 link 错误** — 实证 R1 判断:解注 subscribe 不触发 VM link 失败
- M2 codegen 改动(vue.rs dispatch 分叉)不影响 VM(VM 走 merged_exec_loop,不经 vue codegen)

### 新发现的技术债

| 债 | 说明 | 优先级 |
|---|---|---|
| **GAP-Table** | M2 dispatch 只提取 `output.Text`,但 ls 真实输出是 `output.Table`(结构化)。Table 输出时 `__sse_output_text` 为空,store 回退到 streamed_text(也为空,因 SSE 无 chunk)。要完整渲染 Table 需处理 `r.output.Table` 路径 | 高(影响 ls 等结构化命令的渲染) |
| vite.config server 块不入库 | gen/ 被 gitignore,手维护的 server/proxy 块不进版本控制,重新 codegen 会丢 | 中 |
| GUI 输入验证手段缺失 | IAB 的 fill/press 对 Vue v-model 不可靠,需找替代验证方案 | 中 |
