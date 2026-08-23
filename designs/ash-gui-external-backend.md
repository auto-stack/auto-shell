# 设计:外部后端配置(external backend)—— ash-server Auto 化与启动形式自由

- 日期:2026-08-23
- 提案人:用户(架构裁定);调研与文档:plan 060 第十六轮会话
- 状态:**设计定稿,待立项**(实施计划见 `docs/plans/061-external-backend.md`)
- 上游:Plan 060(api.at 契约归一 / M3 手写宿主 ash-runner)、Plan 057(VM+HTTP
  一等模式)、auto-lang Plan 212b(`use.rust` FFI 桥机制存量)
- 跨仓:auto-shell(ash-server / ash-gui-auto)+ auto-lang(pac.at 配置、
  `auto run` 装载编排)

## 0. 用户原始裁定(2026-08-23)

> ash-server 本身做成 Auto 项目(自带 api.at),是"纯后端"项目。对外接口
> 是 api.at,可根据启动参数选择以 HTTP 形式(独立服务)还是 merged 形式
> (VM 直接动态链接库加载)启动。ash-gui 的 back/ 不需要自己写,而是通过
> pac.at 配置成指向 ash-server 项目作为它的 back/。AutoUI 前后端分离
> 架构需要支持"外部后端配置"。

## 1. 现状与问题

| 事实 | 出处 | 问题 |
|---|---|---|
| api.at 契约长在**前端**项目里 | `ash-gui-auto/src/back/api.at` | 实现者(ash-server)不拥有契约,漂移靠人肉对齐 |
| shell.at 是空桩 | plan 060 M3 | 前端为自己不实现的东西保留假桩 |
| merged 入口 = 手写宿主 ash-runner | `ash-server/src/bin/ash-runner.rs` | 宿主桥注册表(`register_bridges`)手写;`auto run -r vm` 裸入口失效,启动形式不自由 |
| ash-server 是纯 Rust 项目 | 无 pac.at/.at | 与"契约即接口"的 AutoUI 架构不匹配 |
| HTTP 与 merged 共享 worker | plan 060 M3 | ✓ 唯一做对的部分:语义零分叉 |

## 2. 目标架构

```
ash-server(Auto 项目,"纯后端")
├── pac.at                 # 项目声明;build 产物约定
├── api.at                 # 契约本体(从前端 back/ 迁入)——对外唯一接口
├── src/back/*.at          # 薄绑定层:api.at 函数体 → use.rust 调 Rust 实现
└── src/(Rust)             # worker.rs/http.rs/types.rs 等既有实现,原样保留

构建/启动(同一项目,两种部署参数):
  auto run --http      → 起 axum 独立服务(= 现有 ash-server bin 行为不变)
  auto run -r vm       → 宿主读 pac.at 找到后端项目 → libloading 装载其
                          cdylib → 调注册入口 → merged 形态(进程内直调)

ash-gui-auto(前端)
├── pac.at               # back: 指向外部后端项目(见 §4 配置语法)
└── src/back/            # 清空(或仅留本地 mock 开关);不再有 shell.at 假桩
```

核心原则:
- **契约归实现者所有**:api.at 住在 ash-server;前端只是消费方。
- **启动形式 = 部署参数,不是架构分叉**:HTTP/merged 的唯一差异仍是传输层
  (plan 060 §0 原则的自然延伸:fetch vs 进程内,现补第三态 cdylib 直调)。
- **ash-runner 降级为过渡产物**:其 `register_bridges()` 逻辑迁入 cdylib
  导出后,`auto run -r vm` 恢复为一等 merged 入口,run_vm.ps1 回归薄封装。

## 3. merged 的链接机制:整体 cdylib + 注册入口(非逐函数 wrapper)

**明确不做**逐函数 FFI wrapper(Plan 212b 的 `{crate}_wrapper` 路线)——
ash-core 的富类型(Shell 会话句柄/RenderedOutput/异步流式)跨 C ABI 逐签名
marshalling 成本高,且契约本就只有 10 个端点。改为**后端项目整体编译为
cdylib,暴露一个插件注册入口**:

```rust
// ash-server cdylib 导出(草案,ABI 细节计划内定稿)
#[no_mangle]
pub extern "C" fn auto_backend_register(host: *const HostVtable) -> i32;
// HostVtable:宿主提供给插件的回调表
//   - register_host_call(name, fn_ptr)   // 复用既有 host_bridge 注册表
//   - inject_event(tag, json)            // 复用 inject_shell_event(SSE 泵)
//   - log/abort 等辅助
// 注册的函数签名沿用 host_bridge 现状:fn(&str args_json) -> Result<String>
```

机制存量核对(2026-08-23 代码核证):
- 注册表:`auto_lang::vm::host_bridge::register_host_call` ✓(M3 已验证)
- 事件回流:`ui::iced::renderer::inject_shell_event` ✓(SSE 泵在用)
- 动态装载:`libloading` + `init_rust_ffi`(lib.rs:404)已有装载先例 ✓
- 需新做:**插件 ABI 约定 + `auto run` 的装载编排 + pac.at 外部后端配置**

安全边界:装载的后端是本地构建产物(同 HTTP 形态的信任模型),不做任意
下载装载;ABI 版本号放注册入口参数,不匹配即拒载报错。

## 4. pac.at 外部后端配置(语法建议)

```ini
# ash-gui-auto/pac.at
render: "vm"
back: { project: "../ash-server" }   # 相对路径,指向后端 Auto 项目
```

规则:
- `back.project` 指向后端项目根(含其 pac.at + api.at + shell.at 桩);
- **链接式引用**(实施定稿):resolve_module_path 的 EXTERNAL_BACK_ROOT
  钩子把 `back.*` 直接映射到后端根(零复制),前端无需本地 back/;
- 后端 api.at 自带全部契约类型定义,不得依赖前端模块;
- merged 运行期宿主按 `back.project` 定位 cdylib(后端项目 target 产物,
  缺失时给出"先构建后端"的明确报错);
- 省略 `back` 时维持现状(本地 back/ 目录),兼容既有项目;
- 后端项目自身可 `auto run --http` 独立起服(HTTP 形态入口)。

## 5. HTTP 形态(不变)

ash-server bin(axum + SSE)原样保留;前端 `AUTO_BACKEND=http://…` 走
Plan 057 fetch 分派。外部后端配置只影响"默认后端在哪",不碰 HTTP 协议。

## 6. 迁移路径(三步,详见 plan 061)

1. **ash-server Auto 化**:迁入 api.at、加 pac.at 与 cdylib target、
   register_bridges → `auto_backend_register` 导出(代码大半现成);
2. **引擎侧**(auto-lang,**worktree 实施**):pac.at `back.project` 配置 +
   前端契约读取 + `auto run` cdylib 装载编排 + HostVtable;
3. **前端切换**:ash-gui-auto back/ 清空、pac.at 指外部、ash-runner 退役、
   run_vm.ps1 改回 `auto run -r vm`。

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| cdylib 与宿主(auto)版本错配(host_bridge ABI 漂移) | 注册入口带 ABI 版本号;后端 cdylib 由本机构建(同 target 树),天然同版 |
| 事件回流线程安全(插件线程 → VM/渲染线程) | 沿用 inject_shell_event 既有通道与活联门控;不新造机制 |
| 契约双源(迁移期前端 back/ 与后端 api.at 并存) | 迁移 PR 内一次性切换;前端编译报错兜底(grep 契约签名) |
| auto-lang master 并发占用(Plan 419 等) | **引擎改动一律 worktree 实施 + junction 构建**,合并窗口另择(本次 060 R16 已验证该工作流) |
| a2r 路线重叠 | 本设计**不依赖 a2r**;a2r 修复后生成的 Rust 后端可视为 cdylib 的另一产线,不冲突 |

## 8. 与既有计划的关系

- **plan 060**:本设计是其 §1"换后端 = 换 shell 模块绑定"的通用化终态;
  M3 手写 ash-runner 明确标记为过渡形态;
- **Plan 057**:HTTP 分派机制原样复用;
- **auto-lang Plan 419(VM 生命周期/RC)**:并行不悖;其 RC 金丝雀当前在
  ash-gui 触发的确定性 UAF(060 §十六轮补记)修复前,merged 验证需稳定
  引擎基线(worktree 机制同时解决此问题)。
