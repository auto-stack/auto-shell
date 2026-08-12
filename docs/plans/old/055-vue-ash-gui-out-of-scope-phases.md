# Plan 055: vue ash-gui 范围外工作面 — 详细 Phase 调研

> **日期**: 2026-08-12
> **状态**: 📋 调研完成,待按优先级实施(各 Phase 独立,可挑选)
> **来源**: Plan 054 §4「范围外工作面」的详细化 —— 每项做成独立 Phase,含现状/根因/方案/文件/工作量/风险/依赖
> **前置**: Plan 054(M1-M7,vue 版基础能力)已完成
> **核心约束**: CLI 与 vue 共享同一后端引擎(auto_shell::Shell + completions::engine),Shell 是 `!Send`(VM 用 `Rc`),必须钉在一个 OS 线程

---

## 速查表(优先级排序)

| Phase | 内容 | 工作量 | 风险 | 优先级 | 关键复用 |
|---|---|---|---|---|---|
| **C** | 终端专属命令降级(less/more/color) | 小 | 低 | 🥇 先做(快赢) | 流式路径已有 |
| **D5** | VM 折叠 per-block | 小 | 低 | 🥇 先做(纯 .at) | block 对象独立 |
| **D6** | VM prompt_context(renderer 侧读 git) | 小-中 | 低 | 🥈 早做(绕 VM) | renderer 注入模式 |
| **B** | 管道/重定向流式 | 中 | 中低 | 🥈 中(方案 A) | `spawn_external_chained` 已就绪 |
| **A** | 作业控制 + 信号 + 后台 `&` | 中 | 中 | 🥈 中(注意双份管理坑) | `JobManager`/`spawn_external_background` |
| **E** | store 强类型 | 中 | 中 | 🥈 中(服务 053 §6C) | `auto_type_to_ts_type` 已就绪 |
| **D4** | VM 复制按钮(arboard native) | 中 | 中 | 🥉 后做 | VM 原生注册 |
| **F** | Tauri transport 抽象 | 中 | 中 | 🥉 后做(依赖 Tauri 后端) | `IApi` selector 已存在但未接入 |
| **D1** | VM 结构化输出(crate 拆分) | 大 | 中 | ⏳ 后做 | 待确认 ash-core 依赖方向 |
| **G** | a2r 二进制(止血/废弃) | 大 | 高 | ⏸️ 仅 G-a 表格展平止血,其余暂缓 | — |

**依赖关系**:A、B 互相独立可并行(A 的 kill 进程树 + B 的多段 cancel 可合并公共 util);E(vue.rs TS 端)与 G-b(rust.rs Rust 端)是同一"类型透传"问题的两面,若都做应先抽共享 `Type → {ts, rust}` 映射层;F 依赖 Tauri 后端事件总线就绪;D1 方案 A(crate 拆分)解锁 D 类最彻底。

---

## Phase A: 作业控制 + 信号 + 后台 `&`

### 现状
- worker 单线程串行消费 mpsc(`ash-server/src/worker.rs:192` `while let Some(req) = rx.recv().await`),一个 `Run` 处理完才接下一个 —— 前台命令阻塞整个循环。
- `cmd &` 检测(`worker.rs:458-460`)后 `return Ok(None)` 落 `shell.execute`,但 Shell 内部 `execute_background`(`auto-shell/src/shell.rs:4241`)把 child 塞 `self.jobs` 后只 `eprintln!` —— **GUI 既看不到 "[1] Running" 也拿不到 job 列表**。
- `jobs`/`fg`/`bg`/`suspend`(`shell.rs:608-611`)走 `shell.execute`,输出是字符串/`eprintln!`,worker 当 `RenderedOutput::Text` 回传,**无结构化 job 事件**。
- 无 SIGINT 转发(`worker.rs` 全文件无 `signal::`;`bin/ash-server.rs:10-27` 不调 `signal::init()`)。
- Cancel(`worker.rs:125-127`)只置 `AtomicBool`,仅流式路径 `drain_stream`(`worker.rs:537`)轮询 kill;`shell.execute` 慢路径**无法取消**。
- CLI 的 `JobManager`(`auto-shell/src/job.rs:32`,已 `Send`)+ Ctrl-C guard(`auto-shell/src/signal.rs:13/50/57-70`)完整可用。

### 根因
worker 把 Shell 当"一次一命令的同步 RPC"。作业控制需要:(a) 后台命令不阻塞 mpsc;(b) job 状态独立事件通道;(c) cancel 命中具体 child PID。

### 方案
1. **后台命令不阻塞**(worker.rs 主改):`run_streaming_external`(`worker.rs:444`)的 `ends_with('&')` 早退处(`:458-460`)改为真处理 —— `spawn_external_background`(`ash-core/src/cmd/external.rs:124`)→ 注册到 worker 级 `Arc<Mutex<JobManager>>`(新增,复用 `auto_shell::job::JobManager`)→ 发 `ShellEvent::JobStarted` → `return Ok(Some(Empty))` 不阻塞。
2. **新增作业事件**:`types.rs:121` `ShellEvent` 加 `JobStarted{job_id,block_id,cmd}` / `JobDone{job_id,exit_code,cmd}` / `JobList(Vec<JobInfo>)`;`types.rs` 加 `JobInfo{id,command,state,exit_code}`(Serialize)。
3. **Job reaper**:worker 主循环(`:192`)每轮 `job_mgr.reap_finished()`(`job.rs:78`),finished job 发 `JobDone`。
4. **kill_job**:`ShellHandle::cancel()` 扩展 `kill_job(job_id)` → `job_mgr` 取 child → `child.kill()`。**不建议**给 ash-server 装 `signal::init()`(server 无终端,Ctrl-C 语义来自前端 Stop 按钮)。
5. **前端**:http.rs 加 `GET /api/jobs`、`POST /api/kill_job`、`POST /api/fg`;前端 store 加 jobs 面板。
6. **fg 语义**:GUI 无前台终端可回,建议**只做 jobs 列表 + kill,不做真 fg/wait**。

### 工作量 / 风险
**中**(1-2 天核心 + reaper/kill 半天 + 前端 1-2 天)。**风险中** —— Shell 的 `jobs`(`shell.rs:144`)与 worker 级 JobManager 会**双份管理**:worker 拦截 `cmd &` 后**必须不再**调 `shell.execute`(早退),否则双重 spawn。

### 依赖
复用 `JobManager`(Send)+ `spawn_external_background`(已存在)。无新 crate。与 Phase B 无耦合。

### 验收
`sleep 5 &` → 立即返回(block Running 但不卡 UI)+ jobs 面板见 `[1] Running sleep`;5s 后 JobDone → `[1] Done`;`kill_job` 能停。

---

## Phase B: 管道 / 重定向流式

### 现状
- 流式只处理单段管道(`worker.rs:474-477` `if pipe_cmds.len() != 1 { return Ok(None); }`)。`a | b` 落 `shell.execute`(`:427`)→ `execute_pipeline_with_auto`(`shell.rs:1254`)**一次性**返回全量文本,前端无流式 chunk。
- 重定向也早退(`worker.rs:461-466` `redirect.stdout/stdin.is_some()`)。
- CLI 已有真 OS 管道链接:`shell.rs:1392-1417` 用 `spawn_external_chained`(`ash-core/src/cmd/external.rs:91`)+ `into_raw_stdout`(`pipeline/external_stream.rs:160`),kernel 级 `prev.stdout → next.stdin`,200k 行不死锁测试(`external.rs:653-678`)。
- `drain_stream`(`worker.rs:527-561`)只读单流。

### 根因
worker 流式路径为"单个外部命令"设计,刻意剔除管道/重定向/逻辑(`:468-477`)。多段流式需 OS pipe 链 + 只 drain 末端(逻辑 CLI 已实现,但 worker 没复用,因它要边读边推 + cancel)。

### 方案(方案 A,推荐)
扩展 `run_streaming_external`(`worker.rs:474-477` 不再早退):
```
// pipe_cmds: [a, b, c] —— 循环前对每段跑 :487-501 过滤(外部命令 only),
// 任一段是 builtin/auto/alias → return Ok(None) 落 shell.execute
let cwd = shell.pwd();
let mut stream = ext::spawn_external_stream(&pipe_cmds[0], &cwd)?;
for seg in &pipe_cmds[1..] {
    let raw = stream.into_raw_stdout();          // external_stream.rs:160
    stream = ext::spawn_external_chained(seg, &cwd, raw)?;  // external.rs:91
}
// 末段交 drain_stream(worker.rs:527)
```
- **cancel**:多段需收齐每段 `kill_handle()`,cancel 时逐个 kill(否则上游 orphan)。改 `drain_stream` 收 `Vec<...>` 或 cancel 时 kill 进程树(Windows `taskkill /T` 已杀树 `external_stream.rs:189`;Unix 需 `setsid` 建进程组 kill 整组)。
- **重定向**:输出 `> file` 不需流式(保持 `:461-466` 早退);stdin `< file` 可并入(首段 `spawn_external_stream_with_input` `external.rs:76`)。
- 方案 B(大,不推荐):抽 `ash_core::pipeline::spawn_chained` 公共 API,影响 CLI。

### 工作量 / 风险
**中**(1-1.5 天)。**风险中低**(OS pipe 链已验证;主要风险 cancel 时上游 orphan,Unix 进程组方案)。

### 依赖
`spawn_external_chained` / `into_raw_stdout` 已就绪。与 Phase A 独立。cancel 进程树方案可与 A 的 kill 合并公共 util。

### 验收
`ls | grep .rs | head` → 逐行流式 chunk;Cancel 能停全链(无 orphan);`echo hi > /tmp/x` 仍走重定向(非流式)。

---

## Phase C: 终端专属命令降级(less/more/color)★ 先做

### 现状
- `worker.rs:675-677` `is_terminal_only_command` 匹配 `less|more|color`;`:502-504` 命中即 `return Ok(Some(Text(terminal_only_message)))`;`:679-692` 提示「请用 CLI/终端 raw mode」。
- 这些命令需 raw mode + 键盘(less 分页器)或 24-bit 终端色彩(color)。webview 走 CSS,既无 raw mode 也无终端颜色概念。

### 根因
硬拒绝 + 提示用 CLI。但 less/more 在非 tty 时多数实现直接 cat 内容到 stdout;color 的本意(检测色彩能力)在 GUI 恒为真彩。

### 方案(分级降级)
1. **less/more → 透明放行**:删 `worker.rs:675-677` 对 less/more 的拦截(只留 color),让 `less file` 走流式路径(`run_streaming_external`)。无 tty 时 less 通常直接输出内容 ≈ `cat` + block 已有滚动(plan 054 M4)。**兜底**:spawn 失败回退提示。
2. **color → 能力指示**:保留 color 拦截,文案改"GUI 支持 24-bit CSS 真彩,无需检测"或返回 `Text("truecolor (webview CSS)")`。
3. **真分页器(可选,大,前端独立)**:前端 `less` 模式 block(块内 vim-like 滚动 + `/` 搜索),后端只改"读文件 → 流式推 block",前端识别 `block.kind == "pager"`。后端改动同"放行"。

### 工作量 / 风险
**小**(放行 + 文案,半天)。真分页器大(纯前端)。**风险低**(放行后最坏 less 报错,用户见错误信息可接受)。

### 依赖
无。与 A/B 独立。

### 验收
`less README.md` → 内容流式渲染 + 可滚动(非提示文本);`color` → 真彩能力提示。

---

## Phase D: VM 深度兼容(plan 053 §6 D)

VM(merged)模式 = AutoLang VM 直接跑 `.at` 前端,对应 `auto-lang/crates/auto-lang/src/ui/iced/renderer.rs`。**核心架构限制**:auto-shell 依赖 auto-lang(`auto-shell/Cargo.toml:16`),故 auto-lang**不能反向依赖 ash-core**(循环依赖)—— D1 根因。第二限制:`shell_store.at:88` —— **VM 不能把 struct 作 handler 参数**(`push_value` 对 Obj 推占位 0),故全用 `__sse_*`/`__pending_*` 预置字段 + 无参 handler。

### D1 — 命令执行绕过 ash-core(renderer 自解析 stdout)
**现状**:`back/shell.at:424` `run_command` no-op;真执行在 `renderer.rs:2659-2839` `merged_exec_loop`,裸 `std::process::Command`(`:2728-2751`)。结构化解析只 `{Text: stdout}`;仅 `ls`(`:2705`)/`show`(`:2685`)在 spawn 前拦截做 Table/Code,其余(grep/wc/ps/find…)全 Text。
**根因**:循环依赖(auto-lang ↛ ash-core)。
**方案**:
- A(推荐,大)— 把 ash-core 结构化能力抽成独立 crate `ash-render`,auto-lang 依赖它,`merged_exec_loop` 调用。需先确认 `ash-core/Cargo.toml` 不反向依赖 auto-lang。
- B(渐进,中)— 扩 renderer 拦截白名单(grep/wc/ps/find 各写 `handle_X_command` 像 `handle_ls_command`)。治标。
- C(大,架构转向)— merged 模式连本地 ash-server HTTP(像 `http_sse_loop` `renderer.rs:2843`),统一后端。
**工作量/风险**:A 大(2-3 天 + 验证,crate 边界风险);B 中(每命令半天);C 大。**建议**:先确认 ash-core 依赖方向,优先 A。

### D2 — 流式输出
**现状**:**已实现**(plan 044 M1 解决)。`renderer.rs:2760-2785` 逐块读 stdout 发 `command_output`;executor 独立线程(`:2453/2469`)。残留:`full_stdout` 内存累积(长输出压力)。
**方案**:基本无需改;可选优化累积改"最近 N 行 + 总字节"。**工作量小,风险低**。

### D3 — 命令取消
**现状**:**已实现**。`renderer.rs:2762-2766` 检查 `cancel_flags` + `child.kill()`;`:4395+` Cancel 事件设 flag。残留:只 kill 末段(同 Phase B 管道问题)。
**方案**:与 Phase A/B 的 kill 进程树合并。**工作量小,风险低**。

### D4 — 复制按钮 no-op
**现状**:`block_item.at:143-155` `navigator.clipboard.writeText` → VM 无 navigator → 软失败。
**根因**:VM 缺 `navigator.clipboard` 原生(iced renderer 是桌面窗口,无 navigator)。
**方案**:VM 注册 `clipboard.write_text` 原生(`vm/native_catalog.rs`,参考 `:710-745`),用 `arboard` crate;ts_adapter 把 `navigator.clipboard.writeText(s)` 映射到该 native;或 `.at` 改用 `Clipboard.write_text(s)`(需先注册)。
**工作量/风险**:**中**(VM 原生注册 + arboard + codegen 映射),风险中(VM FFI 注册有 CALL_SPEC 静态分发 bug 前科,见 D6)。**依赖**:新增 arboard;与 D6 同源(VM FFI)。

### D5 — block 折叠全局 ★ 先做(纯 .at)
**现状**:`block_item.at:13-15` `var collapsed bool` 是 per-instance model;但 VM 把子组件 state**合并进根 state**(`lib.rs:2611-2620`),多 BlockItem 实例共享 `collapsed` → 点一个全折叠。
**根因**:VM 状态扁平单根(`lib.rs:2591-2620`),子 widget 不做 per-instance state 隔离(架构限制)。
**方案**:
- A(推荐,小)— `collapsed` 移到 block 数据:`block_item.at:15` 删 model 字段;`:34/137` 改 `.block.collapsed`;ToggleCollapse 改 emit 带 block_id 消息给 store,store 翻 `blocks[id].collapsed`(`shell_store.at` 加 handler)。state 天然 per-block。
- B(大)— VM 支持 per-instance state。影响所有 widget,风险高。
**工作量/风险**:A **小**(0.5 天,纯 .at + 验证 VM emit 带 int 参 —— `shell_store.at:109` Msg 已有 `RunCommand(str)` 带参先例,可行),风险低。

### D6 — prompt_context mock
**现状**:`back/shell.at:409-421` 返回静态 `git_branch="main"`。曾用 `std::process::Command` FFI 读 git,但 VM 有 **CALL_SPEC 静态分发 bug**(`c.args()` receiver 变类型名字符串 → invalid Command handle);改 `File.read_text(".git/HEAD")` 后 trim/split 在 server 模块上下文产出垃圾值。
**根因**:两 VM bug —— (a) 方法调用静态分发对某些 receiver 错配;(b) server 模块字符串操作产出垃圾。
**方案**:
- A(绕过,推荐,小-中)— renderer 侧(Rust)读 git:新增 `read_git_info(cwd)` 用 `std::process::Command::new("git").args(["rev-parse","--abbrev-ref","HEAD"])`(renderer 是 Rust,不经 VM,无 CALL_SPEC bug),Init 时注入 store `git_info`(像 `renderer.rs:4274` 注入)。
- C — renderer 侧读 `.git/HEAD` + Rust 解析(避 VM 字符串 bug)。
- B(治本,大)— 修 VM CALL_SPEC bug(`vm/native_catalog.rs`/`generic.rs`)。
**工作量/风险**:A/C **小-中**(半天,风险低,绕 VM);B 大(修 VM,风险高)。

### Phase D 汇总
D2/D3 已完成(只补 kill 进程树,与 A/B 合并)。优先:D5(小,纯 .at)→ D6(renderer 读 git,绕 VM)→ D4(arboard native)→ D1(crate 拆分,大)。

---

## Phase E: store 强类型(Plan 053 §6 C)

### 现状
- 生成的 store 全 any:`useShellStoreStore.ts` 15 个 ref `ref<any>`(`vue.rs:11108`)+ composable `(): any`(`vue.rs:11223`)+ handler 参数 `(cmd: any)`(`vue.rs:11252`)+ module-fn 参数 `: any`(`vue.rs:11396`)。
- 但 `api.ts:3-120` 有完整 interface(`Block`/`PromptContext`/`RenderedOutput`…),fetch 函数已带 `Promise<BootSnapshot>`(`api.ts:124-195`)。

### 根因
类型信息**存在于 IR 但未用于注解**:`AuraStateDef.type_info: Type`(`aura/types.rs:634`)+ 转换函数 `VueGenerator::auto_type_to_ts_type(&Type)`(`vue.rs:10381-10408`,覆盖 Str/Int/Bool/List/Option/User)**已存在**,但只在 `:11120` 用来 gate method-mapping,从未注入 ref 声明。handler 参数:`AuraMsgVariant.payload: Vec<Type>`(`aura/types.rs:693`)携带类型,但 `handler_params` 只存名(`extract.rs:1464-1486`)。module-fn `AuraModuleFn`(`aura/types.rs:403-412`)只有 `params: Vec<String>`(名)+ `ret_ts`,**参数类型缺失**(唯一信息缺口)。

### 方案
- (a) **State ref 类型** —— 改 `vue.rs:11106-11109`:`const {} = ref<{}>({})`,中间插 `auto_type_to_ts_type(&sv.type_info)`。
- (b) **Handler 参数类型** —— 改 `vue.rs:11251-11253`:新增 `handler_param_types(store, pattern) -> Vec<(String,String)>`,解析 action 名 → 匹配 `store.messages[].variants` → zip `payload` 与参数名 → `auto_type_to_ts_type`。无匹配回退 `any`。
- (c) **Composable 返回 interface** —— `vue.rs:11223` `(): any` → `(): <StoreName>Store` + 文件头生成聚合 interface(state ref `Ref<T>` + actions 签名 + computed getter)。最大新增面。
- (d) **module-fn 参数类型** —— `AuraModuleFn` 补 `param_types: Vec<Type>`(改 `aura/types.rs:403` + `extract.rs`),改 `vue.rs:11394-11398`。
- (e) **自定义类型 import** —— `auto_type_to_ts_type` 对 `Type::User` 返回类型名(`vue.rs:10403`),store 文件需 `import type { Block } from '@/lib/api'`。在 `vue.rs:11096-11102` import 块扫描 ref/handler 用到的 User 类型名追加。

### 工作量 / 风险
**中**。(a)(b) 核心低风险(类型已就绪);(c) 返回 interface 新增生成面;(e) import 收集需遍历。**风险中** —— TS strict 可能暴露 store body 隐式 any 操作(`__sse_output.value.Text` 访问需 `?.`)。建议两阶段:先 ref/handler 参数(a+b),返回 interface(c)后做。

### 依赖
无前置;类型 IR 已就绪。(d) 需改 `aura/types.rs` + `extract.rs`(IR 层)。与 Plan 053 §6 C 对应。与 Phase G-b(rust.rs Rust 端)是同一问题两面,若都做应抽共享 `Type → {ts, rust}` 映射层(目前 `auto_type_to_ts_type` vue.rs 私有,`state_rust_type` rust.rs:280 各写各的)。

### 验收
`vue-tsc --noEmit` store 相关 error 清零(ref 类型化 + handler 参数类型化);`useShellStoreStore.ts` 无 `any`。

---

## Phase F: Tauri transport 抽象

### 现状
- store 直接 `new EventSource`(`vue.rs:11281`)+ 裸 fetch(import `@/lib/api` 的 fetch 函数,`vue.rs:11101`)。
- `api.ts` 用 `generate_simple_client`(`api/targets/typescript.rs:387-416`,裸 fetch)。
- **关键发现:transport 抽象已存在但未接入 store 路径** —— `typescript.rs` 有 `generate_iapi_interface`(`:122`)/ `generate_tauri_impl`(`:155`,invoke)/ `generate_http_impl`(`:207`,**axios 非 fetch**)/ `generate_api_selector`(`:269`,`isTauri ? tauriApi : httpApi`)/ `generate_all`(`:465`)—— 与 `generate_simple_client` 并存,**只有 simple-client 接入 store**。
- 检测 key 不一致:selector 用 `'__TAURI__' in window`(`:275`),手写 ash-gui-vue 用 `'__TAURI_INTERNALS__' in window`(`useShell.ts:14`)。
- SSE **完全无 transport 抽象** —— `vue.rs:11276-11336` 硬编码 EventSource,无 Tauri `listen` 分支。

### 根因
两个错位:store import simple-client 裸 fetch(非 IApi selector);SSE 硬编码无 Tauri 分支。

### 方案(方案 A,轻量,贴近手写)
- (a) **api.ts 双模式** —— 改 `typescript.rs:287 generate_fetch_function` / `:388 generate_simple_client`:头部注入 `const isTauri = '__TAURI_INTERNALS__' in window`,每函数体 `return isTauri ? invoke('fn',{...}) : fetch(...)`. 注意 Tauri command 名 + 参数 camelCase(`blockId`)vs fetch snake_case(`block_id`)转换层。
- (b) **Store SSE transport 抽象** —— 改 `vue.rs:11276-11336`:`isTauri` 分支 —— HTTP 现有 EventSource;Tauri `import { listen } from '@tauri-apps/api/event'` + `listen('command-output', ...)`(事件名从 `ep.variants` wire 值映射,手写 `useShellTauri.ts:176-182` 已有参考)。
- (c) **selector 常量** —— `vue.rs:11096-11103` 上方生成 `const isTauri = ...` 供 (a)(b) 共用。
- 方案 B(重,不推荐):store 切 IApi selector,需把 http_impl 从 axios 改回 fetch + IApi 加 SSE 契约。

### 工作量 / 风险
**中**。(a) 参数名 snake↔camel 转换是主要细节;(b) Tauri `listen` 需后端 emit 事件契约。**风险中** —— Tauri 路径需后端(axum/tauri target 或 a2r)实际 emit 这些事件;当前 ash-gui-auto 后端是 axum HTTP,Tauri 模式需额外 wire 后端事件总线。codegen 改动本身低风险。

### 依赖
取决于后端 transport 是否就绪:Tauri 模式需 `api/targets/tauri.rs`(已存在)编译进 Tauri 后端 + 后端 emit `command-output`/`command-result`。与 Plan 043 stream phase / Plan musk-022 SSE dispatch 的 `StreamEndpoint` 复用。

### 验收
Tauri 环境跑(`__TAURI_INTERNALS__` 存在)→ store 用 invoke/listen;HTTP 环境 → fetch/EventSource。两路径功能等价。

---

## Phase G: a2r(Rust 二进制)模式 — 止血 / 废弃

### 现状
- 入口:`auto run -r rust --server rust` → `auto-man/src/rust_ui.rs`(`:19` RustGenerator)→ `ui_gen/rust.rs:431 generate_rust` + `trans/rust.rs`(语句级转译)。
- README:30-35:codegen 成功但 `cargo run` **72 编译错误**,全在前端 `main.rs`(932 行)。VM 是唯一可用路径。

### 根因(72 错误模式)
| 错误 | 次数 | codegen 根点 | 根因 |
|---|---|---|---|
| E0599 不存在的 View 方法 | ~12 | `rust.rs:3320-3325` `tag_to_view_fn` thead/tbody/tr/th/td 直译 | `auto_lang::View` 只有 `View::table(headers, rows)`(`ui/view.rs:959`),codegen 把 HTML 表格子标签当独立 View 节点,无展平 |
| E0609 字段访问 | ~15 | `rust.rs:440-470` Object/Array 硬编码 `serde_json::Value`/`Vec<Value>` | store-composable + 嵌套 type 映射弱类型 Value → `Value.git_status`/`String.kind` 不存在 |
| E0425 未定义符号 | ~13 | view fn 参数/computed helper/子组件引用未 import | 跨组件作用域泄漏 |
| E0599 store 方法缺失 | ~6 | RustGenerator 不生成 store action 方法 | `PromptBar.history()` 等 List/store 方法未生成 |
| E0308 类型不匹配 | ~10 | 动态→静态翻译错 | Value 当 Vec<Value>、String 比整数 |
| move/Copy/Debug/trait | ~16 | `#[derive]` 缺、闭包 trait、生命周期 | `self.cwd` move out、Msg 不 Debug |

**核心判断**:`ui_gen/rust.rs`(5579 行)系统性落后于 `ui_gen/vue.rs`(18376 行),从未对齐。

### 方案(分级)
- **G-a 表格展平(止血,推荐先做)** —— 治 E0599 ×12。改 `rust.rs` view 节点渲染(`generate_view_method` `:1441`,节点遍历 `:1898+`):遇 `table` 节点**不**走通用 child 递归,识别 `thead/tr/th/td` 子树,收成 `headers: Vec<View<M>>` + `rows: Vec<Vec<View<M>>>`,emit 单个 `View::table(headers, rows)`(`ui/view.rs:959`)。新增 `flatten_table_node(node) -> (headers, rows)` 前置 pass。`tag_to_view_fn`(`:3321-3325`)thead/tbody/tr/th/td 分支改报错或并入展平。**自包含、风险低**。
- G-b 强类型 struct 替换 serde_json::Value(治 E0609 ×15)—— 改 `rust.rs:438-470`:对 `Type::User` 生成/引用真实 Rust struct(从 `back/api.at` 镜像 `typescript.rs:101 generate_interface` 的 Rust 对等),嵌套字段 `pc.git_status.staged` 而非 `pc["git_status"]["staged"]`。**与 Phase E 是同根两面**,若都做先抽共享 `Type → {ts, rust}` 映射层。
- G-c 作用域/import 修正(治 E0425 ×13)。
- G-d store-composable 方法生成(治 E0599 ×6)—— 补 store action 生成(镜像 vue.rs handler→action,落 Rust `impl Store`)。

### 工作量 / 风险 / 废弃判断
**整体大**(周级以上),(a) 自包含可交付。**风险高**(横跨整个 Rust backend,与 VM 成熟度差距大;a2r 用户面极窄,投入产出比存疑)。**建议**:保留 a2r codegen 不删(避免回归),**明确标 experimental**;优先只做 **G-a 表格展平**(独立、治 12 错、风险低)止血,其余等真实分发需求再评估。(b) 若做应与 Phase E 统一设计。

### 依赖
G-a 仅依赖 `ui/view.rs:959` `View::table`(已确认)。G-b 依赖 `back/api.at` 类型解析(已有 `ApiType`/`ApiModule`,需加 Rust target)。

### 验收(G-a)
a2r 模式表格相关 E0599 清零;`ls` 输出在 iced 表格视图渲染(非文本)。

---

## 实施建议

1. **快赢批次**(小、低风险,立即收益):Phase C(终端命令降级)+ D5(折叠 per-block)+ D6(renderer 读 git)。1-2 天。
2. **中等批次**(中、中风险,功能补齐):Phase B(管道流式)+ A(作业控制)+ E(store 强类型 a+b)。各 1-2 天。
3. **重型批次**(大,按需):Phase D1(crate 拆分)+ F(Tauri,依赖后端)+ G(a2r 止血/废弃)。

各 Phase 独立,可按需挑选,不强制全做。建议每 Phase 单独 commit + 验证(沿用 plan 054 模式:改 .at/codegen → regen → vite build → 端到端)。

---

## 实施进度(2026-08-12)

| Phase | 状态 | commit |
|---|---|---|
| **C** 终端命令降级 | ✅ | auto-shell `08b5d58` |
| **D5** 折叠 per-block | ✅ | auto-shell `08b5d58` |
| **D6** VM prompt_context | ⏭️ 跳过(VM 专属;vue 走 ash-server 真实 git) | — |
| **B** 管道流式(多段 OS pipe 链) | ✅ | auto-shell `47023f4` |
| **E(a)** store ref scalar 类型化 | ✅ | auto-lang `91e50ad7` |
| **A** 作业控制(A.1 后端 + A.2 前端轮询) | ✅ | auto-shell `8240bec`/`f184518` |
| **E(b/c/e)** handler 参数 / composable 返回 interface / User import | ⏳ 后续(E(a)已铺路) | — |
| **D4** 复制 / **F** Tauri / **D1** / **G** | ⏳ 按需(重型批次) | — |

**本轮验证**:C(less 放行 200)/ B(where|findstr 200,echo 单段无回归)/ E(a)(ref<string>×8 / ref<number>×5 / ref<boolean>×1,User 回退 any)/ D5(ToggleCollapse 回调链 regen)。vite build 全 exit 0。前端 5173 + ash-server :3000 在线。

