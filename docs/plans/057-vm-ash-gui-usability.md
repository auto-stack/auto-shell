# 057 — 提高 VM 版 ash-gui 可用性:真后端(HTTP) + 显示对齐

- 日期:2026-08-16
- 状态:**Phase 0/1 完成,Phase 2 大部分完成,Phase 3/4 未做**(见 §4 剩余)
- 上游:plan-056(其 §2 阻塞 A 已修,本文接棒其 §3 剩余项)
- 跨仓库提交:auto-lang `e0decbadf`/`5856ae069`/`8be979338`/`f085a2334`,
  auto-shell `f93a169`/`d9f4586`/`bf55654`/`27342f4`

## 0. 问题定性

VM 版"粗糙"两层根因:

1. **执行语义空心**:renderer 侧 `std::process` 直执行(merged_exec_loop),绕过
   ash-core;补全/历史/git/命令表是 `back/shell.at` 静态 mock;`AUTO_BACKEND`
   只切命令提交通道,`#[api]` 调用仍链接 mock。
2. **显示结构性缺口**:VM 路径 Textarea 丢弃样式;Row 分支无 absolute 脱流
   叠层;深层点路径文本绑定无法解析;容器动态样式只读静态串。

## 1. 已完成(全部实测验证)

### Phase 1 — HTTP 一等模式(核心,✅)

- **`#[api]` over HTTP**:AUTO_BACKEND 非空 → api_over_http 启用(lib.rs),
  emit_api_http_call 的 base URL 取 AUTO_BACKEND(codegen.rs 原硬编码)。
  Init 的 command_list/history/prompt_context/jobs、PromptBar 的 complete、
  store 的 run_command/cancel/open_path/kill_job 全部直连 ash-server。
- **renderer 纯 SSE 泵**:http_sse_loop 去 poster(防双提交);转发
  job_started/job_done;Cancel/RunCommand 拦截在 HTTP 模式让行。
- **job 事件 Rust 直管 job_list**(镜像 blocks 模式,绕 VM handler 字段读)。
- **真实 exit_code**:优先用后端 CommandResult.exit_code。
- **cwd 回写 + RefreshContext**:command_result 后写 store.cwd(剥 `\\?\`
  前缀)+ 触发 .RefreshContext() 刷 git 标签。
- **ash-server 侧修复**(Vue 版同样受益):流式路径收集子进程真实退出码
  (ExternalStream::exit_status_handle),非零码报 Failed("exit code N")——
  修复未知命令被误报 Success/0;worker 2s Tick 驱动 job reaper(job_done
  实时,不再等下一条用户请求)。
- **run_vm.ps1(UTF-8 BOM)/run_vm.sh 一键启动** + README 重写。
- 验收实测:boot 真数据、echo/ls 结构化输出、补全 body `{"line":"ec",
  "cursor":2}`、badcmd→Failed("exit code 1")、cd 后 cwd 回写、git 标签实时
  (⎇ main +1 ⇡N ⇣N)、jobs 增删闭环、无双重提交。

### Phase 2 — 显示对齐(✅ 除 2.4 表格最后一步)

- **2.1 Textarea 样式移植 VM 路径**(原 `style: _` 整体丢弃):text-transparent
  隐藏 editor 副本、placeholder 弱化灰、字号——透明 textarea+overlay 技术成立。
- **2.2 Row 分支 absolute hoist**(镜像 Column 的 Plan 409 实现):ghost/高亮
  overlay 的 `row{relative} > row{absolute inset-0}` 不再挤开 textarea。
- **2.3 逐键管线整合**:iced text_editor 无 keyup 通道 → .OnInput(vm 死路)
  的 tokenize/续行搬进 OnInputComplete(sibling 调用在 RET 下溢保护后可用)。
  实测:`grep -q x` 分色渲染(grep→#34d399 / -q→#d8b4fe / 空格→#e5e7eb),
  未闭合引号→in_continuation,截图无双重文字。
- **2.4/2.5 基建**:resolve_expr_to_string_with 补 Bina(Add) 拼接;
  extract_style_with 绑定感知样式(六类容器 convert × tracked/untracked
  12 处);class.rs grid-cols-[N];block_body.at 表格行改 Tailwind 类拼接。
- **解析层修复**:extract.rs flatten_dot_path(深层 Dot 链文本内容此前
  UnsupportedExpr 静默丢弃);resolve_interpolation_with 深层点路径嵌套 Dot
  求值。

### Phase 0 — 测试基建

- MCP 工具数断言 12→13;_find_prompt_input_vnode 加 15s 重试(live VTree
  晚于首帧);_submit_command 每次重解析 vnode id(补全候选行改变 vtree
  哈希);conftest 侧栏就绪门(修 APP-07 启动竞态)+ ASH_TEST_VM_LOG。
- 全量:55 pass + 43 skip(基线 54;剩余 1-4 个顺序/负载 flake 与基线同款)。

## 2. 关键诊断发现(供后续修复)

1. **VM handler 内 `.input` 字段读求值成 Int 0**:仅发生在 **API 调用参数
   位置**(emit_api_http_call 的 arg 编译上下文);普通语句位置的字段读正常
   (实验:`.ghost_text="["+.input+"]"` 正确)。规避:参数经 handler 参数 v
   传入(view 侧显式传参,镜像 onenter 模式)。
2. **handler 内读数组 prop(`.history`)静默中止整个 handler**:探针
   `.history.len()` 即触发,后续语句全部不执行(tokenize/spans 连带全空)。
   → ghost text 暂时只留 vue。GET_FIELD 日志佐证:`non-i32 obj_id ... field=history`。
3. **表格块视图渲染缺最后一步**:Table 变体块的 state 数据正确、条件分支
   判断正确(字面量 marker 渲染),但块内**动态文本内容**(含 `.block.command`
   级深链)不出现;echo/Text 块正常。已定位:块 item 文本不走 untracked
   convert_text_element(调试埋点证实 ✓/:0 等不经过),tracked 孪生共享
   helper 但探针仍不渲染——需对 tracked 路径(约 :2985 起)单独埋点排查,
   疑 cached view 或 bindings 上下文差异。**echo 的 Text 输出体是否渲染
   也待确认**('hello_view' 此前与命令按钮 label 混淆)。
4. **sibling handler 调用不再 exit 101**(RET 下溢保护生效),f663b6e 的
   规避注释已过时(代码中已更新)。

## 3. 明确不做(维持原计划)

merged 内嵌 ash-core;a2r;VM 嵌套调用帧根因;交互式命令(PTY);store 类型化。

## 4. 剩余工作(下轮)

1. **表格块渲染收尾**(上节 §2.3):tracked 路径埋点 → 修 bindings/缓存差异;
   2.4 的列对齐基建已就位,修好即自动获得。
2. **ghost text(VM)**:等 auto-lang 修 handler 内数组 prop 读取(§2.2)后,
   把 ghost 逻辑并回 OnInputComplete。
3. **2.6 剪贴板**:native fn `os.set_clipboard`(arboard 已在依赖树,参照
   __preview_copy)+ block_item 复制按钮接线(navigator.clipboard 在 VM 无效)。
4. **Phase 3**:merged 模式 cd 特判回写 cwd(merged_exec_loop 已有 show/ls
   特判先例)。
5. **Phase 4**:conftest http_backend fixture(起 ash-server :3011 + 注入
   AUTO_BACKEND)+ 解锁 BACK-01..08/BB-02..13/PB-05/06/08/TS-02/04 skip;
   vue codegen 产物重生成(auto gen)适配 .at 改动;vue-tsc 验证。
6. gen/front/vue 重生成 + `auto build` 全量验证(.at 多处改动后 vue 产物未更新)。
