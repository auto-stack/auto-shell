# 065 — 测试 flake 根因排查 + HTTP 层验证 + 手写资产同步

- 日期:2026-08-25
- 状态:**已完成**(测试侧修复全绿;引擎侧根因钉死后移交;HTTP API 层验证通过、
  视觉层受阻待补)
- 工作方式:worktree `.worktrees/plan-065`(分支 plan-065);运行依赖 gen/ 等
  git 忽略资产,经 junction 借用主仓;pytest 以 `AUTO_BIN` 指向 auto-lang 的
  auto.exe。

## 背景

064 收尾时遗留六项:① VM 轮间/轮内连接 flake;② RefreshContext 一命令 4 次
触发的来源;③ HTTP/vue 形态零验证;④ gen/ 手写资产流程债;⑤ auto-lang 提交
归属混杂(仅记录);⑥ 小项(ST-03 弱断言、ASH_DEBUG_SMART 打点、chips 残留
语义)。本计划处理 ①②③④⑥。

## 排查结论(全部实测钉死)

### ② RefreshContext 4 次触发 —— 根因:命令被重复执行,非事件重复投递

- 干净路径实测:单次 submit → `suggest hook fired` ×1、`read_ai_next` ×1。
- SN-01 实测:`suggest hook fired` ×4(连续突发)→ **worker 执行了 4 次**。
- 链条:MCP `autoui_action submit` 在**请求时**从 `shared.view`(滞后快照)的
  Input/Textarea value 抓文本,嵌入 ActionMessage 排队;`_submit_command` 的
  0.4s 盲重发在首个 submit 未处理完时,每条重发各带完整命令 → 4×RunCommand →
  4×command_result → 4×RefreshContext(首拉有数据后续空 = 063 SN 空拉清 chips
  的放大源;重复执行块 = ST-03「残留块」的实体)。
- 测试侧修复(`_submit_command`):submit 后耐心轮询清空(0.2s 步进,默认 3s,
  `ASH_SUBMIT_PATIENCE` 可调),超时才重发 —— 丢消息仍可恢复,滞后不再放大。
  验证:SN 轮 hook:ai_next = 1:1(原 4:4)。
- 引擎侧根治(移交):submit 应在**投递时**读当前值(空则 no-op),见 DEBTS。

### ① VM flake —— 两类已修/钉死,一类移交

- **跨会话起不来(skip)**:9247 固定口的 bind 竞争(孤儿占口 / TIME_WAIT
  堆积)+ bind 失败静默 return(mcp_server.rs 只 eprintln,VM 无头续跑)。
  修复:conftest 每会话选 **ephemeral 空闲口**(AUTOUI_MCP_PORT 显式指定时
  沿用固定口),共享资源竞争整类消失。
- **会话中途断连(ConnectionError 连环)**:死亡现场已捕获 —— VM 日志末行
  `------------- end --------------` + teardown 打印 `code=0` → **非崩溃**,
  iced 窗口被干净关闭(`run()` 返回 Ok("UI closed"),auto main.rs:972)。
  与 renderer.rs 2026-08-22 注释的「心跳周期 view 重建在大 Code 块下静默
  退出」同族(iced 0.14.0);主仓与本 worktree 同样复现,与本次改动无关;
  同机多轮起停后恶化、单轮稳定。根因(谁关的窗)需引擎侧在 window close
  路径加打点 —— 移交。
- **可观测性**:conftest 在 startup 超时与 teardown 两处打印 VM 存活/退出码
  (自退类死亡此后免费拿到信号)。心跳保持开启(测试依赖它驱动 bounds
  收集,AUTOUI_MCP_DISABLE=1 会让 vtree/动作全部失联 —— 已实测)。

### ⑥ 小项

- **ST-03 恢复严格断言**:清场根治(`_clear_suggestion_cards`:点 ✕ 取消后
  **等到消失**,ST-02 原先点完不等,ST-03 的 multi-a 等待被残留卡瞬时满足)
  + 按钮出现本身作为等待条件(翻译卡头部含 multi-a/c 文本先渲染,▶/▶▶ 按钮
  要等 RefreshContext 拉 steps 后的下一次 view 重建 —— 头部先行窗口竞态,
  ST-02/ST-03 同修)+ rfind 定位执行块头断言 a→b→c 派发序。
- **ASH_DEBUG_SMART 打点保留**:本次 4× 触发定位完全依赖它,与
  ASH_DEBUG_JOBS 同惯例(门控 eprintln,默认静默)。

### ③ HTTP 形态(ash-server.exe + vite)

- 起法:ash-server.exe :3000(固定)+ `gen/front/vue` 下
  `AUTO_FRONT_PORT=3001 AUTO_HTTP_PROXY=http://127.0.0.1:3000 npm run dev`
  (vite 默认口与后端同占 3000,必须错开)。
- **API 层全验**(diag 驱动,SSE 订阅 + run_command + 槽位拉取):
  - SSE 流式:ping 3s → 10 帧 `command_output` chunk;echo → 终态
    `command_result`(Success/exit_code/output.Text/duration/cwd 全字段)。
  - `script <abs>.ash` 经 run_command:smart 通道收尾,Text 全量 ✓。
  - `ai_next`:双层 JSON 字符串契约(串内 JSON 数组)✓;`ai_steps`:
    `\n` 连接多步串 ✓;`boot_script`:完整 `script <路径>` 命令串 ✓。
- **视觉层未验**:vue 侧代码路径在位(store 模块加载即拉 boot_script 并
  RunCommand,useShellStoreStore.ts:88),但本环境浏览器工具(IAB webview)
  持续 "guest not attached" 不可用,chips 行/分步卡/Init 直提的视觉验证
  待有浏览器时按 web-gui-tester 流程补(DEBTS 已记)。

### ④ gen/ 手写资产

- 新增 `ash-gui/restore-vue-assets.py` + 权威副本 `ash-gui/vue-handwritten/`
  (lib/api.ts、lib/utils.ts、components/ui/** 全树,入库跟踪):
  - 默认 push(gen ← 权威,`auto gen` 后恢复);`--pull`(gen → 权威,手改
    后固化);`--check`(漂移即 exit 1,CI 守卫)。
  - 实测:删空后 push 恢复、check 报漂移/通过均正确。
- codegen 根治(api.ts 端点产出、vue.rs 里写死的旧 demo API_FUNCTIONS 名单)
  移交引擎侧(DEBTS 已记)。

## 验收

- test_ai_parity 双轮(suggest ON 7 passed / 默认 6 passed)×2 全绿;严格顺序
  断言在列。
- test_command_exec 2 passed;test_boot_script 默认轮 BS-03 passed(bs01/02 按
  064 惯例需 boot 轮环境,当时已验)。
- 修复前对照:主仓同组合电池当日两跑各死一次(中途 exit 0 连环
  ConnectionError)—— 与本计划改动无关的原存 flake,死亡证据链已固化。

## 移交清单(详见 DEBTS「Plan 065 新增」)

1. submit 请求时取值 → 投递时取值(引擎,mcp_server.rs)。
2. 会话中途窗口干净关闭根因(引擎,window close 打点;疑似与 2026-08-22
   心跳注释同族)。
3. MCP bind 失败静默 return(引擎,SO_REUSEADDR/重试/信号)。
4. renderer.rs shell_event_subscription 不可达重复匹配臂(清理)。
5. codegen api.ts 产出 + API_FUNCTIONS 名单(引擎)。
