# ash RoadMap

**版本**: v0.4 → 未来
**最后更新**: 2026-06-26
**状态**: 战略路线图（活文档，随进展更新）

---

## 愿景

**让 ash 成为 AI Agent 最安全、最可靠、最好解析结果的命令执行工具。**

ash 的独特定位：唯一同时具备 **跨平台 + 结构化输出 + 沙盒安全 + AI-ready** 的 shell。人用着顺手，AI 用着更顺手。

---

## 架构分层（核心决策）

```
┌─────────────────────────────────────┐
│  AutoCoder (AI Agent UI)            │  ← 新建：ratatui 全屏 TUI
│  - Block 流式渲染（类 Claude Code）  │
│  - 侧栏：TODO / Stats / Context     │
│  - 异步：AI 边生成边渲染            │
│  - 审批流 / checkpoint              │
└──────────────┬──────────────────────┘
               │ ash -c "..." --json
               ▼
┌─────────────────────────────────────┐
│  Ash (命令执行引擎 + 纯正 Shell)    │  ← 当前：reedline 行编辑 + 结构化管道
│  - POSIX 命令 + AutoLang 脚本       │
│  - 结构化 Atom 管道（表/记录）      │
│  - 沙盒 / 权限 / 审计               │
│  - 非交互模式（--json 输出）        │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│  OS (跨平台：Windows/macOS/Linux)   │
└─────────────────────────────────────┘
```

### 为什么分层（而非把 ash 改成 Agent）

1. **技术模型冲突**：reedline（行编辑 + 滚动流）和 Block 全屏 TUI（空间布局 + 异步流式）是两种不可调和的 UI 模型。强行混在 ash 里会两头不讨好。
2. **行业先例都是分层**：Claude Code / Codex / Warp 都是"新建 Agent UI 层，调用现有 shell"，没有一个把底层 shell 改造成 Agent。
3. **职责单一**：Ash 专注"可靠执行 + 结构化 + 安全"；AutoCoder 专注"AI 工作流的最佳 TUI"。各自做到极致。
4. **与 RoadMap 契合**：MS1（非交互 + `--json`）正是 AutoCoder 调用 Ash 的接口。

### ash-gui 的定位
现有 `ash-gui`（iced GUI）继续作为独立窗口的富可视化版本存在（面向需要 GUI 窗口的场景）。AutoCoder 走 ratatui 全屏 TUI 路线（终端内 Block UI，SSH/容器友好，适合 AI Agent 主力场景）。两者并行，不冲突。

---

## Milestone 总览

| MS | 主题 | 交付价值 | 状态 |
|----|------|----------|------|
| **MS1** | Agent 可调用（非交互 + 稳定 + JSON） | 任何 AI/Agent 能立刻用 ash 当工具 | ✅ 完成 |
| **MS2** | 沙盒与权限控制 | AI 命令可安全执行、可审计、可撤销 | ✅ 完成（008/009）|
| **MS3** | 脚本编程能力 | ASH 能写真实自动化/CI 脚本 | 规划中（调研后发现 80% 已有，拆为 010/011）|
| **MS4** | AI 原生接入（等 auto-ai） | AutoCoder + auto-ai 集成 | 阻塞（等 auto-ai） |

每个 MS **可独立交付**，不互相阻塞。MS1 完成后 ash 即对 Agent 有用。

---

## Milestone 1：Agent 可调用

**目标**：让 AI 能 `ash -c "ls | sort -w size" --json` 一次性执行并拿到结构化结果。

### 功能清单

1. **非交互执行模式**
   - `ash -c "command"`：执行单条命令并退出
   - `ash script.ash`：执行脚本文件
   - `ash < script.ash` / stdin 管道：从 stdin 读脚本
   - 无 reedline 初始化，纯执行路径（快、可重复）

2. **结构化输出（`--json`）**
   - `--json` 全局 flag：管道末端 Atom 序列化成 JSON 输出到 stdout
   - 默认（无 flag）：渲染表格/记录（人读）
   - `--format <fmt>`：扩展（json/csv/table/text），本期至少 json
   - **stdout 纪律**：数据走 stdout，诊断走 stderr

3. **退出码规范（对齐 POSIX）**
   - 0：成功
   - 1：命令执行错误
   - 2：用法错误（参数解析失败）
   - 126：命令不可执行
   - 127：命令未找到

4. **健壮性（panic 消除）**
   - 审计所有 `unwrap()`/`expect()`，corner case 返回错误而非崩溃
   - 错误传播：命令错误 → 非零退出码 + stderr 诊断
   - 确定性：同样的命令每次结果一致

### 验收标准
- [ ] `ash -c "ls | sort -w size" --json` 输出合法 JSON 数组
- [ ] `ash -c "ls /nonexistent"` 退出码 1，stderr 有诊断，stdout 空
- [ ] `ash -c "show data.csv | grep alice"` 非交互正确执行
- [ ] 全量测试通过，无 panic（用模糊/边界输入验证）
- [ ] stdout 只含数据（Agent 解析不被日志污染）

### 依赖
- 无外部依赖，纯 Ash 内部改动

---

## Milestone 2：沙盒与权限控制

**目标**：AI 生成的命令在受限环境执行，可审计、可撤销。这是 ash 的**差异化护城河**（bash/PowerShell 永远不会做沙盒）。

### 功能清单

1. **命令白名单/黑名单**
   - 配置文件（`allow`/`deny` 命令列表）
   - `--allow <cmd>` / `--deny <cmd>` 命令行覆盖
   - 默认策略可配置（默认允许 vs 默认拒绝）

2. **路径沙盒**
   - `--sandbox <dir>`：cd/文件操作限制在该目录内
   - 沙盒外的绝对路径访问被拒绝
   - 符号链接穿透检测

3. **能力开关**
   - `--no-network`：禁用 http_* 等网络命令
   - `--read-only`：禁用所有写操作（rm/mv/cp/mkdir/touch 写入）
   - `--no-exec`：禁用外部命令执行

4. **审计日志**
   - `--audit <file>`：记录每条命令 + 结果 + 时间戳
   - 结构化日志（JSON lines），便于事后分析

5. **预演模式**
   - `--dry-run`：解析并显示会做什么，但不执行写操作
   - Agent 的"思考-确认"模式基础

6. **危险模式检测**
   - `rm -rf /`、`rm -rf ~`、`> /dev/sda` 等已知危险模式拦截
   - 可配置的危险模式列表

### 验收标准
- [ ] `ash -c "rm -rf /" --sandbox /tmp` 被拦截
- [ ] `--dry-run` 显示操作但不实际执行写
- [ ] `--read-only` 下 `touch f` 被拒绝
- [ ] `--audit` 生成完整命令日志

### 依赖
- 建议基于 MS1（非交互模式）做端到端验证

---

## Milestone 3：脚本编程能力

**目标**：ASH 脚本能写真实自动化/CI 任务，补齐 bash 的必修课。

### 功能清单

1. **函数定义**
   - `fn name(args) { ... }`
   - 函数参数、局部变量、返回值

2. **控制流**
   - `if`/`else if`/`else`
   - `for`（遍历数组/范围）、`while`、`break`/`continue`
   - 管道数据迭代（`ls | each { |f| ... }`，已有 each 基础）

3. **变量系统**
   - 类型化变量（不只是字符串）
   - 作用域（局部/全局）
   - 环境变量导出（`export`）

4. **错误处理（学 Nushell）**
   - `try { cmd } catch { ... }`
   - 管道错误传播（`||`、`&&`）
   - 自定义错误

5. **退出控制**
   - `return`（函数返回）
   - `exit <code>`（脚本退出码）

### 验收标准
- [ ] 能写一个部署脚本（条件判断 + 循环 + 错误处理 + 函数）
- [ ] `ash deploy.ash` 端到端执行
- [ ] try/catch 能捕获命令错误并恢复

### 依赖
- 基于 MS1 的非交互执行

---

## Milestone 4：AI 原生接入

**目标**：auto-ai 架构成熟后，AutoCoder + ash + auto-ai 完整集成。

### 功能清单

1. **AutoCoder TUI 应用**（ratatui 全屏）
   - Block 流式渲染（AI 输出边生成边更新）
   - 多 Block 布局（输入 + 多个结果 + 侧栏）
   - 侧栏：TODO / Stats / Context / 审计

2. **Agent 工作流原语**
   - `with-approval`：危险操作需人工确认
   - `checkpoint`：可回滚的检查点
   - `retry`：失败重试策略

3. **自然语言 → ash 命令**
   - AI 解析用户意图，生成 ash 命令
   - 基于 MS2 沙盒安全执行

4. **F3 AI 模式打磨**（ash 内已有的骨架）

### 阻塞条件
- **主动等待 auto-ai 架构就绪，不抢跑**。在 auto-ai 成熟前，MS1-3 让 ash 作为通用 Agent 工具可用（任何 AI 框架都能调）。

---

## 与成熟 Shell 的能力对比

### 当前（v0.4）

| 能力 | bash | fish | nushell | warp | ash v0.4 |
|---|---|---|---|---|---|
| 跨平台统一 | ❌ | ❌ | 🟡 | ✅ | ✅ |
| 结构化输出 | ❌ | ❌ | ✅ | ❌ | ✅ |
| 沙盒/权限 | ❌ | ❌ | ❌ | ❌ | ❌ |
| 非交互+JSON | ❌ | ❌ | ✅ | ❌ | ❌ |
| 脚本编程 | ✅ | ✅ | ✅ | ❌ | 🟡 基础 |
| 补全生态 | 手写 | ✅ | ✅ | ✅ | 🟡 |
| AI 原生 | ❌ | ❌ | 🟡 | ✅ | 🟡 F3骨架 |

### RoadMap 完成后

| 能力 | bash | fish | nushell | warp | ash(RoadMap后) |
|---|---|---|---|---|---|
| 跨平台统一 | ❌ | ❌ | 🟡 | ✅ | ✅ |
| 结构化输出 | ❌ | ❌ | ✅ | ❌ | ✅ |
| **沙盒/权限** | ❌ | ❌ | ❌ | ❌ | ✅ **独有** |
| 非交互+JSON | ❌ | ❌ | ✅ | ❌ | ✅ |
| 脚本编程 | ✅ | ✅ | ✅ | ❌ | ✅ |
| AI 原生 | ❌ | ❌ | 🟡 | ✅ | ✅(MS4) |

ash 的**独特定位**：唯一同时具备"跨平台 + 结构化 + 沙盒 + AI-ready"的 shell。

### 主要差距（RoadMap 后仍可能存在的）
- **补全生态广度**：fish/nushell 有庞大的社区补全。ash 的声明式补全框架在，但覆盖面需积累。
- **成熟度/边缘案例**：bash 几十年积累的健壮性，ash 需时间打磨（MS1 的 panic 消除是第一步）。
- **第三方工具集成**：bash 的管道哲学让所有 Unix 工具天然兼容。ash 的结构化管道对纯文本工具有适配成本。

---

## 决策记录

### D1：方向 2（Agent-ready Shell）而非方向 1（现在加 AI）
- auto-ai 未就绪，抢跑会做不兼容的半成品
- Agent 调用的 shell 必须先可靠/安全，这是前置
- **决策**：先把执行侧做扎实

### D2：分层架构（Ash + AutoCoder）而非 ash 自带 Agent
- reedline（行编辑流）和 Block TUI（全屏空间布局）UI 模型冲突
- 行业先例（Claude Code/Codex/Warp）都是分层
- **决策**：Ash 纯正 Shell，AutoCoder 独立 TUI 应用

### D3：AutoCoder = ratatui TUI 而非 iced GUI
- AI Agent 场景重依赖终端（SSH/容器/远程），TUI 友好
- 复用 ash 已有的 ratatui 经验
- **决策**：AutoCoder 用 ratatui 全屏 TUI；ash-gui(iced) 作为独立窗口版本继续存在

### D4：MS1 = 非交互 + 稳定 + JSON（最优先）
- Agent 调用的最低门槛，做完即可被任何 AI 用
- 最小可验证步骤
- **决策**：MS1 先做非交互模式

### D5：输出默认表格，`--json` 显式切换
- 人/AI 各取所需，同一命令两种输出
- 不做隐式 tty 检测（避免困惑）
- **决策**：`--json` flag 显式控制

---

## 实施原则

1. **计划驱动**：每个 MS 拆成多个 Plan（如 `plans/007-*.md`），先 plan 再 TDD 实施。
2. **可独立交付**：每个 MS 完成即有用户价值，不互相阻塞。
3. **不抢跑**：MS4 主动等 auto-ai，MS1-3 不为 AI 做特化。
4. **回归保护**：每个功能 TDD + 全量回归，保持现有测试全绿。
