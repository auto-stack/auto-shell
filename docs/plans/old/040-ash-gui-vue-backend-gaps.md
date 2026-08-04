# Plan 040: ash-gui-vue 后端差距 — 让 GUI 跑满 ash 的命令能力

> **日期**: 2026-08-04
> **状态**: ✅ 完成(M1-M6 已实施)
> **来源**: Plan 039(ash-gui-vue M1-M4)完成后的差距分析——对比 TUI/CLI ash 与 Vue GUI 的能力
> **范围**: `ash-gui/ash-gui-vue/src-tauri/`(后端)+ 必要的 `ash-core`/`auto-shell` 改动
> **前置**: Plan 039 已提交;auto-lang 编译稳定后可联调验证
> **预估**: M1-M6,~800 行 Rust

---

## 0. 背景:为什么需要这个计划

Plan 039 交付了 ash-gui-vue(M1-M4),真实链路已验证(Shell worker → Tauri event → 表格渲染)。但差距分析发现**后端命令执行路径有一个架构缺陷**,导致日常命令会坏掉:

**`render_structured` 绕过了 `execute_inner` 的完整预处理管线。**

GUI worker 的命令执行是:
```
render_structured(shell, cmd)     # shell_worker.rs:200-217
  → parse_args → registry.get → run_atom → render_pipeline_to_structured
  → 失败时 fallback shell.execute(cmd)
```

而 TUI/CLI 的 `shell.execute` 内部(`shell.rs:564-696`)会做:变量展开、`~` tilde、glob、命令替换、`$?` 处理、重定向、env 前缀、`preexec`/`precmd` hooks、安全策略检查。

**后果**:GUI 里 `ls ~/foo`、`echo $HOME`、`ls *.md`、`grep x file > out.txt`、`FOO=bar cmd` 全部失效——因为这些只走 `run_atom` 短路路径,不经过展开管线。

> 注意:这个问题是从 iced 版 `ash-gui-bin/src/main.rs:350-367` 原样继承的。iced 版同样存在,但从未被系统性指出。

## 1. 目标

让 ash-gui-vue 后端的命令执行路径与 TUI/CLI 对齐到"日常可用"级别:

1. **M1(最高优先级)**:修复预处理缺陷——`ls ~/foo`、`echo $HOME`、`ls *.md`、重定向、env 前缀全部可用
2. **M2**:Shell 初始化对齐——别名、`~/.ashrc` 函数、插件
3. **M3**:SmartCommand 执行修复(当前侧栏注入的 `smart run X` 是坏的)
4. **M4**:流式输出——长命令增量显示
5. **M5**:命令取消(Ctrl+C 语义)
6. **M6**:历史持久化(与 CLI 共享 `~/.auto-shell-history`)

## 2. 详细设计

### M1: 修复预处理缺陷(关键)

**问题根因**(`shell_worker.rs:200-217`):
```rust
fn render_structured(shell, input) -> Option<RenderedOutput> {
    let parts = parse_args(input);
    let cmd = registry().get(&parts[0])?;
    let parsed = parse_args(&signature, args).ok()?;
    let pipeline = cmd.run_atom(&parsed, AtomPipeline::empty(), shell).ok()?;
    render_pipeline_to_structured(&pipeline)
}
```
这直接调用 `run_atom`,跳过 `execute_inner` 的展开。

**方案 A(推荐)**:先跑 `shell.execute(cmd)`(它做完整展开 + 预处理),然后**对结果重新结构化**。即把执行和结构化解耦:
1. `shell.execute(cmd)` → 返回 `Option<String>`(文本)
2. 若文本非空,尝试 `render_pipeline_to_structured` 的结构化路径……

问题:`execute` 返回文本,结构化信息已丢失。需要一个能拿到 `AtomPipeline` 且经过展开的路径。

**方案 B(更干净)**:在 worker 里调用 `execute_inner`(或等价的完整管线),得到 `AtomPipeline`/`PipelineData` 后渲染。需要确认 `execute_inner` 的可见性(可能 `pub(crate)`,需要开一个公开入口)。

**方案 C(务实)**:
1. 先做 `parse_args` + 完整展开(复用 auto-shell 的展开函数,如 `expand`,若有),再 `run_atom`。
2. 或者:对**非注册命令/展开场景**,先走 `shell.execute`,对**纯注册命令**,先做最小预处理(检查输入是否含 `~`/`$`/`*`/`>`/`<` 等特殊字符;若含,走 `execute` 路径;若不含,走 `run_atom` 捷径)。

> 实施时需要读 `auto-shell/src/shell.rs` 的 `execute_inner`(640-654)和展开函数,选最优方案。**验收标准**:以下命令在 GUI 里全部正确:
> `ls ~/foo`、`echo $HOME`、`ls *.md`、`echo a > out.txt`、`grep x file > out2.txt`、`FOO=bar echo $FOO`。

### M2: Shell 初始化对齐

当前 worker(`shell_worker.rs:78-79`)只做:
```rust
let mut shell = auto_shell::Shell::new();
shell.load_env_persistence();
```
TUI/CLI 的 REPL(`repl.rs:35-79`)还加载:别名(`ash.toml`)、`~/.ashrc` 用户函数、插件、`less/more/color` 终端命令、render/pager hooks。

**改动**:worker 初始化时复用 REPL 的初始化序列(若可提取成公共函数),或至少加载 aliases + `.ashrc` + 插件。验收:`alias ll='ls -l'` 后在 GUI 里 `ll` 可用;`~/.ashrc` 里定义的函数可用。

### M3: SmartCommand 执行修复

**现状问题**(两个):
1. `ToolSidebar.vue:53` 注入 `smart run {name}`,但 `smart` 是 CLI 子命令(`main.rs:68-72`),不是 Shell 命令——worker 的 `shell.execute("smart run X")` 解析不了。
2. `App.vue:50` 传 `smart-commands=[]`——侧栏从不显示 SmartCommands。

**改动**:
1. 后端新增 command(如 `run_smart_command(name, args)`),调用 `auto_shell::smart_command::executor::execute`(参考 `ai/ask.rs` 或 CLI 的 smart 子命令)。
2. 前端 `command_list` 已经返回 `smart_commands`(boot 快照里有),`App.vue` 把 `smart_commands` 传给侧栏(现在传空数组)。
3. 侧栏点击 SmartCommand → 调用新 command 执行,而非注入文本。

验收:侧栏能看到 SmartCommands,点击能真正执行。

### M4: 流式输出

**现状**:worker 一条命令跑完才 emit 一次 `command-result`(`shell_worker.rs:86-104`)。长命令(`show file | less`、`watch`、长 `find`)无中间反馈。

**已有基础**:`ExternalStream`(`ash-core/src/pipeline/external_stream.rs:71-121`)提供逐行迭代 + 后台退出码线程;`AtomPipeline::ExternalStream` 是管线变体。

**改动**:
1. worker 检测到 `ExternalStream` 管线时,逐行 drain,emit `command-output`(携带 `block_id` + chunk),结束时 emit `command-result`(带退出码)。
2. 前端 `useShell` 监听 `command-output`,把 chunk 追加到 Running block。

验收:跑一个输出多行/耗时的命令(`for i in 1..100 { echo $i; sleep 0.1 }`),看到逐行增长。

### M5: 命令取消

**现状**:前端有 `Running` 状态但无 abort 路径;`shell_worker.rs` 的命令是同步阻塞的,无法中断。

**改动**:
1. worker 的命令执行移入可取消的 task(在已有的 tokio current-thread runtime 里 `tokio::task::spawn`),持有一个取消标记(`CancellationToken` 或 `AtomicBool`)。
2. 新增 command `cancel_command(block_id)`(或全局取消),设取消标记;worker 在 stream drain 循环里检查标记并停止。
3. 前端每个 Running block 显示一个"停止"按钮。

验收:跑长命令,点停止,命令中断,block 标记为 failed(cancelled)。

### M6: 历史持久化

**现状**:前端历史是 `useShell.ts:30-32` 从内存 blocks 派生的,与 CLI 的 `~/.auto-shell-history`(repl.rs:82-86)不共享。

**改动**:
1. 后端新增 `history()` command,读 CLI 的历史文件(或 Shell 的 history 状态,若有)。
2. 前端 boot 时拉取,用于 ↑↓ 导航 + Ctrl+R(与 041 联动)。

验收:GUI 重启后历史保留,且与 CLI 共享同文件。

## 3. 里程碑与验证

| 里程碑 | 内容 | 验证 |
|---|---|---|
| M1 | 预处理修复(最高优先级) | `ls ~/foo`、`echo $HOME`、`ls *.md`、重定向、`FOO=bar` 全对 |
| M2 | Shell 初始化对齐 | `alias`、`.ashrc` 函数、插件在 GUI 可用 |
| M3 | SmartCommand 执行 | 侧栏显示 SmartCommands,点击能执行 |
| M4 | 流式输出 | 长命令逐行增长 |
| M5 | 命令取消 | 长命令可停止 |
| M6 | 历史持久化 | 重启后历史保留 |

M1 独立可验证,强烈建议先做。M2-M6 各自独立。

## 3.1 实施记录(M2-M6)

> 提交于 `feat(040 M2-M6)`。后端 `cargo check -p ash-gui-vue` + 前端 `vue-tsc
> --noEmit` 均通过。

### M2 — Shell 初始化对齐
新增 `shell_worker::init_shell(&mut Shell)`,镜像 `repl.rs:36-79` 的初始化序列:
`load_env_persistence` → `AshShellConfig::load` 别名 → `~/.ashrc` 用户函数 →
`load_all_plugins`。worker 的 `spawn()` 现在调用它(替代原先的裸
`load_env_persistence`)。`~/.ashrc` 首次缺失时播种默认内容(与 REPL 一致)。

### M3 — SmartCommand 执行
- `CommandReq::RunSmart { name, args, reply }`:把执行路由到 worker 线程,跑在
  worker 的**活跃** Shell 上(保留 cwd/env/函数),而非临时 Shell。
- `ShellHandle::run_smart(name, args)` 用 `oneshot` channel 收回复。
- Tauri command `run_smart_command(name, args)` 调用上述接口。
- 前端 `ToolSidebar` 对 SmartCommand 发 `run-smart(name)`(不再注入坏的
  `smart run X` 文本——`smart` 是 CLI 子命令,不是 Shell 命令)。
- `App.vue` 把 `smart_commands` 传给侧栏(原先传空数组)。
- **已知局限**:body 的实时输出打到 worker 线程的 stdout(GUI 不可见),前端
  只看到成功/失败状态与最终返回值。长时 SmartCommand 的流式化留待后续。

### M4 — 流式输出
- `run_command` 现在是 `async`。对**简单外部命令**(无重定向/管道 `|`/链式
  `&&`/`||`/DSL 阶段 `.x`/Auto 表达式/注册命令/legacy builtin/shell 关键字/
  别名),走 `spawn_external_stream` + 逐行 drain,每行 emit `command-output`
  chunk;最终 emit `command-result`(携带累计文本)。
- 其它路径仍走 M1 的 `shell.execute()`(完整预处理 + 结构化捕获)。
- 资格判定函数 `is_shell_builtin` 列出 `execute_inner`/`execute_builtin` 处理
  的全部关键字(cd/alias/source/pushd/…),避免把 builtin 当外部进程 spawn。
- 前端 `useShell` 监听 `command-output`,把 chunk 追加到 Running block 的
  `streamedText`;`BlockItem` 在 Running 时渲染该文本,`command-result` 到达后
  清空并由正式 output 取代。

### M5 — 命令取消
- **关键决策**:取消是**并发信号**,不走命令队列。`ShellHandle` 持有共享
  `Arc<AtomicBool> cancel`;`cancel_command` Tauri command 直接置位。
  原因:worker 在 `spawn_blocking(drain_stream)` 期间无法 dequeue channel 消息,
  若把 Cancel 排进队列会错过窗口。drain 循环在每行之间轮询该标记,置位即停。
- `CommandReq` 不再有 `Cancel` 变体(改用直接置位的 flag)。
- 前端每个 Running block 显示 ■ 停止按钮 → `cancelCommand()`(乐观标记为
  Cancelled,worker 的 `command-result` 随后用真实状态覆盖)。
- **限制**:仅在流式路径生效;`shell.execute()` 内阻塞的命令自然跑完。

### M6 — 历史持久化
- Tauri command `history()` 读共享文件 `~/.auto-shell-history`(与 CLI/TUI 同
  一文件,一行一条),oldest-first。
- worker 每次命令后 `append_history(&cmd)`(转义内嵌换行,保证一条一行)。
- 前端 boot 时拉取 `history()` 存入 `persistedHistory`;`history` computed =
  `persistedHistory + 本次会话命令`,↑/↓ 导航看到两者(按时间顺序)。

## 4. 风险与回退

| 风险 | 缓解 |
|---|---|
| `execute_inner` 不可见/不可复用 | 读 `shell.rs` 确认可见性;必要时在 auto-shell 加公开包装 |
| 流式与结构化渲染冲突 | ExternalStream 先 emit 文本行;最终 `command-result` 再尝试结构化 |
| auto-lang WIP 不稳定 | 与本次改动无关;联调待稳定后执行 |

## 5. 参考文件

- `ash-gui/ash-gui-vue/src-tauri/src/shell_worker.rs`(worker + `render_structured` 220 行)
- `ash/auto-shell/src/shell.rs`(`execute_inner` 564-696、`execute` 401-430、history)
- `ash-core/src/pipeline/external_stream.rs`(流式基础)
- `ash/auto-shell/src/smart_command/executor.rs`(SmartCommand 执行)
- `ash-tui/src/repl.rs`(初始化序列 35-79、历史 82-86、快捷键)
