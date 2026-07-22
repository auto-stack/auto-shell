# ASH for AI Agents

> 给 AI Agent（Claude Code / Cursor / Codex / 自建 Agent）集成 ash 的指南。

## 为什么用 ash 当 Agent 的命令执行层？

| 痛点 | bash/pwsh | ash |
|------|-----------|-----|
| 输出不可靠解析 | 文本流，需正则猜结构 | **结构化 JSON 信封** |
| 安全无保障 | `rm -rf /` 直接执行 | **内置沙箱**（路径/网络/读写拦截） |
| 命令能力不可发现 | Agent 靠猜 | **79 命令的 JSON Schema catalog** |
| 跨平台不一致 | bash≠pwsh | **三平台同一行为** |

## Agent CLI 接口（Plan 028）

ash 提供专用的 `ash agent` 子命令族，给 Agent 用：

### 1. 发现工具：`describe-tools`

```bash
ash agent describe-tools --format compact
```

返回所有命令的 JSON Schema catalog（MCP `tools/list` 兼容格式）：

```json
{
  "schema_version": "1",
  "tool_count": 79,
  "tools": [
    { "name": "ls", "description": "List directory contents", "inputSchema": {...} },
    { "name": "grep", "description": "Search text", "inputSchema": {...} },
    ...
  ]
}
```

Agent 启动时调一次，拉取完整工具列表。

### 2. 查看策略：`describe-policy`

```bash
ash agent describe-policy
```

返回当前安全策略的能力位摘要（**不含具体路径/密钥**，安全进 system prompt）：

```json
{
  "schema_version": "1",
  "policy": {
    "sandboxed": true,
    "read_only": false,
    "no_network": true,
    "deny_count": 2
  }
}
```

### 3. 先问后做：`check`

```bash
ash agent check "rm -rf /tmp/old_data"
```

Dry-run 探测，**不执行**，返回是否会被允许：

```json
{
  "command": "rm -rf /tmp/old_data",
  "allowed": false,
  "decision": "deny",
  "denied_reasons": [{ "rule_id": "security-policy", "message": "..." }]
}
```

### 4. 执行：`run`

```bash
ash agent run "ls -la /sandbox"
```

执行命令，返回**结构化 JSON 信封**：

```json
{
  "schema_version": "1",
  "status": "success",
  "data": {
    "kind": "file_list",
    "atom_type": "FileList",
    "value": [{"name": "README.md", "size": 2048, "type": "file"}, ...],
    "pipeline_hint": "pipeable to filter/sort/select"
  },
  "error": null,
  "timing": { "wall_ms": 12 },
  "command_echo": "ls -la /sandbox"
}
```

**关键**：`status` 是顶层判别（`success`/`failed`/`denied`/`partial`），Agent 不用挖 exit code。`data.kind` 告诉 Agent 输出的语义类型，便于后续 pipeline 决策。

## 典型 Agent 集成流程

```python
# 伪代码：一个外部 Agent 调 ash 的完整流程

# 1. 启动期：拉 catalog + policy
tools = ash("agent describe-tools --format compact")
policy = ash("agent describe-policy")
system_prompt += f"\nAvailable tools: {tools}\nSandbox: {policy}"

# 2. 规划期：对可疑命令先 check
result = ash('agent check "rm -rf /old/data"')
if result["allowed"] == false:
    # 看 denied_reasons 的 remediation，改路径重试
    new_cmd = extract_remediation(result)
else:
    result = ash(f'agent run "{new_cmd}"')

# 3. 执行期：拿结构化输出
output = ash('agent run "ls -la /project"')
for file in output["data"]["value"]:
    if file["size"] > 10_000_000:
        print(f"Large file: {file['name']}")
```

## 安全沙箱（给 Agent 用）

启动 ash 时加安全 flag，Agent 的所有命令自动受限：

```bash
# 路径限制 + 禁网络 + 审计
ash --sandbox /project --no-network --audit /var/log/ash.jsonl

# 白名单（只允许 ls/cat/grep）
ash --allow ls --allow cat --allow grep

# 完全只读
ash --read-only
```

这些 flag 对 Agent 透明——Agent 正常调 `ash agent run`，ash 内部按 policy 拦截。

## 输出信封 schema

所有 `ash agent run` 的输出遵循统一信封：

| 字段 | 说明 |
|------|------|
| `schema_version` | 信封版本（当前 "1"） |
| `status` | `"success"` / `"failed"` / `"denied"` / `"partial"` |
| `data.kind` | 语义类型（`file_list` / `process_list` / `table` / `text` / `empty` / ...） |
| `data.atom_type` | Atom 类型名（`FileList` / `Table` / ...） |
| `data.value` | 实际数据 |
| `data.pipeline_hint` | 该输出可接的 pipeline 操作提示 |
| `error.kind` | 错误类别枚举（`nonzero_exit` / `not_found` / `permission_denied` / `timeout` / `sandbox_violation` / ...） |
| `error.remediation` | 机器可读的恢复建议 |
| `timing.wall_ms` | 执行耗时 |
| `command_echo` | 回显执行的命令 |

## 旧的 `--json` 接口（Plan 007，向后兼容）

```bash
ash -c "ls" --json
```

这是 Plan 007 的原始 Agent 接口，**仍然可用**。新的 `ash agent run` 是它的增强版（结构化信封 + 错误枚举）。两者并行，旧的不废弃。

## MCP 集成（计划中）

`ash agent describe-tools` 和 `ash agent run` 的输出格式**跟 MCP 兼容**（`tools/list` + `tools/call`）。未来 `ash agent mcp-serve`（独立 Plan）会在两者上加一层 stdio JSON-RPC 包装，让 Claude Desktop / Cursor 直接连。

## 相关文档

- [SKILL.md](../SKILL.md) —— 给 Agent 读的完整技能说明
- [Plan 028 设计](../designs/028-agent-execution-engine.md)（已删除，委托 auto-ai）—— Agent 引擎的原始设计
- [快速上手](quickstart.md) —— ash 基础
