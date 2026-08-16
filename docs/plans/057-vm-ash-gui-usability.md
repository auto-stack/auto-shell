# 057 — 提高 VM 版 ash-gui 可用性：真后端(HTTP) + 显示对齐

- 日期:2026-08-16
- 状态:**进行中**
- 上游:plan-056(其 §2 阻塞 A 已修,本文接棒其 §3 剩余项)
- 跨仓库:auto-lang(`D:\autostack\auto-lang`,renderer/VM 桥)+ auto-shell(.at/测试/脚本)

## 0. 问题定性

VM 版"粗糙"两层根因:

1. **执行语义空心**:renderer 侧 `std::process` 直执行(merged_exec_loop),绕过
   ash-core;补全/历史/git/命令表全是 `back/shell.at` 静态 mock;`AUTO_BACKEND`
   只切命令提交通道,`#[api]` 调用仍链接 mock。
2. **显示结构性缺口**:VM 路径 Textarea 丢弃样式(renderer.rs render_dynamic_view
   的 `style: _`);Row 分支无 absolute 脱流叠层(Column 有)→ ghost overlay 断;
   表格行样式是动态拼接 inline-CSS,`resolve_expr_to_string_with` 不处理 Binary
   拼接 → 求值为空串;六个容器 convert 动态样式只读静态串。

## 1. 主路线

**把 `AUTO_BACKEND=http://127.0.0.1:3000`(VM + ash-server)升级为一等模式**:
命令执行、补全、历史、git、jobs 全走真 ash-core 会话,仓库保持解耦。
显示层补齐四个 iced renderer 原语。a2r 继续搁置。

## 2. Phase 清单

- **Phase 0 基线**(✅ 2026-08-16):smoke/全量测试跑通(54 pass + 43 skip;
  2 个顺序依赖 flake 非回归);MCP 工具数断言 12→13;plan-056 状态纠偏。
  另修测试竞态:`autoui_find` 的 live VTree 晚于首帧就绪,
  `_find_prompt_input_vnode` 加 15s 重试(test_command_exec.py)。
- **Phase 1 真后端**(auto-lang 为主):
  - 1.1 AUTO_BACKEND 非空时 `#[api]` 编译为 HTTP(URL 从 AUTO_BACKEND 取,
    复用 --no-merge 的 emit_api_http_call);备选:native 层注册 shell.* 覆盖
  - 1.2 renderer HTTP 模式改纯 SSE 泵(去 poster 防**双提交**;Cancel 拦截让行)
  - 1.3 http_sse_loop 转发 job_started/job_done/job_list(预置字段+无参 handler)
  - 1.4 真实 exit_code(替换 status 推导)
  - 1.5 command_result.cwd 回写 store + 新增 `.RefreshContext()` 刷 git
  - 1.6 run_vm.ps1/.sh 一键启动(ash-server + AUTO_BACKEND + auto run -r vm)
- **Phase 2 显示对齐**:
  - 2.1 Textarea 样式移植到 VM 路径(抄 into_iced:1593-1629)
  - 2.2 Row 分支 absolute hoist(仿 Column:8560-8610)+ inset-0 全零偏移
  - 2.3 overlay mono 字体类解析(font-mono-ash/whitespace-pre)
  - 2.4 表格列对齐(Binary 拼接求值 + grid-cols-[N] 解析 + .at 改 Tailwind 类)
  - 2.5 六容器 convert 动态样式绑定感知(~12 处)
  - 2.6 native fn `os.set_clipboard`(arboard)+ 复制按钮接线
- **Phase 3 merged 快赢**:cd 特判回写 cwd;真实 exit code
- **Phase 4 测试与文档**:http_backend fixture(ash-server :3011 + AUTO_BACKEND
  注入);解锁 BACK-01..08/BB-02..13/PB-05/06/08/TS-02/04;README/TODO/DEBTS 更新

## 3. 明确不做

merged 内嵌 ash-core(仓库耦合);a2r;VM 嵌套 sibling 调用帧根因(guard 已保命);
交互式命令(PTY,独立大项);store 类型化重构。

## 4. 风险

- emit_api_http_call body 手工拼接类型擦除 → 逐端点契约测试,失败转 native 备选
- ghost overlay 字体度量错位 → fallback 预案:View::Textarea 加 spans 字段(不实施)
- blocks 双写架构不动:job 事件走预置字段+无参 handler(与 vue 产物同构)
- auto-lang 工作区有另一会话未提交 toast 改动(2895-3081 行)——不触碰该区域,
  提交时注意分离
