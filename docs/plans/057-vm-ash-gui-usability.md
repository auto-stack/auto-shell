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

## 2. 关键诊断发现(供后续修复)—— ✅ 2026-08-16 三项已全部解决

1. ~~**VM handler 内 `.input` 字段读求值成 Int 0**~~ **(已解决,复查为连环污染)**
   复查 emit_api_http_call(codegen.rs:4280):body 参数走与普通位置**完全相同**
   的 `compile_expr`,不存在独立的参数位编译缺陷。原 Int 0 观察是在 §2.2 中止
   bug 修复前采集的连环污染(.history 读失败 → legacy 重试污染调用帧)。
   fake-backend 契约测试复核:`complete(.input, .input.len())` 发出
   `{"line":"ec","cursor":2}`,参数位字段读正常。prompt_bar.at 已改回直用
   `.input`(原 v 参数规避保留给 ghost 匹配,语义等价)。
2. ~~**handler 内读数组 prop(`.history`)静默中止整个 handler**~~ **(已解决,四层修复)**
   根因链:① `Value::Array` 无法 nanobox 进 VM 栈(读回 double 垃圾)→
   vm_bridge ensure_child_state 把数组 prop 转**堆 ListData**(存 Int id);
   ② `Unknown.len` 无 native → stdlib 补 `("List","len")`(接受 i32/对象 tag
   的堆 id,downcast ListData<i32>/<Value>/<String>);③ `app.at` 的
   `history: .store.history` 是 **computed** → VM `.store.X` 展平只读裸字段,
   FieldNotFound → prop 恒 unresolved → 改传 `.store.persisted_history`;
   ④ dynamic.rs 吞错(Err→legacy→Err=>{})→ 补 ASH_DEBUG_VM_LOG=1 门控日志。
   **ghost text 已恢复双端**(截图验证:"ec"+灰显"ho http_mode_test")。
3. ~~**表格块视图渲染缺最后一步**~~ **(已解决,self 根解析)**
   根因:parser dot_item 把 `.a.b.c` 建成 `Dot(Dot(Ident("self"),a),b)...`,
   根是 `Ident("self")` 而非字段名;resolve_expr_to_value 的 Ident 臂把它当
   名叫 "self" 的字段查 → None → 整条深链解析为空 → 文本节点被"视觉空"过滤。
   单层 `.field` 因 Dot 臂特判侥幸存活。修复:Ident("self")/`.` 解析为 state
   对象本身(aura_view_builder.rs:3576),深链自然续走。表格 4 列对齐+表头+
   dir/file 着色截图验证通过。
4. **sibling handler 调用不再 exit 101**(RET 下溢保护生效),f663b6e 的
   规避注释已过时(代码中已更新)。
5. **Str.substr 字节边界 panic(回归中发现,已修)**:session 级测试实例偶发
   `end byte index 1 is not a char boundary; it is inside '⎇'`(git_label 的
   多字节首字符)→ 整个 app 进程死亡 → 后续测试连环 ConnectionError。
   .at 的 tokenize/ghost 扫描用 `substr(i,i+1)` 逐字节走,多字节字符处
   索引落在字符中间。修复:shim_str_substr 对 start/end 做字符边界钳制
   (ASCII 行为不变),并留 ASH_DEBUG_SUBSTR=1 门控诊断。触发调用方为
   偶发路径(全套件 0 次复现),钳制后无进程死亡风险。

## 3. 明确不做(维持原计划)

merged 内嵌 ash-core;a2r;VM 嵌套调用帧根因;交互式命令(PTY);store 类型化。

## 4. 剩余工作(下轮)

1. ~~表格块渲染收尾~~(§2.3 已解决)、~~ghost text(VM)~~(§2.2 已解决)。
2. **2.6 剪贴板**:native fn `os.set_clipboard`(arboard 已在依赖树,参照
   __preview_copy)+ block_item 复制按钮接线(navigator.clipboard 在 VM 无效)。
3. **Phase 3**:merged 模式 cd 特判回写 cwd(merged_exec_loop 已有 show/ls
   特判先例)。
4. **Phase 4**:conftest http_backend fixture(起 ash-server :3011 + 注入
   AUTO_BACKEND)+ 解锁 BACK-01..08/BB-02..13/PB-05/06/08/TS-02/04 skip;
   vue codegen 产物重生成(auto gen)适配 .at 改动;vue-tsc 验证。
5. gen/front/vue 重生成 + `auto build` 全量验证(.at 多处改动后 vue 产物未更新)。
6. run_vm.ps1 健壮性:优先用已构建的 target/debug/auto.exe(现 cargo
   Start-Process 偶发不存活,脚本已回退直启 ash-server.exe)。
