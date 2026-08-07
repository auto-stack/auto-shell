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

- [x] **M0 地基**(2026-08-07 完成,见 §9 与 auto-lang Plan 398):
      vm 启动 + MCP 12 工具连通 + `autoui_snapshot` 返回 App 树。
      M0 的 3 个 VM 兼容阻塞由 Plan 398 全部修复(parser [][]T/[](tuple) +
      sibling-handler rewrite + parse 错误 log::warn)。**注意:M0.4 测试骨架
      (conftest/desktop_mcp/test_smoke)尚未搭**——下轮 M0 收尾要做。
- [x] **M1 SSE 流式桥**(2026-08-07 完成,端到端闭环验证通过):
      renderer.rs SSE→Task subscription 桥 + Rust 执行器线程(merged std::process /
      HTTP SSE 双模式)+ vm MCP 子组件交互修复(type/submit/input_state_map/emit模拟/
      Rust 侧 block 构造更新)。**端到端闭环已验证**:type echo + submit → Success + output;
      badcmd → Failed(ash-gui 全部 8 测试绿:smoke 6 + command_exec 2)。
- [~] **M2 行为对齐**(2026-08-07 纯逻辑组完成,约 12 处;难档留下轮):
      BL-08..10 duration badge、BB-08 点击范围、BB-12 bold/italic、BB-11 usage 回退、
      TS-01 描述、HS-04 不敏感+倒序+cap50、HS-13 计数、PB-11 Ctrl+L、PB-comp-07 description、
      APP-05/06 git_label、CMD-06 cancel 首个。**14 测试(11 pass + 3 xfail)**。
      难档(PB-ghost/highlight/textarea/debounce/autofocus/continuation、BL-01 自动滚动、
      CMD-09/10 smart 失败+duration)留下轮 —— 依赖 iced 能力或 renderer 拦截。
- [~] **M3 MCP 测试套件**(2026-08-07 骨架完成,99 用例:49 pass / 47 skip / 3 xfail):
      10 个测试文件覆盖 APP/PB/BL/BB/CMD/TS/HS/BACK 行为编号。每条行为推导自 Vue 源(file:line),
      按 vm 可测性分级。**49 pass** 验证 M0/M1/M2 已实现行为;**47 skip** 占位待实现(难档 + mock 数据空);
      **3 xfail** 真实未实现(HS Ctrl+R 面板)。最大杠杆:修 EDGE-01(keyboard onkeydown emit 模拟)
      可解锁 ~20 个 skip → pass。
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

### 9.3 未解决阻塞:VM 兼容性是**级联多重缺陷**,非单一原因 ❌(二轮修正)

> **重要修正**:第一轮假设"19 个 pub type 触发 link 失败"已被第二轮证伪。
> 二轮建立了干净二分 harness(stub app.at + 极简 store + mock shell.at + 完整 api.at),
> 逐项验证后定位到真因。

**二轮二分结论(已验证)**:
- ✅ **api.at 完全没问题**:完整 19 个 pub type(含 `?T`、`[][]T`、`[](str,RenderedCell)`
  元组数组)**全部能 link**(在 stub app + 极简 store 下)。换 015 的 api.at 也能 link。
- ✅ **复杂类型特性全部支持**:`?T` 可选、`[][]T` 嵌套数组、tuple 数组都 OK。
- ✅ **`var x T = T{}` + 字段赋值的 struct-fix 模式不触发 panic**(已隔离验证)。
- ✅ **shell 模块名不特殊**(rename shbackend 仍失败)。
- ❌ **真因:real app.at + real store 触发 VM 级联多重缺陷**,逐个暴露:
  1. `Undefined symbol: handler_ShellStore_ClearScreen` — store 缺 app 要的 handler。
  2. `Undefined symbol: PromptBar_State.Exit in module App` — 子组件 state 符号问题。
  3. **`codegen.rs:6058 panic: "Assignment to complex LHS not supported yet"`** —
     VM codegen 对某种 LHS 赋值形式不支持(`Expr::Dot` 分支要求 obj 是 `Expr::Ident`)。
     触发源在 real store 某个 handler body(非 §9.1 的 struct-fix 模式——已排除)。
- **"Undefined symbol: api.complete"的真相**:它不是 link 错误本身,而是 **codegen
  panic 中途崩溃 → handler 符号未生成 → linker 报缺符号**。修复 panic 后会自然消失。

**harness(下轮直接复用,可复现)**:
```
stub app.at(无 store/children)+ 极简 store(只 .Init)+ mock shell.at + 完整 api.at
→ 链接通过(基准)。从此基准逐个加回 real 内容,定位每个缺陷。
```

**下轮 M0 的精确步骤(机械、快速)**:
1. 从 harness 基准起,加回 real store 的**逐个 handler body**(保留所有 handler 签名,
   body 一个个填),定位触发 codegen panic 的具体赋值形式。重点怀疑:
   - `.blocks[.i] = ...`(数组索引赋值 —— codegen 有 `Expr::Index` 分支,但可能对
     嵌套 .field 的元素失效)。
   - `b.status = st`(b 是 for 循环变量 —— 循环变量在 codegen 里可能不是 `Expr::Ident`)。
   - `result.status.Failed`(读取 status.Failed —— status 是 str 联合,读 .Failed 是
     复杂 RHS,但 panic 在 LHS,故优先查 LHS)。
2. 定位后:绕过(改写 .at,如用临时变量拆解),或上报 auto-lang 修 codegen.rs:6058
   (补 `Expr::Dot(non-Ident-obj, field)` 分支)。
3. 修通 store 后,逐个加回子组件(BlockList/PromptBar/ToolSidebar),定位
   `PromptBar_State.Exit` 类的子组件符号问题(可能是 state struct 命名/导出问题)。

**次要问题(已暴露,易修)**:
- `var s BootSnapshot = command_list()` → `Undefined variable: s`(VM handler 对带类型
  注解的局部绑定 + 立即用某些类型有问题)。规避:`var s = command_list()`(去类型注解)。
- handler 参数名 `s` 与某些符号冲突(用更具描述性的名)。

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

### 9.7 三轮二分(2026-08-07):定位到 3 个**真实 VM bug**,非"级联"

> 本轮(第三轮)从二轮 harness 起逐项加回,推翻了"级联多重缺陷"的模糊判断,
> 精确定位到 **3 个独立、可绕过的 VM bug**。harness 方法有效——每修一个就前进一步。

**已排除的假因(本轮证伪)**:
- ❌ store handler body 的 struct-fix 模式(`Block{}`+字段赋值、循环变量 `b.status=st`)
  → 单独测全部 link OK(tests E/F/G)。
- ❌ 后端模块名 `shell` vs `db`(rename 仍失败)、`use types:` vs `use api:`(都试过)。
- ❌ store 在 stub app 上(完整所有 handler)→ 完全 link OK(test G)。
- ❌ ash-gui api.at 本身(stub app 上完整 19 type 全 link)。

**真因(3 个独立 VM bug,每个都有 workaround)**:

**BUG-A:App 调 store.X() 时,store 的 `back.api` 导入不在 App 作用域 ❌→✅ 已绕过**
- 症状:`Undefined symbol: api.command_list in module App`(逐个 back.api fn 报)。
- 真因:VM 把 store handler body 链接到 App 模块作用域,store 的 `use back.api: ...`
  导入**不透传**到 App。015-notes 的 app.at **自己也 `use back.api: list_notes`**
  (第 7 行)——所以它能跑。ash-gui app.at 没加,故崩。
- **正解**:给 `app.at` 加 `use back.api: command_list, history, complete, run_command,
  run_smart, prompt_context, open_path`(列 app 经 store 间接用到的所有 fn)。已验证:
  加上后 back.api 类错误消失。
- 性质:**文档/惯例缺口**(auto-ui-creator skill 的 U1 应补此条)。

**BUG-B:store handler 调用**另一个** store handler `.X()` → `<Store>_State.X` 未定义 ❌**
- 症状:`Undefined symbol: ShellStore_State.RefreshGit in module App`。
- 触发:`shell_store.at` 的 `.Init` 与 `.RunResult` 内部调 `.RefreshGit()`(store 自己
  的另一个 handler)。VM 把 store handler 调用解析成 `ShellStore_State.RefreshGit`
  state-struct 符号,但该符号未生成 → link 失败。
- **正解(workaround)**:不要在 store handler 里调 `.SiblingHandler()`。把 RefreshGit
  的 body **内联**到 Init/RunResult 里(或抽成普通 module fn 由两处调用)。已验证:
  删掉两处 `.RefreshGit()` 调用,此错误消失,前进到下一个。
- 性质:**真 VM bug**(store handler 间互调未支持)。应上报 auto-lang:
  `crates/auto-lang/src/ui/vm_bridge.rs`(handler dispatch)。

**BUG-C:子组件 handler 仅被内部引用(非模板)→ `<Child>_State.<Handler>` 未定义 ❌**
- 症状:`Undefined symbol: PromptBar_State.Exit` → 修一个变 `PromptBar_State.PickCompletion`
  → 逐个 PromptBar 内部 handler 都报。**系统性**,非单点。
- 触发:PromptBar 的 `expose { .Exit }` + 内部 `.Exit()` 调用;以及所有仅在 handler
  逻辑里被调、模板未直接绑定的 handler(`PickCompletion`、`AcceptGhost` 等)。
  VM 对子组件的 state-struct handler 查找要求该 handler 被**模板直接引用**,否则
  `Child_State.Handler` 符号不生成 → App 链接失败。
- **正解(workaround,重)**:每个 PromptBar 内部 handler 都要"模板可见"——要么在
  view 里加一个隐藏的 dummy 引用(如 `if false { button { onclick: .PickCompletion(...) } }`),
  要么重构把这些逻辑挪到模板直接绑定的 handler。**这是大改**,因为 PromptBar 有
  ~10 个此类 handler(ghost/completion/keyboard 全套)。
- 性质:**真 VM bug**(与 `expose` 的设计意图冲突——`expose` 本该解决这个,但 VM
  没实现透传)。应上报 auto-lang;短期靠 workaround。

**对 M0 的结论**:
- BUG-A 已有 clean workaround(加 use back.api),下轮直接应用。
- BUG-B workaround 简单(内联 RefreshGit),下轮应用。
- **BUG-C 是真正的 M0 阻塞**:PromptBar(最复杂的组件)在 vm 模式几乎不可用,除非
  大改或修 VM。两条路:
  1. **短期**:vm 模式先跑一个 **PromptBar 简化版**(去掉 ghost/completion/highlight,
     只留基本 input + run + history),让 UI 能开;复杂功能留给 a2r/HTTP 模式。
  2. **中期**:上报 auto-lang 修 BUG-C(子组件 handler state-struct 符号生成,
     让 `expose` 真正生效),这是让 vm 模式对真实应用可用的关键修复。

**下轮 M0 精确步骤(基于本轮)**:
1. app.at 加 `use back.api: ...`(BUG-A workaround,5 分钟)。
2. shell_store.at 内联 RefreshGit、去掉 `.SiblingHandler()` 调用(BUG-B,10 分钟)。
3. 决策点(BUG-C):选"简化 PromptBar"(快速通 UI)还是"上报修 VM"(彻底但慢)。
   建议先简化通 UI(MCP 连通 + smoke),修 VM 列为 M1 并行任务。
4. 通 UI 后立即搭 tests 骨架(M0.6)。

**本轮 harness 复现脚本(下轮直接用)**:
```
stub app.at(无 store/children)+ 完整 store + mock shell.at → link OK(基准)。
加 use back.api 到 app → back.api 错误消失。
逐个加回子组件 → 命中 BUG-C 的 `<Child>_State.<Handler>`。
```

## 10. M1 实测发现(2026-08-07,SSE 流式桥)

M1 在 auto-lang worktree `plan-ash-gui/m1-sse-bridge` 完成。本节记录架构决策、
已验证部分、以及阻塞端到端验证的 vm 缺陷。

### 10.1 架构决策(用户确认)

| 维度 | 决策 | 理由 |
|---|---|---|
| 命令执行位置 | **Rust 执行器线程**(renderer 侧 std::process) | UI VM 不注入 ShellHost;复用 mcp_action_subscription 的「全局 channel + 后台线程」模式 |
| handler 参数 | **预置字段 + 无参 handler** | VM `push_value` 对 struct 参数只推占位 0(vm_bridge.rs:929);renderer 用 write_state 写 `__sse_*` 预置字段,handler 改无参读 |
| merged vs HTTP | **两者都做** | merged(默认,broadcast/std::process)+ HTTP(`AUTO_BACKEND=http://...`,reqwest SSE 客户端连 /api/stream) |

### 10.2 已实现 + 已验证 ✅

- **renderer.rs SSE 桥**(~340 行新增,renderer.rs:2132-2478):
  - `ShellStreamEvent` / `PendingShellCommand` / `ShellExecutorHandle` 数据结构
  - `start_shell_executor()` 启动执行器线程(仿 mcp_server current_thread runtime)
  - `merged_exec_loop`:std::process::Command 执行,stdout 逐块流推 command_output,
    退出码→command_result(Success/Failed/Cancelled),对齐 ash-server 契约
  - `http_sse_loop`:reqwest bytes_stream + 手写 SSE 帧解析,连后端 /api/stream;
    命令提交/取消走 POST
  - `shell_event_subscription`:16ms poll SHELL_EVENT_RX → IcedMessage
  - run_dynamic_iced 启动接线(MCP_ACTION_RX 初始化旁)+ subscription 闭包注册
  - update 闭包 command_output/command_result 分支(write_state 预置字段 + 无参
    on_with_input_for("ShellStore", "RunOutput"/"RunResult"))+ RunCommand/Cancel 拦截
- **shell_store.at**:加 `__sse_*` / `__pending_command_*` 预置字段;RunCommand 写预置
  字段(不再调后端);RunOutput/RunResult 改无参读预置字段;msg 枚举改无参
- **shell.at 修 M0 遗留类型 bug**:`[]T` 空字面量误推断为 T 类型导致 push 失败 →
  改用 `List<T>.new([])`(015-notes 模式);prompt_context 显式构造 git_status
- **执行器单元测试通过**(`test_shell_executor_success_and_failure`):
  echo→Success、nonexistent_cmd→Failed、block_id 正确、事件经 channel 产出
- **smoke 测试不退化**(6/6 绿);store.Init 成功(cwd 字段有值)

### 10.3 vm MCP 子组件交互修复(2026-08-07,5 个互补修复)✅

原 §10.3 记录的阻塞已解决。vm 模式命令执行端到端闭环跑通,需要 5 个互补修复
(auto-lang worktree `plan-ash-gui/mcp-vnode-action`):

1. **tool_type 支持 vnode_**(mcp_server.rs):parse_element_id 替代 parse_aura_id
   (支持 aura_N + vnode_N);无 element_id 时从 styled_vtree 找首个 Input vnode
   (find_first_input_vnode),而非未展开的 view_template。
2. **submit action**(mcp_types.rs + mcp_server.rs + action_mapper.rs):UiActionType::Submit
   触发 Input on_submit(Enter 语义);execute_action_vnode 从 target_view 读 value
   作 handler 参数(模拟 `onenter: .Run(.input)`)。
3. **input_state_map 递归子组件**(dynamic.rs):extract_input_state_map_with_registry
   扫描 root + 所有注册子组件 view_tree;scan_node_for_inputs 支持
   `Expr::Dot(Ident("self"), field)`(.input 解析形式,非 Expr::Ident)。
4. **emit 模拟**(renderer.rs):handler_codegen 剥离子组件 callback prop 调用
   (handler_codegen.rs:996 Plan 370 D-GAP-4),PromptBar.Run → App.RunCommand 的自动
   emit 在 vm 不发生;renderer 在 PromptBar.Run 后用 cmd 值直接触发 store.RunCommand。
5. **Rust 侧 block 构造/更新**(renderer.rs):VM 对嵌套 struct 赋值
   (`block.status = BlockStatus{}`)在连续 handler 调用下崩溃(Stack Underflow);
   store.RunCommand 只记 pending{block_id,cmd},block 由 renderer 用 auto_val::Obj
   构造 + update_block_in_state 直接更新(streamed_text/status/output)。

### 10.4 对计划的影响

| 原 M1 假设 | 实测 | 调整 |
|---|---|---|
| M1 = SSE 桥代码 | ✅ 完成 + 5 个 vm 交互修复 | 端到端闭环验证通过 |
| M1 验收 = CMD-01..12 在 iced 成立 | ✅ 核心(CMD-01/02 Success/Failed) | 其余 CMD 用例留 M3 测试套件 |
| shell.at 是空 mock | 部分 | 修了类型 bug;真执行由 renderer 执行器接管 |

### 10.5 M1 产物清单

- auto-lang worktree `plan-ash-gui/m1-sse-bridge`(renderer.rs SSE 桥 + 执行器 + 单元测试)
- ash-gui-auto `shell_store.at`(预置字段模式 + 无参 handler)
- ash-gui-auto `shell.at`(类型 bug 修复 + prompt_context 完善)
- ash-gui-auto `tests/test_command_exec.py`(端到端测试,待 §10.3 修复后转绿)
- 本节(§10)发现归档


