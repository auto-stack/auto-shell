# Plan: ash-gui 原生 iced 运行 + AutoUI MCP 测试套件

> 目标(用户原文):让 Auto 版 ash-gui 通过 a2r 或 vm 方式跑起基于 iced 的
> `render=rust`/`render=vm` 界面,UI/UX 与 Vue 版基本一致;并设计一套基于
> AutoUI MCP 的测试套件,覆盖 ash-gui 的所有操作和行为。
>
> 本计划基于三轮定向调研(渲染管线 / MCP 协议 / 行为目录),所有结论带
> `file:line` 证据,见文末「调研证据」。

## 0. 关键事实(调研结论,改变原假设)

1. **`render: "iced"` 是死枚举**——`auto-man/src/automan.rs:1056-1134`(gen)
   与 `1288-1362`(run)都没有 `BackendType::Iced` 分支。真正的原生 iced 路径
   只有两个:
   - **a2r**(`render: "rust"`):`.at` → 生成 `main.rs`+`Cargo.toml` → `cargo run`
     → iced 二进制。产物在 `examples/rust-workspace/<member>/`,**不是** `back/`
     (`back/` 是 API 后端目录)。store-composable 应用更成熟(已在 015-notes 验证)。
   - **vm**(`render: "vm"`):**不产生序列化产物**,而是 `auto_lang::run_file()`
     在运行时解析 `.at` → `DynamicComponent` → `run_dynamic_iced`。Shell 场景的
     默认(`session.rs:155-159`)。handler 跑在**真 VM Codegen** 上,不是树遍历。
   - 两条路径**共用同一个 iced 渲染器** `auto-lang/src/ui/iced/renderer.rs`(339KB)。

2. **#1 阻塞:`~Stream<ShellEvent>` SSE 契约两条原生路径都不消费。**
   `ash-gui-auto/src/back/api.at:218-230` 声明 `pub fn stream() ~Stream<ShellEvent>`,
   这是 Vue(`EventSource('/api/stream')`)/Tauri(`listen()`)的 codegen 契约。
   原生 iced 既无 SSE 客户端,也无 subscription 桥——`.RunOutput`/`.RunResult`
   永远到不了 UI,block 会永远停在 Running。**必须新建 SSE→iced::Task 桥。**

3. **Tailwind 样式是原生路径的强项**——`auto-lang/src/ui/style/{class.rs(46KB),
   iced_adapter.rs(32KB)}` 完整解析 ash-gui 用的工具类,含运行时 dark-mode/accent
   token 解析。UI/UX 一致性可行。

4. **MCP UI 服务端已建好**(Plan 278/299/314):嵌在 iced 进程内的后台线程,
   HTTP `POST http://127.0.0.1:9247/mcp`,JSON-RPC 2.0,**12 个 `autoui_*` 工具**
   (snapshot/inspect/action/check/screenshot/state/wait/type/keyboard/vtree/find/exists)。
   vm 模式(`renderer.rs:2584`)和 rust 模式(`renderer.rs:6432`)都启动它。

5. **Vue→Auto 已发现 27 处行为差异**(ghost text、语法高亮、continuation、
   git label 格式、路径缩写、history 合并、cancel 停所有 vs 首个、smart 失败路径
   等),每处带 Vue 源 `file:line`。

## 1. 决策(默认,用户未答时按此推进;可在 ExitPlanMode 前调整)

| 维度 | 决策 | 理由 |
|---|---|---|
| 运行路径 | **vm 为主(开发+测试),a2r 为辅(可分发二进制)** | vm 是 Shell 默认、热重载快、MCP 测试直接打它;a2r 出二进制。共用渲染器,修一处通两处 |
| 测试形态 | **Python(pytest),复用 013/015 的 desktop_mcp.py 模式** | 落地最快,与社区示例一致;可选追加 ZCode skill 做探索式 |
| 差异修复 | **27 处全修** | "UI/UX 基本一致"是硬要求;测试套件逐条验证 |
| SSE 方案 | **iced SSE→Task subscription 桥(保 .at 源不改)** | 保 `~Stream<T>` 契约,前端 .at 零改动;研究指明的正解 |

## 2. 工作分解(5 个阶段,M0–M4)

### M0 — 地基与可观测(打通"能跑+能看")

**目标**:ash-gui-auto 能以 vm 模式启动 iced 窗口,MCP 服务端可连,能取到快照。

- M0.1 **pac.at 切换**:把 `ash-gui-auto/pac.at` 的 `render: "vue"` 改 `"vm"`。
  建一个 `pac.rust.at`(或环境变量切换)给 a2r 路径用 `render: "rust"`。
- M0.2 **冒烟启动**:在 `ash-gui-auto/` 跑 `auto run`(走 `run_vm_ui`,
  `rust_ui.rs:2160`),预期:iced 窗口打开,标题栏/侧栏/空态可见,但跑命令会卡在
  Running(因 SSE 未接,符合预期)。记录所有 panic/编译错误。
- M0.3 **MCP 连通性**:用 Python `requests.post('http://127.0.0.1:9247/mcp', ...)`
  调 `tools/list`(预期 12 工具)、`autoui_snapshot`、`autoui_vtree`,确认能取到
  widget 树。这是后续所有测试的前置。
- M0.4 **最小测试骨架**:在 `ash-gui/ash-gui-auto/tests/` 建 `conftest.py` +
  `desktop_mcp.py`(从 015-notes 拷贝并泛化:启动子进程、`wait_for_server`、
  `wait_for_snapshot`),写 1 个 `test_smoke.py`(能启动 + snapshot 含 "ash" 或
  空态文案)。

**验收**:窗口能开,MCP 12 工具可调,smoke 测试绿。

### M1 — SSE 流式桥(核心阻塞,打通命令执行闭环)

**目标**:命令输出/结果能流式到达 iced UI,block 正常 Success/Failed/Cancelled。

这是唯一需要动 **auto-lang 代码** 的阶段(其余主要在 ash-gui-auto 与测试)。

- M1.1 **设计 SSE→iced 桥**:在 `auto-lang/src/ui/iced/renderer.rs` 增加一个
  `subscription` 分支(iced 的 `application(boot, update, view)` 可加第 4 参
  `subscription`)。当 `DynamicComponent` 检测到后端有 `~Stream<T>` 端点时,
  用 `reqwest` + `eventsource-stream`(或 `tokio` 手写 SSE 解析)订阅
  `/api/stream`,把每条 SSE 事件转成 `IcedMessage`,经 `update` 闭包派发到
  store 的 `.RunOutput`/`.RunResult`。
- M1.2 **契约发现**:让 vm 启动时从后端 `api.at` 元数据(或 pac.at)发现
  `~Stream` 端点路径与事件枚举名,避免硬编码 ash-gui。做成通用 AutoUI 特性。
- M1.3 **merged 模式**:vm 默认 merged(后端 in-process),此时不走 HTTP SSE,
  而是用 `tokio::sync::broadcast`/`mpsc` 把后端 `shell.subscribe()` 的事件直接
  喂给 iced subscription。HTTP 模式(`AUTO_BACKEND_IMPL` 非 merged)才走真 SSE。
- M1.4 **a2r 路径**:a2r 生成的 `main.rs`(`rust_ui.rs:1450 wrap_example`)同样
  注入 subscription;`generate_api_client`(`rust_ui.rs:620`)对 `~Stream` 端点
  生成 subscription 接线而非 fire-and-forget。
- M1.5 **ash-gui 验证**:跑 `ls` → 看到 streamedText 增长 → 最终 Success +
  output 渲染;跑失败命令 → Failed + ✗;点 ■ stop → Cancelled + ⊘。

**验收**:CMD-01..CMD-12 行为(见行为目录)在 iced 上全部成立。

### M2 — 行为对齐(修 27 处 Vue→Auto 差异,使 UI/UX 一致)

**目标**:Auto 版行为与 Vue 版逐条一致。按行为目录分组推进,每组改 .at + 写测试。

> 改动主要在 `ash-gui-auto/src/front/*.at` 与 `src/back/api.at`。多数是 .at 层
> 逻辑/模板补全,不动 auto-lang。下表标 ⚠️ 表示可能需 auto-lang/iced 支持。

| 组 | 差异(编号见行为目录) | 改动位置 | 备注 |
|---|---|---|---|
| **状态格式化** | APP-05/06 git label;APP-03/BL-17/EDGE-10 路径缩写(`~`、`\`→`/`) | `shell_store.at`(computed `git_label` 实现)、新增 `lib/path.at` 或 view fn | computed 须浅(陷阱 U8),复杂逻辑放 handler 写 model 字段 |
| **history** | PB-hist-05 合并 session+persisted;HS-04 大小写不敏感+倒序+cap50;HS-01 open 重置;HS-13 计数 | `shell_store.at`、`history_search.at` | `to_lower` 未映射(DEBT)→ ⚠️ 可能需 iced/ts_adapter 或用 `to_string` 比较 |
| **PromptBar 核心** | PB-01 autofocus;PB-05..10 continuation(`needsContinuation`);PB-15 textarea 自增长(现用单行 input);PB-11..14 Ctrl+L/C/D | `prompt_bar.at` | continuation 是纯逻辑;textarea multiline ⚠️ 需 iced textarea 多行支持确认 |
| **completion** | PB-comp-01 debounce 80ms + 序号守卫 + slice8;PB-comp-02 本地 fallback;PB-comp-07 描述/标题 | `prompt_bar.at` | debounce 在 .at 里用 handler 计时或 ⚠️ 需 iced timer subscription |
| **ghost text** | PB-ghost-01..06(Ctrl+F / Ctrl+Right) | `prompt_bar.at` | 现为空 stub;补 ghost 计算 + 渲染 + 绑定 |
| **语法高亮** | PB-high-01..09(tokenize 着色覆盖层) | `prompt_bar.at` + 移植 `lib/highlight.ts` → `view fn` 或 computed | ⚠️ 透明 textarea 叠覆盖层在 iced 里要确认可行性(可能用 text span 着色) |
| **injected** | PB-inj-01 发 `injected` + focus + 选区 | `prompt_bar.at` | relay 已有,补 emit 与 focus |
| **侧栏/块** | TS-01 描述;BL-01 自动滚动;BL-08..10 duration badge;BL-12 copy catch;BL-17 已归状态格式化 | `tool_sidebar.at`、`block_list.at`、`block_item.at` | 自动滚动 ⚠️ 需 iced scroll subscription 或 watch |
| **渲染器** | BB-08 仅 Dir/FileName 可点;BB-11 memory progress 标签 + usage 回退;BB-12 bold/italic;BB-14 ✗ glyph+样式 | `block_body.at` | 逻辑修正为主 |
| **store 语义** | CMD-06 cancel 只停首个;CMD-09/10 smart 失败路径+durationMs | `shell_store.at` | 纯逻辑 |
| **退出** | APP-11 Ctrl+D `window.close()` | `app.at` | ⚠️ iced `window::close` 任务 |

**验收**:27 处差异各有对应测试通过(见 M3)。

### M3 — MCP 测试套件(95 条行为 → pytest)

**目标**:覆盖行为目录全部 ~95 条,回归保护。

- M3.1 **客户端封装** `tests/desktop_mcp.py`:在 015 版基础上升级——
  - 优先用 `vnode_N` + `autoui_find`/`exists` 定位(robust),非旧的 `aura_`+正则。
  - 封装:`snapshot_json()`(解析 AURA 文本为结构)、`find_by_label(kind,label)`、
    `click_label(...)`、`type_into(text, label=None)`、`key("Enter")`、
    `state(*fields)`、`wait_until(fn, timeout)`、`screenshot(name, diff=True)`。
  - 因为工具返回纯文本,封装层做正则/Atom 解析。
- M3.2 **fixture**:`conftest.py` 提供 `app` session fixture(启动 `auto run` 子进程,
  wait_for_server,结束清理)、`mcp` fixture(返回 client)。
- M3.3 **测试文件**(按行为目录章节,每章一文件):
  - `test_app_shell.py`(APP-01..15)
  - `test_prompt_input.py`(PB-01..15)
  - `test_prompt_history.py`(PB-hist-01..06)
  - `test_prompt_completion.py`(PB-comp-01..07)
  - `test_prompt_ghost.py`(PB-ghost-01..06)— 依赖 M2 ghost 修复
  - `test_prompt_highlight.py`(PB-high-01..09)— 依赖 M2 高亮
  - `test_prompt_injected.py`(PB-inj-01)
  - `test_history_search.py`(HS-01..13)
  - `test_tool_sidebar.py`(TS-01..05)
  - `test_block.py`(BL-01..18)
  - `test_blockbody.py`(BB-01..14)
  - `test_command_lifecycle.py`(CMD-01..12)— 依赖 M1 SSE 桥
  - `test_backend.py`(BACK-01..12)— 启动/boot/SSE 集成
  - `test_edge_cases.py`(EDGE-01..14)
- M3.4 **快照回归基线**:对关键屏(空态、跑过 `ls` 后、HistorySearch 打开、
  Error 块)用 `autoui_screenshot baseline=True` 建基线,后续 `diff=True` 比对
  (threshold 默认 0.01)。放 `tests/screenshots/`。
- M3.5 **expected-fail 机制**:M2 未修完前,相关测试标 `@pytest.mark.xfail`,
  修一条转一条为 pass,进度可视化。
- M3.6 **a2r 路径参数化**:`@pytest.mark.parametrize("mode", ["vm", "rust"])`
  在稳定后对核心用例跑双路径(共用渲染器,应同结果)。

**验收**:`pytest -q` 全绿(vm 模式);a2r 模式核心用例绿;截图基线齐。

### M4 — a2r 可分发二进制 + 收尾

**目标**:产出独立 iced 二进制,文档齐。

- M4.1 **a2r 冒烟**:`pac.at` 切 `render: "rust"`,`auto gen` → `auto run`,
  修 SSE subscription 在生成代码里的接线(M1.4)。验证与 vm 行为一致。
- M4.2 **打包**:`cargo build --release --features ui-iced` 产出独立二进制;
  记录运行依赖(后端 in-process merged vs HTTP)。
- M4.3 **文档**:
  - `ash-gui-auto/README.md`:vm/a2r 两条运行方式、MCP 端口、测试运行法。
  - 在 `designs/` 写归档文档:SSE 桥设计、27 处差异清单与修复、测试覆盖矩阵。
- M4.4 **DEBT/TODO 同步**:把 ash-shell 的 `DETS.md`/`TODO.md` 里相关项更新。

## 3. 关键技术风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| SSE→iced subscription 桥无先例 | M1 阻塞 | 先做 merged 模式(broadcast channel,最简),HTTP SSE 次之;参考 iced `subscription` API |
| iced 无透明 textarea 叠覆盖层 | 语法高亮(PB-high)无法复刻 | 备选:iced text span 着色(RGB 内联样式已有 `style_obj` 支持);最差降级为不高亮,M2 标 xfail |
| `to_lower` 未映射(已知 DEBT) | history 大小写不敏感(HS-04)做不了 | 先用 `to_string` 双向比较或两侧 `contains`;必要时给 ts_adapter/iced 加 1 行映射 |
| MCP keyboard 是 handler 名派发,非真按键 | Ctrl 组合/Tab 等可能打不到 | 行为目录键盘项先确认 handler 名;缺的 key_* handler 在 .at 补;极端情况直接调对应 action |
| a2r 对 store-composable 生成可能有残留 bug | M4 二进制跑不起 | 015-notes 已验证,风险中;遇错回灌 `ui_gen/rust.rs` |
| 三端(vue/vm/a2r)行为漂移 | 长期维护负担 | 测试套件主跑 vm,a2r 参数化;Vue 版仍由现有 gui-test-screenshots 守护 |

## 4. 调研证据索引(关键 file:line)

**渲染管线**
- 后端枚举/目录:`auto-lang/crates/auto-lang/src/config.rs:145-193`
- 编排派发(无 Iced 分支):`auto-man/src/automan.rs:1056-1134`(gen)、`1288-1362`(run)
- a2r 生成:`auto-man/src/rust_ui.rs:323`(gen)、`2072`(run)、`1450`(main.rs)、`1644`(Cargo.toml)
- vm 运行:`rust_ui.rs:2160`;`auto-lang/src/lib.rs:2614,2368,2384`
- DynamicComponent/VmBridge:`ui/dynamic.rs:88`、`ui/vm_bridge.rs:105,800,831`
- 真 iced 渲染器:`ui/iced/renderer.rs:2468`(run_dynamic_iced)、`2332`(DynamicState)、`2660`(update)、`3455`(dynamic_view)、`1671`(IcedMessage)
- Shell 默认 vm:`auto-lang/src/session.rs:155-159`
- Tailwind 解析:`ui/style/{class.rs:438, iced_adapter.rs}`

**MCP UI 服务端(12 工具)**
- 服务端:`ui/mcp_server.rs`(工具注册 `480-828`,派发 `834-850`,HTTP `392-473`,启动 `2391-2413`)
- 工具:`autoui_snapshot`/`inspect`/`action`/`check`/`screenshot`/`state`/`wait`/`type`/`keyboard`/`vtree`/`find`/`exists`
- 类型:`ui/mcp_types.rs:95-137`(`UiActionType`、`ActionResult`)
- VNode/VTree:`ui/vnode.rs:31-264,373-382`;ComputedNode:`ui/debug/inspector_cache.rs:54-71`
- 双 MCP 服务端区分:`docs/specs/auto-lang/mcp/design/dual-mcp-servers.md`
- 参考客户端:`examples/ui/015-notes/tests/desktop_mcp.py:74-102`

**SSE 契约(阻塞点)**
- `ash-gui-auto/src/back/api.at:218-230`(`pub fn stream() ~Stream<ShellEvent>`)
- 消费:`shell_store.at:83`(`.RunOutput`)、`94`(`.RunResult`)

**27 处差异 + 95 条行为**
- 见 `ash-gui-auto/src/front/*.at` vs `ash-gui-vue/src/**`(逐条 file:line 在行为目录)

## 5. 里程碑与验收总表

| 里程碑 | 产物 | 验收 |
|---|---|---|
| **M0** 地基 | pac.at 双模式、tests 骨架、smoke | vm 窗口开 + MCP 12 工具可调 + smoke 绿 |
| **M1** SSE 桥 | renderer subscription + a2r 接线 | CMD-01..12 在 iced 成立 |
| **M2** 行为对齐 | 27 处 .at 修复 | 27 条对应测试绿 |
| **M3** 测试套件 | 14 文件 ~95 用例 + 截图基线 | `pytest -q` 全绿(vm);a2r 核心绿 |
| **M4** 二进制+文档 | release 二进制 + README + 归档 | a2r 独立跑通;文档齐 |

## 6. 非目标(明确不做)

- 不改 Vue 版(ash-gui-vue)——它是参照基准。
- 不做 Jet/ArkTS/Godot/GPUI 后端。
- 不重写 MCP 服务端(复用现有 12 工具;仅在确有缺口时最小补)。
- 不做性能优化、动画对齐像素级——"基本一致"指行为与视觉布局,非像素完美。
- 不在本轮做 ZCode skill 形态的探索式测试(M3 选 Python;skill 列后续可选)。

## 7. 执行顺序与依赖

```
M0(地基) ──> M1(SSE 桥,阻塞解除) ──> M2(行为对齐,可与 M3 交错)
                                        │
                                        v
                                     M3(测试套件,逐条验证 M2)
                                        │
                                        v
                                     M4(a2r 二进制 + 文档)
```

M2 与 M3 交错:每修一组差异,立即写对应测试转绿。M1 必须先于 M3 的
`test_command_lifecycle.py`/`test_backend.py`。

## 8. 进度跟踪

- [~] M0 地基(pac.at 切换 + 冒烟 + MCP 连通 + 测试骨架)— **进行中,已发现关键阻塞,见 §9**
- [ ] M1 SSE 流式桥
- [ ] M2 行为对齐(27 处)
- [ ] M3 MCP 测试套件(~95 用例)
- [ ] M4 a2r 二进制 + 文档

## 9. M0 实测发现(2026-08-07,关键,改变 M0 工作量评估)

冒烟启动 `auto run -r vm`(ash-gui-auto)暴露 **3 个真实阻塞**,计划原假设
"切 pac.at 即可跑"不成立。对照 015-notes(同环境 `auto run -r vm` 正常开窗 +
MCP 监听)逐一诊断:

### 9.1 已修复:VM handler_codegen 不支持完整 struct literal ✅

- **症状**:`handler_codegen: ShellStore..RunCommand failed: Undefined variable: self`
  (RunSmart 同)。
- **根因**:VM handler_codegen 对**状态结构体的 `Type{ field: val }` 完整字面量**
  解析失败。已用最小探针(`/tmp/ash-probe`)复现并验证。
- **正解**:`var x T = T{}` 空字面量 + 字段赋值 `x.f = ...`(已验证可行)。
- **已改**:`shell_store.at` 的 RunCommand / RunResult / Cancel / RunSmart 四处
  Block{...}/BlockStatus{...}/RenderedOutput{...} 全部改写为此模式。
  (015-notes 规避此限制:从不构造字面量,只调后端 fn 再 `.notes = list_notes()`。)

### 9.2 已定位:VM merged 模式无 in-process 后端 ⚠️(部分缓解)

- **症状**:`link failed for 'App': Undefined symbol: api.complete in module App`
  (先是 cancel,移除后变 complete —— 逐个 api 函数都 link 不到)。
- **根因**:ash-gui 的真后端是 Rust crate `ash-core`,api.at 的 `shell.X()` 在
  Vue/Tauri 走 HTTP/invoke 没问题;**VM merged 模式需要 in-process 后端**,
  而 `back/` 目录只有契约 api.at,没有实现。
- **缓解尝试**:新增 `src/back/shell.at`(纯 .at mock 后端,对齐 015 的 db.at 模式),
  并在 api.at 加 `use shell`(015 api.at 有 `use db`,ash-gui 缺失)。**但仍未 link 通过**
  —— 见 §9.3。
- **结论**:ash-gui 的 vm 运行需要一份 in-process 后端实现(mock 或桥接),这是
  比"切 pac.at"更深的工作。**已写入计划的 M1 需求**(原 M1 只讲 SSE,现扩展为
  "in-process 后端 + SSE 桥")。

### 9.3 未解决阻塞:api.at 整体 link 失败 ❌(定位到 19 个 pub type)

- **症状**:`Undefined symbol: api.complete in module App`(移除 complete 变
  command_list,逐个 api 函数都 link 不到)。
- **关键二分结果(本轮重大进展)**:
  - ✅ **极简 api.at**(只保留 BootSnapshot + ToolEntry + SmartCommandEntry 三个
    type + `command_list()` 一个 fn,调 `shell.command_list()`)→ **link 通过**
    (api.* 错误消失,进入下一个问题)。
  - ✅ 换用 **015-notes 的 back/api.at + db.at** → link 也通过(错误变成别的)。
  - ❌ ash-gui 完整 api.at(19 个 pub type)→ link 失败。
- **结论**:**ash-gui 的 19 个 pub type 中有(至少)一个触发 VM link 失败**。
  最可疑的复杂特性(015 都没有):`?T` 可选字段、`[][]T` 嵌套数组、
  `[](str, RenderedCell)` 元组数组。完整列表见 `src/back/api.at`。
- **已排除**:`~Stream<T>`(注释掉仍失败,非主因);缺 `use shell`(加了仍失败);
  pac.at 的 api/render 字段(与 015 一致);front/types.at 重名(禁用仍失败);
  store 的 use back.api(极简 store 也失败)。
- **下一步(下轮 M0,精确二分)**:保留 `use shell` + mock shell.at + 极简 store,
  在 api.at 里**逐个加回 pub type**(顺序:简单 → 复杂):
  1. 先加回简单 type(Block、BlockStatus、CompletionItem、PromptContext、
     GitStatusInfo、ToolEntry、SmartCommandEntry、BootSnapshot、CodeSpan)。
  2. 再加回带 `?T` 的(RenderedOutput、RenderedCell、TaggedCell、TableOutput、
     CodeOutput、ErrorOutput、RecordOutput、CommandResult、ShellEvent)。
  3. 重点验证元组数组 `fields: [](str, RenderedCell)`(RecordOutput)与
     嵌套数组 `rows [][]RenderedCell`(TableOutput)、`lines [][]CodeSpan`(CodeOutput)。
  - 每加一个 `auto run -r vm` 测一次,定位到首个破坏 link 的 type/特性。
  - 这是 VM 侧类型系统的边界,定位后可绕过(改 type 形状)或上报 auto-lang 修 VM。
- **次要问题(同轮暴露,易修)**:`var s BootSnapshot = command_list()` 在 store
  handler 里报 `Undefined variable: s`——VM handler_codegen 对**带类型的局部
  变量绑定 + 立即用**某些类型也有问题。规避:`var s = command_list()`(去掉显式
  类型注解,让 VM 推断),或 `var s = command_list()  .cwd = s.cwd` 拆开。

### 9.4 已验证:MCP 基础设施在 vm 模式正常 ✅

- 最小探针(`/tmp/ash-probe`,单 widget Counter)`auto run -r vm` 启动后,
  `AutoUI MCP: listening on http://127.0.0.1:9247`,`tools/list` 返回 12 工具,
  `autoui_snapshot` 可调。**测试套件的方法成立**——只要 ash-gui 能 link 通过。

### 9.5 对计划的影响

| 原 M0 假设 | 实测 | 调整 |
|---|---|---|
| 切 pac.at → vm 能跑 | ❌ 崩(handler struct literal + api link) | M0 先修这 2 个,再谈 MCP/骨架 |
| M1 = SSE 桥 | 部分对 | M1 扩展为「in-process 后端实现 + SSE 桥」(§9.2) |
| 测试套件等 vm 跑起来 | ✅ 方法成立(§9.4) | 可先在最小探针上搭骨架,ash-gui link 通后迁移 |

### 9.6 本轮已落地的产物

- `src/front/shell_store.at`:4 处 struct literal 改写(§9.1,正解,应保留)。
- `src/back/shell.at`:新增 mock 后端(§9.2,方向对,待 link 通后完善)。
- `src/back/api.at`:加 `use shell` + 注释 stream()(待 M1 恢复)。
- `designs/ash-gui-native-plan.md`:本节(§9)发现归档。
- `tests/`:下轮搭骨架(本轮因 link 阻塞,先在 /tmp 探针验证 MCP 方法)。

