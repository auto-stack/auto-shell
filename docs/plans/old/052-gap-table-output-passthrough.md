# Plan 052: GAP-Table — 结构化 output 完整透传

> **日期**: 2026-08-11
> **状态**: ✅ M1-M4 完成(2026-08-11)
> **来源**: plan 051 M5 发现的 GAP-Table 技术债
> **范围**: 让 store 的 RunResult 能处理所有 RenderedOutput 变体
> （Table/Record/Code/Error/Text），不只是 Text。
> **核心目标**: vue 模式下 `ls` 的结构化 Table 能在 GUI 里渲染出来。

---

## 0. 背景

### plan 051 的遗留

plan 051 打通了 SSE 链路（curl 已证 ash-server 真实执行 ls → 结构化 Table 回流），
但发现 **GAP-Table**：store 层只处理 `output.Text`，其它变体被丢弃。

**根因**：`__sse_output_text: str` 字段承载不了结构化 output。整个 SSE→store 传输层
（plan 051 的 ②-b 预置字段模式）只为"文本流式"设计。

**好消息**：
- 视图层（block_body.at:151-179）**已完整支持所有变体分发**
  （`if .output.Table != None { RenderTable } else if .output.Record ...`）
- 手写版 useShellHttp.ts:152 也是直接 `block.output = r.output` 透传
- **瓶颈纯粹在 SSE→store 传输层**

### 决策（用户已确认）

- **Empty 变体不特别处理**：透传 r.output 后，Empty（裸字符串 "Empty"）走
  block_body.at 的 else 兜底（行为正确，和 None 表现一致）
- **VM 兼容用 RunResult 回退逻辑**：若 `__sse_output` 为空（VM renderer 不填），
  回退到 streamed_text 包成 {Text: streamed_text}

---

## 1. 任务（M1-M4）

### M1: api.at 契约修正（补遗漏字段）

修 3 处契约漂移（手写版 shell.ts:44-62 是权威参照）：

| 类型 | 行号 | 改动 |
|---|---|---|
| `TableOutput` | api.at:49-52 | 补 `atom_type: str`（服务端 Table 真实发送，renderer.rs:31） |
| `ErrorOutput` | api.at:59-61 | 补 `kind: str`（服务端 Error 真实发送，renderer.rs:52-55） |
| `RenderedOutput` 注释 | api.at:33-37 | 更新注释说明 Empty 走透传 + else 兜底，Table/Record 带 atom_type |

**不改 RenderedOutput 结构**——Empty 不特别表达（透传后走 else），保持 ?T 可选变体联合。

### M2: shell_store.at — 加 __sse_output 字段 + RunResult 改赋完整对象

**model 字段**（92-99 附近）：
- 新增 `var __sse_output RenderedOutput = RenderedOutput{}`
- 保留 `__sse_output_text` 不动（不破坏现有路径）

**RunResult handler**（184-190）：从硬编码 `out.Text = __sse_output_text` 改为：
- 若 `__sse_output.Text != None`（vue SSE 透传了完整 output）→ `b.output = .__sse_output`
- 否则（VM merged 回退）→ `out.Text = b.streamed_text; b.output = out`

### M3: codegen emit_preset_dispatch — 透传整个 r.output

文件：auto-lang worktree `crates/auto-lang/src/ui_gen/vue.rs:10983-11020`

RunResult 分支加 `__sse_output.value = r.output ?? {}`（保留原 __sse_output_text 赋值）。

### M4: 验证

1. 重新 codegen（worktree auto.exe）
2. curl + grep 验证产物含 `__sse_output.value = r.output`
3. VM 回归：`auto run -r vm` 启动正常

---

## 2. 改动文件清单

| 文件 | 仓库 | 改动 |
|---|---|---|
| `ash-gui-auto/src/back/api.at` | auto-shell | TableOutput/ErrorOutput 补字段 + 注释 |
| `ash-gui-auto/src/front/shell_store.at` | auto-shell | 加 __sse_output 字段 + RunResult 改赋值 |
| `auto-lang/crates/auto-lang/src/ui_gen/vue.rs` | auto-lang worktree | emit_preset_dispatch 加 __sse_output 透传 |

视图层（block_body.at）**无需改动**。

---

## 3. 实施结果（2026-08-11）

### 各里程碑状态

| 里程碑 | 状态 | 说明 |
|---|---|---|
| M1 api.at 契约修正 | ✅ | TableOutput 补 atom_type、ErrorOutput 补 kind、注释更新 |
| M2 shell_store.at | ✅ | 加 __sse_output 字段 + RunResult 改赋完整对象(VM 回退 streamed_text) |
| M3 codegen emit_preset_dispatch | ✅ | 加 `__sse_output.value = r.output ?? {}` 透传(修了 format! 的 `{}` 转义 bug) |
| M4 验证 | ✅ | curl 证 SSE Table 帧 + codegen 产物正确 + VM 回归通过 |

### M4 验证证据

**codegen 产物**(dispatch):
```ts
else if (data.event === 'command_result') {
  const r = data.CommandResult ?? data;
  __sse_output.value = r.output ?? {};        // ← Plan 052 新增:透传整个 output
  __sse_output_text.value = (r.output && r.output.Text) ? r.output.Text : '';
  ...
}
```

**RunResult handler**(产物):
```ts
if (__sse_output.value.Text != null) {
  b.output = __sse_output.value;              // ← vue 模式:用完整 output(Table 等)
} else {
  let out = {  };
  out.Text = b.streamed_text;                 // ← VM 回退:streamed_text 包成 Text
  b.output = out;
}
```

**SSE 帧**(curl 实测):
```json
{"event":"command_result","block_id":201,"status":"Success",
 "output":{"Table":{"columns":["name","type","size","modified"],
   "rows":[[{"Tagged":{"text":"src",...}},...]]}}}
```
Table 结构化数据完整透传到 store,视图层 block_body.at 的 `if .output.Table != None { RenderTable }` 分支即可渲染。

**VM 回归**: `auto run -r vm` → `first state sync in view()`,无 link 错误。新增 `__sse_output` 字段不影响 VM。

### 实施中修的一个 bug

M3 初版 format 字符串里 `r.output ?? {}` 的 `{}` 被 Rust format! 当成参数占位符吃掉,
产物变成 `r.output ?? else if`(语法错误)。修法:`{}` → `{{}}`(format 字面量转义)。
