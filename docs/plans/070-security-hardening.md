# 070 — 安全加固 P0:ash-server 网络面 + AI 执行链审批收尾 + sandbox 失效修复

- 日期:2026-08-26
- 状态:**✅ 已实施,验收通过**(M1-M4 全落;回归结论见 §7)
- 实施记录(2026-08-26 深夜):
  - **M1**:新 `guard.rs`(GuardConfig + axum 中间件:Bearer token 常数时间
    比较 / Origin 白名单 / 回环 Host 检查,`ASH_SERVER_TOKEN`/`ASH_SERVER_ORIGIN`/
    `ASH_SERVER_BIND` 三 env);bin 默认绑 `127.0.0.1:3000`,非回环无 token
    拒启动;新 `sysopen.rs`(canonicalize+exists → argv 直 spawn:目录
    explorer/文件 rundll32 FileProtocolHandler,macOS open/Linux xdg-open),
    http.rs 与 backend.rs 两处 open_path 同改;删从未用到的 tower-http 死依赖
    (原 http.rs:40 "+ CORS" 注释是假的,防护由 guard 承担)。
  - **M2**:`load_secured(policy, proposals)` 新入口(repl.rs/block_tui.rs 两
    CLI 入口接线);repl 审批卡 [Enter]执行/[e]替代/其他取消(Ctrl-C/D 必消)、
    block_tui `prompt_approval` 黄色块 + 同语义;`AshCommandShellThread::
    start_with_policy` + `ask::run(args, policy)` + main.rs plugin mutating
    子命令在 read-only/no-exec/dry-run 下拒绝;EvalAutoTool 代码级 DANGER_
    PATTERNS 检查(内嵌 system("rm -rf /") 同拦)+ 工具描述与 ask 提示词
    措辞改安全版;**last_denial 机制**(shell.rs 三处拒绝点记痕,
    execute_for_agent 转 Err、eval_auto 附加 `[security]` 行 —— 修掉
    "AI 被拒却看到执行成功空输出"的透明性缺口)。
  - **M3**:`active()` 补 `sandbox_dir.is_some()` + 回归测试。
  - **M4**:`json_args_to_cli` 严格化(控制字符 token 加引号中和、`$`/反引号
    拒绝并提示走提案;裸串含元字符拒绝),ProposeTool 走 lenient 版(建议卡
    人类全文审阅,原文展示)。
  - **顺手适配(非计划内,解锁编译)**:auto-ai 上游 PLAN-027 重构后
    `ToolOutput` 移除(execute 直返 String)、`StreamEvent::TurnStart/TurnEnd`
    删除、`CompletionResponse.model_meta` 删除 —— ash_command_tool.rs /
    ai/ask.rs / repl.rs / block_tui.rs / worker.rs 六处跟进。
  - **测试**:ash-server 0→15(guard 10 单测 + 5 路由级 oneshot:401/403/
    rebind Host 403/open_path 注入 400/回环放行);auto-shell 新增 9
    (M4 中和 5 + policy 线程 1 + eval_auto 拒绝 1 + sandbox active 1 +
    last_denial 链路);ash-core sandbox 1。
- 来源:[工程审核报告 REVIEW-2026-08-26](../../REVIEW-2026-08-26.md) §二(S-1..S-7,全部经人工复核原码证实)
- 范围:仅 P0 安全止血。P1/P2 结构/性能/文档项已登记 `TODO.md`(审核整改节),
  逐个另立计划;本计划不碰。

## 0. 威胁模型(定级依据)

产品定位是「AI Agent 安全执行层」,按此标准定级。射程内:
- **T1 局域网攻击者**:ash-server 绑 `0.0.0.0:3000` 无鉴权,`/api/run_command`
  即 web RCE(ash-server.rs:18-21)。
- **T2 恶意网页**:SSE/全部端点无 Origin/Host 校验 → DNS rebinding 下浏览器
  可读 `/api/history` 并代发命令;http.rs:40 注释自称 "+ CORS" 但实现为零。
- **T3 失控的模型输出**(幻觉/提示注入):CLI chat 工具全直exec、eval_auto
  无任何 danger 检查、`system()` 可绕过工具层。
- **T4 误配的用户**:单 `--sandbox` 静默失效;`ash plugin`/`ash ask` 子命令
  在 set_policy 之前 early return(policy 豁免)。

明确不在射程:同用户恶意本地进程(读 env/内存,超出威胁模型)、插件供应链
签名(生态决策,P3 另立)、TLS 远程访问模式。

## 1. M1 — ash-server 网络加固(S-1 + S-2)

方案已与用户定案(2026-08-26 对话):**回环 + 启动令牌 + Origin/Host 校验**
三层;明文 token 的防嗅探由回环保证(流量不出网卡),不引入 TLS。

- **绑定**:默认改 `127.0.0.1:3000`;`ASH_SERVER_BIND` 可覆盖,但目标为非
  回环地址且未配 token 时**拒绝启动**并提示。
- **token**:server 读 env `ASH_SERVER_TOKEN`(随机 32B hex,由统一启动方
  生成后同时注入两端);middleware 常数时间比较 `Authorization: Bearer`,
  缺失/错误 → 401。**token 未配置时回环仍放行无鉴权** —— 兼容现有
  `cargo run` 裸跑工作流;一旦配置则强制全部 14 路由(含 SSE `/api/stream`)。
- **Origin/Host**:Origin 在白名单(env `ASH_SERVER_ORIGIN`,默认
  `http://localhost:5173` 与 `http://127.0.0.1:5173`)之外 → 403;无 Origin
  的非浏览器客户端(curl/vite proxy)仅回环放行;Host 非 localhost/127.0.0.1
  → 拒(DNS rebinding 特征)。
- **open_path 注入修复(S-2)**:去掉 `cmd /C start "" <path>` 字符串拼接
  (http.rs:184-195、backend.rs:205-219 两处),改直接 spawn 参数数组:
  Windows `explorer.exe <path>`、macOS `open`、Linux `xdg-open`;执行前
  canonicalize + exists 校验,路径不存在直接 4xx。
- **前端接线**:vite 工程在 `ash-gui-auto/gen/front/vue/`(codegen 产物,
  **勿手改**)—— token 注入改在 `.at` 源头的 vite/proxy 模板(读
  `ASH_SERVER_TOKEN` env 注入请求头),或统一 dev 启动脚本层完成。
- **测试(ash-server 0→1)**:强制态无 token 401;伪 Origin 403;
  `{"path":"x & calc"}` 被拒不 spawn;非回环 bind 无 token 拒启动。

## 2. M2 — AI 执行链:CLI 审批门收尾 + policy 透传(S-3 + S-4 + S-5)

068 §2 的既定设计(CLI proposal 渲染为 F3 既有审批卡)在 069 只完成了入口
收敛(F3 退役并入 chat),**审批门没有落到 CLI** —— repl.rs:400 与
block_tui.rs:693 仍是 `ChatSession::load()`(None=全直exec,worker.rs:1266
注释自证)。本节即 068 Phase 2 的真正收尾:

- **CLI proposal 门(S-3)**:两个入口改 `ChatSession::with_client_and_proposals`,
  sink 侧渲染复用 069 保留的审批卡形态([Enter]执行/[e]编辑/[Esc]取消);
  多提案 v1 单活跃(与 GUI 侧一致)。
- **policy 透传(S-5)**:agent 工具用的 `Shell::new()`(ash_command_tool.rs:82、
  worker.rs:291)改为 clone 交互会话的 SecurityPolicy —— `--read-only`/
  `--sandbox` 下 AI 线程同样受限,堵住"用户带只读标志、AI 写盘畅通"。
- **eval_auto 纳管(S-4)**:EvalAutoTool 对代码加 danger 检查(DANGER_PATTERNS
  同表);提取代码中的 `system("...")` 调用串一并过表。工具描述
  (ash_command_tool.rs:322-358)与 ask 系统提示词(ai/ask.rs:39-49)中
  "Call system(...)" 的教学措辞改为引导走提案工具。
- **子命令豁免修复**:main.rs:71/:77 的 `plugin`/`ask` 在 early return 前
  应用 policy —— `parse_security_flags` 的结果(:63 已在手)补 `set_policy`
  传递即可。

## 3. M3 — `--sandbox` 单独使用静默失效(S-6)

`SecurityPolicy::active()`(security.rs:85-93)未计入 sandbox_dir,而
main.rs:283 仅 `if policy.active()` 才 set_policy → `ash --sandbox box` 进
REPL 后沙箱整体丢弃(-c/-s/script 路径无条件 set,不受影响)。修复:
active() 补 `self.sandbox_dir.is_some()`,加回归测试(单 --sandbox 启动后
写外部路径被拒)。

## 4. M4 — 参数拼接元字符校验(S-7)

`json_args_to_cli`(ash_command_tool.rs:277-292)对 token 含
`; | & > < $ \`` 与反引号时不再裸拼 —— 拒绝该工具调用并返回错误,提示模型
改走提案工具。杜绝 `{"args":["hi",";","touch","pwn"]}` 拼成
`echo hi ; touch pwn` 借只读白名单直exec。

## 5. 验收

- 新增测试全绿;ash workspace `cargo test --workspace` 不回归。
- 手工冒烟清单:
  - `curl -X POST localhost:3000/api/run_command`(配 token 态、无/错 token)→ 401;
    浏览器 Origin 白名单外 → 403;`open_path` 传 `x & calc` → 4xx 不弹 calc。
  - CLI `?` 对话诱导 AI 执行 `touch /tmp/x` → 出审批卡而非直接执行。
  - `ash --sandbox box` 进 REPL,写 box 外路径被拒(修复前放行)。
  - `ash --read-only ask "帮我删掉这个文件"` → AI 线程写盘被 policy 拒。
- DEBTS 记账:S-8(read-only/no-network 名单定位=提醒而非边界)、S-9(插件
  capabilities 未强制、启动即 source)转入 DEBTS.md;THREAT-MODEL 一页纸
  随 P1 文档计划(071 候选)出。

## 6. 明确不做(射程外)

- **TLS / 远程访问模式**(`--listen 0.0.0.0 --tls` 显式开关):无真实需求前
  不付复杂度;届时自签证书 + 指纹比对 + 强制 token。
- **命名管道 / PID 级对端甄别**:单用户开发者工具定位下,回环+token+Origin
  白名单等效,且对浏览器架构零侵入。
- **read-only/no-network 名单语义改造(S-8)**、**plugin 签名/域名白名单
  (S-9)**:生态决策,单独立项。
- **fallback 引号注入(REVIEW I-1)、GBK 编码(I-2)、子进程超时(I-3)**:
  P1 实现项,见 TODO 审核整改节,不混入本计划。

## 7. 回归结论(2026-08-26)

- **ash-core**:408+1 全绿(含 M3 新测试)。
- **ash-server**:15/15 全绿(0→1)。
- **ash workspace**:ash-tui 153 / auto-shell lib 700 过 2 挂 / parity 2 过 /
  examples 3+1;**5 个失败全部预存或环境项,与 070 改动零交集,已入 DEBTS**:
  - `test_auto_expression_execution`:069 时已在册豁免(auto-lang 430/432)。
  - `test_ls_tilde_lists_home`:ls 比较器非全序(依赖本机 home 内容触发,
    确定性失败,070 未触碰 ls/排序)。
  - `examples_smoke` cron-list/svc-status、`examples_parity` positional_arg:
    auto-lang VM 在途回归(engine.rs:1492 panic / for 循环字符串拼接损坏,
    手工复现在案)。
- **手工冒烟(验收清单)**:
  - 活体 server:无 token 401 / 错 token 401 / 对 token 200 / 伪 Origin 403;
    `ASH_SERVER_BIND=0.0.0.0` 无 token 拒启动;默认绑 127.0.0.1。
  - `open_path {"path":"x & calc"}` → 400,不 spawn。
  - `--sandbox <dir> -c "echo x > 越界路径"` → 拒绝 exit=1(文件未创建)。
  - `echo hi ";" touch pwn` → 打出字面分号、不建文件(引号中和对真实
    解析器生效)。
  - CLI `?` 审批卡 / `--read-only ask` 拒写:需 AI key 的交互路径未做活体
    验证,由单测覆盖(policy 线程/EvalAuto 拒绝/审批通道组件级全绿)。
- **对照说明**:stash 基线对照不成立 —— 干净 HEAD 因 auto-ai API 漂移编译
  不过(070 顺手适配的六处即此),069 归档时同样以"引擎预存项在册豁免"
  口径收口。
