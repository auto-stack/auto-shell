# 060 — api.at 契约归一:merged 模式进程内后端(退役 renderer 桥)

- 日期:2026-08-22
- 状态:**M1+M2 完成**(契约归一 ✓ 语义下沉 ✓ 12 条命令 MCP 全量验证通过;
  M3 留接口不做)。实施记录与设计偏差见 §6
- 上游:Plan 057(VM+HTTP 一等模式,SSE 泵)、Plan 059(表格增强,ls 拦截的
 下游债务)、2026-08-22 的 cd/pwd 会话拦截与 `ls | where` 过滤(过渡实现)
- 跨仓库:auto-shell(worktree `plan-060`)+ auto-lang(worktree `auto-shell`)

## 0. 需求与设计原则(用户确认)

**HTTP 与 merged 两种模式的唯一差异应是 `#[api]` 调用的传输层(fetch vs
进程内直调);语义层必须同源。** ash-core 是所有 shell 形态的后端核心;
api.at 是唯一契约层(其 codegen 已规划 TS client / Axum routes / Tauri
commands 三种物化)。

现状违反点(本项目要消除的):
- merged 模式 `shell.at` 的 `run_command` 是 **no-op**,真执行走 renderer 桥
  (store 写 `__pending_command_{id,str}` 信号 → renderer 读信号提交
  `merged_exec_loop`)——**完全绕过 api.at 契约**。
- shell 语义长在 renderer(auto-lang)里:`show`/`ls`/`cd`/`pwd` 拦截、
  `ls | where` 过滤、`parse_output_to_structured`、`handle_ls_command`/
  `handle_show_command`、`table_to_csv/tsv`。每个真语义缺口都在 renderer
  打补丁,债务持续累积。

## 1. 目标架构

```
HTTP:  前端 → api.at fn → codegen fetch → ash-server(ash-core)
         → SSE /api/stream → http_sse_loop → __sse_* 预置 → store .RunOutput/.RunResult
Merged:前端 → api.at fn → shell.at(进程内)
         → shell_exec_submit native → 执行线程(spawn + 流式,零语义)
         → 事件队列 → relay → __sse_* 预置 → 同一组 store handlers
```

要点:
- **两种传输汇合于同一组 store handlers**(`.RunOutput`/`.RunResult` 已存在,
  模式无关,零改动)——这是"只差传输层"的结构性保证。
- renderer 侧只剩**哑传输**:进程 spawn + stdout 流式转发 + 退出码。无任何
  shell 语义。
- shell 语义全部落 .at:ls 表格构建、cd 会话 cwd、where 过滤、输出结构化。

### 技术路线裁定(2026-08-22 调研结论)

- VM 为**协作式单线程调度器**(SPAWN 绿色任务);`process.spawn_with_output`
  阻塞 → .at 内直接执行长命令会冻结 UI。**执行必须留在 OS 线程**,.at 只做
  结果语义处理。
- `event.dispatch` 为 Vue-only(window.dispatchEvent 物化),VM 无状态回流
  通道 → 复用 `http_sse_loop` 已验证的回流路径(`__sse_*` 预置 +
  `on_with_input_for`),不发明新机制。

## 2. 任务拆解

### M1 契约归一(核心)

- **T1**(auto-lang):新增 native `shell_exec_submit(block_id, cmd, cwd)`
  ——压入全局执行队列;`merged_exec_loop` 队列源切换为该队列。白名单三处
  登记(shim 表 / for_each_bigvm_native / NATIVE_ID_ENTRIES),新 ID,不复用
  既有号(2843 撞号教训)。
- **T2**(auto-shell):`shell.at` `run_command` 实装——调
  `shell_exec_submit`;HTTP 模式不受影响(codegen fetch 已替换 api 调用,
  native 不会被触达)。
- **T3**(auto-lang):删除 renderer 的 `__pending_command` 桥(两处建块 +
  提交逻辑),store 的信号字段保留(前端自有簿记)但 renderer 不再读。
- 验收:merged 模式 echo / ls / 长命令(sleep)流式正常,取消正常;
  renderer 中 grep 不到 `__pending_command`。

### M2 语义下沉(退役拦截)

- **T4**(auto-shell):store `.RunResult` 增加命令语义分发(或经 shell.at
  辅助函数):
  - `cd <path>` → fs natives 解析(~/相对/绝对,canonical 语义以
    `fs.canonical` native 近似)+ 更新 `.cwd`;非法路径 → Failed 块;
  - `pwd` → 输出会话 cwd(Text);
  - `ls [path] [| where col op val]` → `shell.at build_ls_table`:
    `fs.read_dir` + `File.is_dir` 构建 Table(列 name/type/size,目录优先
    排序,隐藏文件过滤,`-a`),`where` 过滤以 .at 重写(从 renderer
    handle_ls_command 迁移;read_dir 返回的 JSON 数组串用 substr 扫引号
    对解析——JsonValue 方法链在 handler 内静默中止的既知规避)。
- **T5**(auto-lang):删除 `merged_exec_loop` 的 show/ls/dir/cd/pwd 拦截、
  `handle_ls_command`/`handle_show_command`、`parse_output_to_structured`
  (stdout 直通 Text)。执行线程只保留:cmd /C | sh -c + 逐块流式 + 取消
  flag + 退出码。
- 验收:M2 后行为与迁移前一致(MCP:`ls` 表格、`cd ..` 标题栏、
  `ls | where type == file`、`pwd`、`show` 降级路径);renderer 中 grep
  不到 `handle_ls_command`。

### M3 真后端绑定(可选,本期不做)

- a2r 生成二进制或薄 runner crate 实现 api.at 契约直连 ash-core,
  `use shell` 指向切换,前端零改动。前置:a2r codegen 修复(TODO 已记录)。
- 本期只保证接口形状:M1/M2 完成后,换后端 = 换 shell 模块绑定。

## 3. 风险与对策

| 风险 | 对策 |
|---|---|
| native 队列与 renderer 静态句柄的模块边界(native.rs 触不到 renderer static) | 队列放中立模块(如 ui/mod 或 vm 侧全局),双方引用 |
| shell.at 模块无法写 store 状态 | 不写——语义在 store handler 内执行,shell.at 只提供纯函数 |
| where/ls 迁移后 .at 表达能力不足(字符串/循环坑) | cd Tab 补全已验证所需原语(read_dir/is_dir/substr 扫描)均可用 |
| HTTP 模式回归 | submit native 仅 merged 路径触达;HTTP 冒烟单独跑 |

## 4. 验证计划

- MCP(auto ui):echo、ls、`ls -a`、`ls tmp`、`ls | where type == file`、
  `cd ..` + 标题栏跟随、`pwd`、长命令 `sleep 3` 流式、运行中取消、
  cd Tab 补全(回归 Plan 060 前功能)。
- 回归:HTTP 模式冒烟(ash-server 起服,run_vm.ps1 -NoServer 变体)。
- 像素级不需要(无 UI 变化)。

## 5. 遗留(预期)

- ~~M3:a2r/runner 绑定 ash-core~~:已落地(ash-runner + host 桥,见 §M3 落地完成)。
- `run_smart`/`jobs`/`kill_job` 仍为 mock(无真实需求,契约保留)。
- `show` 命令:迁移后为外部命令直通(`cmd /C show` 会失败)——按 ash 语义
  在 T4 一并 .at 化(读文件 + Code 变体)或显式记录降级。

## 6. 实施记录(2026-08-22,M1+M2 落地)

### 交付形态(与 §1 的偏差:修复侧 → 提交侧)

原设计把语义修复放在 store `.RunResult`(结果侧幂等修复)。实施中发现
**merged 模式下 command_result 由 renderer 事件泵直写块字段**
(update_block_in_state),store handler 根本不被调用 —— VM 的 handler
无法写 renderer 管理的 blocks 数组(Value::Array ↔ VM 堆不同步,B 系列
引擎债)。故改为**提交侧分派**:

```
shell.at run_command(block_id, cmd, cwd):
  ls/dir → ls_result_json(fs 直读 + where 过滤,直接产出 payload JSON)
           → auto.shell.emit_result(2868) 直发,不 spawn
  cd     → resolve_cd(段栈归一 + is_dir 校验)→ emit_result(带新 cwd)
  pwd    → emit_result(Text = 会话 cwd)
  其他   → auto.shell.exec_submit(2867) → 执行线程 cmd /C | sh -c
```

传输层(merged_exec_loop)只剩两条路:Result 变体直发 / 进程执行流式。
**契约归一达成**:两种模式都经 api.at(HTTP: fetch;merged: shell.at),
汇合于同一事件泵与 store handlers。

### 关键改动

- auto-lang:natives `auto.shell.exec_submit`(2867)/`auto.shell.emit_result`
  (2868);`vm::shell_bridge` 队列(中立模块,native 与 renderer 共用);
  删除 `__pending_command` 桥、全部拦截(show/ls/dir/cd/pwd)、
  `handle_ls_command`/`handle_show_command`/`ls_file_kind`/
  `parse_output_to_structured`;`PendingShellCommand`/`handle.queue` 退役。
- auto-shell:api.at `run_command` 增加 cwd 参数(HTTP 端 ash-server 忽略
  body 附加字段);shell.at 实装 run_command 分派 + ls/cd/pwd 语义 +
  JSON 拼装(json_escape/result_json_with/fail_result_json)。

### 实施中发现的 VM 缺陷(规避 + 待引擎侧根治)

1. **`tag` 是 .at 关键字**(TokenKind::Tag):`.tag` 属性访问解析失败
   ——TaggedCell 的 tag 字段在 .at 不可赋值(渲染只读 kind,留默认)。
2. **模块函数返回 struct 静默中止**:`var out RenderedOutput = build_ls_output(...)`
   使整个调用方 handler 事务性回滚(连 blocks.push 都丢)。标量(str)
   返回正常 —— ls 走"直接产出 JSON 字符串"管线绕开。
3. `List<X]`/`List[X>]` 方括号泛型笔误不报解析错,但令
   `collect_module_imports` 整体失败(Undefined variable: List),
   下游 api 函数全部失联 —— 排查成本高,值得加 lint。

### 验证(MCP,2026-08-22)

| 命令 | 结果 |
|---|---|
| echo done | Success,Text ✓ |
| ls | Table 10 行(9 文件 + tmp 目录)✓ |
| ls \| where type == file / dir / size > 10000 | 9 / 1 / 4 行,过滤精确 ✓ |
| ls tmp | Table 1 行(子目录列举)✓ |
| pwd(cd 前/后) | 会话 cwd,cd 后跟随 ✓ |
| cd .. / cd front / cd zzz | Success / Success / Failed(no such directory)✓ |
| ping -n 2 | 进程路径流式输出 ✓ |
| 标题栏 cwd | cd 往返跟随(段栈归一后为正斜杠形态)✓ |

### show 卡死追查与修复(2026-08-22,第三轮)

用户复报"`show types.at` 没有显示结果"。经复现与逐层排查,发现**三个叠加
的根因**(前一轮的 .at 高亮移植 `hl_*` 四函数正是导火索,已整体回撤):

1. **VM 字符串池 u16 截断(引擎缺陷,已规避)**:字符串池只增不减,
   natives 侧索引 `as u16` 截断(上限 65535)。.at 侧 `line = line + ch`
   式逐字符拼装 + `js = js + ...` 拼 payload,一次 show(types.at ~30KB、
   block_item.at ~100KB)就往池里塞 1~2 万条;池溢出后索引回绕,既有字符
   串互相串写 —— 实测铁证:会话 cwd 被写成单字符 `r`(state dump 可见),
   payload 丢失 → 块永挂 Running,进程满转后静默 exit(1)。
   - 修复 A(show 下沉 Rust):新增 native `auto.shell.emit_show`(2869),
     读文件 + 逐行高亮 + payload JSON 全在 Rust 侧完成
     (vm/shell_bridge.rs `highlight_rgb`/`show_result_json`;renderer 的
     font-mono 自动高亮改为委托同一实现,消除双份色板)。shell.at 只做
     路径归一,`hl_*`/旧 `show_result_json` 巨串管线整体删除。
   - 修复 B(池去重):engine.rs `add_string` 按内容去重(重复内容复用
     索引),`load_strings` 换池时同步重建去重表。运行时高重复 churn
     (样式串/标签)不再撑池。
   - 引擎债:索引 u32 化 / 池 GC 未做 —— 真去重后仍有唯一串增长路径
     (如 ls 前缀拼接),超长会话仍可能触顶。
2. **MCP 心跳强制重建风暴**:MCP UI 服务默认开启(:9247),心跳
   200ms 一拍,每拍触发一次完整 update→view 重建。基线视图 ~80ms/次
   (空闲 ~40% CPU);大 Code 块渲染 >200ms 时消息队列常态积压 → 事件
   饿死(命令提交丢失、CPU 满转)。修复:心跳放宽到 2s(快照新鲜度对
   工具足够;空闲 CPU 降至 ~7%)。
3. **Code 渲染成本**:逐行逐 span 建 text widget(660 行 ≈ 3000 节点/
   重建)过于昂贵。修复:apply 结果时由 Rust 把 Code 全文写入
   `block.streamed_text`(复用 Block 已有 str 字段的 renderer↔.at VM-only
   契约),block_item.at 用**单个 font-mono text** 渲染整块 —— Rust 侧
   highlight_code 一次扫描,重建成本 O(1) 节点。

验证(MCP):`show types.at` ×3 连续 Success,span 色板齐全(注释灰
107,114,128 / 标点青 137,221,255 / 关键词紫 199,146,234 / 白
229,231,235);echo/pwd/`show 不存在`(Failed+消息)全绿;截图确认
彩色渲染与块宽正常;空闲 CPU ~7%。

**遗留(新债,引擎侧专项)**:部分场景进程仍会**静默退出**(exit 1,
无 panic 输出、无 WER、未达 run_dynamic_iced 出口;`show block_item.at`
大文件后偶发)。疑点:MCP 状态同步线程与 VM 线程的数据竞争,或 VM 深层
内存问题 —— 需 ASAN/线程sanitizer 级工具专项排查。另:快速连打
(type 后 <80ms submit)时 oninput debounce 重放旧 input,renderer 桥判
"input 非空"丢弃命令(不建块)—— 序列号守卫未覆盖 input 重放,记 UI 债。

### 第四轮:块内滚动丢失 + 高亮存疑(2026-08-22)

用户复报:代码结果无高亮、块内无滚动(应 max-h + 内部滚动条)。

- **滚动(真 bug,已修)**:代码块 scrollable 实测 3058px(内容高),
  max-h-[400px] 完全未生效。根因在 iced 适配:`from_style` 对
  `w-full + overflow-y` 推断 `height=Fill`(竖向填充意图),而 max-h
  封顶逻辑(Plan 057 续)写在 `is.height=None` 分支 → 永不触发。
  修复(auto-lang `build_scrollable`):cap 判定与 height 推断解耦 ——
  有 cap 一律 Shrink + `Container::max_height` 封顶(CSS max-height
  语义:短内容收缩、长内容滚动),显式 h-*/Fill 让位。验证:代码块
  scrollable @rect 高 3058→400,内滚出现。
- **高亮(未回滚,一直生效)**:VM 的代码着色走"font-mono Text →
  renderer 自动高亮"(Plan 409 §10 续 20,即"之前的实现"),像素级
  验证代码区确有 prism-tomorrow 色板(关键词紫 #cc99cd / 注释灰 #999 /
  运算符青 #67cdcc / 标点 #ccc;字符串绿/数字橙在 types.at 可视区无
  此类 token 故为 0)。与 Plan 411 增强分词器已合流为单一来源
  (`vm::shell_bridge::highlight_rgb`),show payload 与 font-mono 渲染
  同色板 —— vue/vm 一致由共享 .at + 共享高亮函数保证。
- **用户侧二进制陈旧(操作注意)**:`run_vm.ps1` 用
  `auto-lang\target\debug\auto.exe`(master target)。本轮排查发现该
  二进制停在 14:15(早于当日全部修复),旧二进制 + 新 .at 直接启动
  失败(Undefined symbol: shell.emit_show)。已重编 master target。
  **auto-lang 合并后必须重编 master target,否则 run_vm.ps1 起旧件。**

### 第五轮:结果底栏补全 + 文本边界 + 静默退出定位(2026-08-22)

用户提两需求:show 块缺结果底栏(复制 icon);cat/show 结果裸贴无边界,
建议上下横线围合 + 下边界右下放结果工具栏。

- **底栏缺 Code 变体(根因)**:底栏条件与 CopyOutput(.at handler +
  renderer arboard 桥)都只支持 Text/Table。补齐:条件加 Code;
  CopyOutput 两侧都加 Code → 复制全文(streamed_text;桥侧空串防御)。
  验证:点击 show 块 copy icon → 剪贴板 3593 字符 types.at 全文。
- **结果边界**:Text 与 Code 变体上下各一条 `h-px w-full bg-border` 行
  (iced 无 border-t/border-b 单边类,1px 背景行是可靠等价物),底栏
  row(justify-end)贴下边界 —— 构成"文本框"视觉围合。Text 变体同时
  补上 max-h-[400px] + 块内滚动(此前只有 Code 有,cat 长输出会无限撑高)。
- **静默退出定位(重大进展)**:大 Code 块在场时进程 ~10s 内消失
  (无 panic/无 WER/未达 run_dynamic_iced 出口)。对照实验:
  AUTOUI_MCP_DISABLE=1 关心跳 → 30s+ 存活;开启 → ~10s 死。
  **触发器 = MCP 心跳引发的周期性 view 重建循环**(默认开启,:9247,
  普通运行也在跳)。修复:心跳改活联门控 —— SharedState 记最近 HTTP
  请求(note_activity),仅 30s 内有 MCP 请求(agent 活跃)才心跳;
  普通运行(无 agent)不再周期性重建。验证:show 后空闲 75s 存活
  (修复前同场景 ~10s 死)。重建路径的底层内存根因未除(agent 活跃期
  间长会话仍可能触发),仍为引擎债;AUTOUI_MCP_DISABLE=1 留作诊断开关。

### 第六轮:ls 表格无色(2026-08-22)

用户报 ls 表格无彩色。排查:渲染层与 is_dir/ends_with 原语均正常
(`ls ..` 的 back/front 正确拿 Dir/sky);根因是 **merged shim 的文件分类
与 ash-core 不一致** —— ash-core `file_name_kind`(renderer.rs:306)把
`.at`/`.rs` 都归 CodeAtRs(emerald),shim 只写了 `.rs`,漏了 `.at`;
默认 cwd(src/front)全是 .at 文件 → 整列 Plain 无色。叠加因素:此前
front 里唯一有色的 tmp 目录(sky)在清理时被删,观感上"全无色"。
修复:shell.at 分类严格镜像 ash-core(`.at`/`.rs`→CodeAtRs、
`.exe`/`.dll`→Executable、`.toml`/`.json`/`.yaml`/`.yml`→Config),
shim 自创的 `.bat`/`.cmd`/`.ps1`/`.lock` 规则一并删除(HTTP 模式本就
不认)。验证:ls(front)→ CodeAtRs×9(.at 文件名绿)。

### M3 落地完成(2026-08-22 终)

用户指令:merged 不再单独做逻辑;所有后端逻辑在 ash-server(API)内调
ash-core(命令逻辑),参照 015-notes 双模式。**已全链路跑通并合并。**

**架构(最终形态)**:
- merged 模式入口 = `ash-runner`(ash-server 新 bin):进程内起 worker
  (auto_shell::Shell → ash-core)+ 10 端点宿主桥 + ShellEvent→
  inject_shell_event 事件泵 + auto-man run_vm_ui 起 GUI。
- 调用形态:auto-lang codegen host 分派 —— 裸 `#[api]` 调用改写为
  `auto.host.call(裸名, args_json)` + `json.to_value`(与 HTTP 改写
  同构,值在调用点落栈,规避 .at fn return 无法携带复合值的 VM 边界)。
- shell.at = 空桩(仅保签名供编译解析);`auto run -r vm` 旧入口退役
  (无 runner 时 merged 不可用),run_vm.ps1/sh 改走 ash-runner。

**调试过程中定位并修复的深层问题**:
1. api_funcs 元数据收集被 `api_over_http` 门住 → 扩为 `|| has_host_calls()`。
2. **宿主桥 native 表派发错位**:native_interface 表内 2870 绑的不是
   shim_host_call(has_shim=true 但调用无效果;直调 shim 一切正常)。
   修复:引擎 CALL_NAT 对 2870/2871 直调(仿 112/2300 特例先例);
   根因(native 注册秩序)记引擎债,待统一后回收特例。
3. 构建环境:sysinfo 0.33(其 windows≤0.57)曾令 gpu-allocator 与
   wgpu-hal 的 windows 0.58 类型错位 → 升 0.35;32MB 栈链接参数补齐;
   跨仓 path 经 .worktrees/{auto-lang,auto-ai} junction(worktree 构建)。
4. 方法链 `x.len().str()` 为已知 VM 限制(拆两步写法即可),非本线问题。

**验证(MCP + 像素)**:boot 真数据(79 命令侧栏、真 cwd=进程目录、
真历史 438 条);pwd/ls/echo 流式;`ls | where type == file` 真管道
(Tagged: CodeAtRs+Plain);cd 会话跟随;`show src/front/types.at`
Success + Code 变体;渲染:代码高亮(紫/灰/青像素)、400px 块内滚动、
ls 文件名 emerald。show 巨串/高亮管线随 shell.at 逻辑退役自然消失。



用户指令:merged 不再单独做逻辑;所有后端逻辑在 ash-server(API)内调
ash-core(命令逻辑)。参照 015-notes 双模式实现。

**已落地(auto-lang,机制件,已合并主检出)**:
- `vm/host_bridge.rs`:名字→函数注册表;native `auto.host.call`(2870,
  JSON 串)/`auto.host.call_value`(2871,直推 VM 值);`host` 加入
  codegen stdlib 模块名表;`ui::iced::renderer` 提 pub +
  `inject_shell_event`(SHELL_EVENT_TX 全局)。
- codegen host 分派:裸 `#[api]` 调用改写为 `auto.host.call(bare, args)`
  + `json.to_value`(与 HTTP 改写同构;spike 证明 .at fn return 无法
  携带复合值,故走调用点发射)。`ApiCallInfo` 增 `fn_name`;api_funcs
  收集门从 `api_over_http` 扩为 `|| host_bridge::has_host_calls()`。
- ash-runner(auto-shell 仓 ash-server 新 bin):进程内 worker + 10 端点
  桥 + ShellEvent→inject_shell_event 事件泵 + auto-man run_vm_ui 起 GUI。
  依赖修复:sysinfo 0.33→0.35(其 windows≤0.57 约束曾令 gpu-allocator
  与 wgpu-hal 的 windows 0.58 类型错位);.cargo/config 补 32MB 栈。
  跨仓 path 经 `.worktrees/auto-lang`/`auto-ai` junction。

**当前阻塞(精确记录,续作入口)**:改写在编译期触发(31 处),但
**运行时复合值落地失败**:
- host 模式:Init 静默中止(store 回滚,commands=0)。
- 对照实验:同一 spike(tests/spike-m3)走 **HTTP 模式同样坏** ——
  `history()` 返回列表,`h.len().str()` 得 "438-2147483647"(垃圾);
  即 **VM 前端调 #[api] 拿复合值这条路径在 ash-gui 从未真正验证过**
  (HTTP 模式历史冒烟只看了事件流,没看 Init 数据落地)。
- 下一步:修 VM 侧 HTTP/host 共享管线的复合值落地(json.to_value 的
  堆对象 → var/字段赋值的槽位/类型编码;对比 015-notes notes_store
  `.notes = list_notes()` 可行的差异点 —— 疑与带类型 var 声明或
  store vs widget 模块的 handler codegen 路径有关)。诊断探针
  (DBG-HOST/DBG-HOSTCALL/DBG-BRIDGE/DBG-PUMP)已留在代码中,修完删除。

### show 迁移补记(2026-08-22,后续提交)

`show` 真实现属 **ash-core**(reader + Prism tomorrow 高亮)。merged 侧
在 shell.at 做等义 shim(同 ls 的"直接产出 JSON"管线):

- **提交侧分派**:`show` → 路径归一(.at)→ `auto.shell.emit_show`
  native(Rust 侧读文件+高亮+payload)直发,不 spawn。
- **语法高亮**:Rust 侧 `vm/shell_bridge.rs::highlight_rgb`(自 renderer
  的 highlight_code 移植为中立实现,后与 Plan 411 增强版分词器合流;
  色板 prism-tomorrow:关键词紫 #cc99cd / 字符串绿 #7ec699 / 注释灰
  #999 / 数字·布尔·函数橙 #f08d49 / 运算符青 #67cdcc / 标点 #ccc);
  renderer font-mono 自动高亮与 emit_show payload 共用同一份。
- **Code 内联渲染**:BlockBody 子 widget prop 字段读取为空(B 系列
  已知),Code 分支内联进 block_item.at;回退条件加 Code。渲染用单个
  font-mono text(streamed_text 全文契约,见上节根因 3)。
- **ResultBlock w-full**:Code 渲染 col 漏 `w-full` 会收敛到内容宽、
  把块挤窄、滚动条脱离面板最右 —— 补上后代码块横向占满面板。

### 遗留更新

- ~~`show` builtin 未迁移~~:已迁移(见上),含 Prism tomorrow 高亮;
  但属 merged shim,HTTP 模式走 ash-server 侧 ash-core 真实现 ——
  M3 a2r/runner 绑定 ash-core 时 merged 侧可退役 shim。
- ~~Vue api.ts 本地桩~~:不再需要(api.at 无 plain 转发 fn,最终设计
  未引入)。
- run_smart/jobs 仍为 mock(无真实需求)。

### 第七轮:侧栏 commands"乱"(2026-08-22,双根因)

现象:用户侧栏条目文字叠压、内容漂移("前面也出现过一次")。像素行带
分析(用户截图 vs 实况)定位两个独立问题:

1. **字符串池 u16 回绕(auto-lang,真凶)**:运行期池索引在 natives
   消费侧 40+ 处 `as u16` 截断,而字段读(`push_value` Value::Str 臂)
   每次直推新池条目无去重 —— 侧栏 81 命令 × name/description 随每次
   视图重建(每敲一键)膨胀,会话活跃数分钟即越过 65535,索引回绕
   互串:文本错乱 → 换行数漂移 → 行高错位 → 叠压;且随重建漂移。
   与"show 巨串堆损坏"(第一轮)同病根(池无界增长 + u16 上限),
   该次只把 payload 构建下沉 Rust 绕开,未治本。
   **修复(auto-lang e8672fdd)**:`get_string` u16→u32 + 清除全部
   runtime 截断点;重新驻留热点(push_value/STR_CAT/json_to_vm_value/
   btreemap/stream-chunk/convert 等)改走 `add_string` 内容去重;
   `tests_string_pool` 锁死两条不变量(池 >65535 往返精确 / 重复内容
   零增长)。codegen/template_codegen 为编译期自洽 u16 池,未动
   (app 常量表规模 ~数千,远离上限;彻底 u32 化留引擎债)。
2. **truncate 未实现(观感)**:Vue 版侧栏描述靠 `truncate`(单行
   省略);VM 侧该类此前根本没进 StyleClass。长描述(如 ash 的
   "Execute a .ash script in the current shell session (alias for
   source)")换行 2-3 行,条目高低不平。
   **修复(auto-lang cbfc1c76)**:StyleClass::Truncate + iced_adapter
   `wrap_none` + renderer Text 臂 `Wrapping::None` + clip 容器。

验证:MCP 实例像素行带严格 [名1行→描述1行] 交替零重叠;压力(5 条
命令 + 多次快照)后数据层 81 钮(`.`/build/cat 序)与像素层均稳定。
注:注册表真实清单 81 条,首条即 `.`(DotCommand,POSIX source 惯例),
字母序 —— "alias/bash 缺失"系视觉模型从"(alias for source)"描述
文本脑补,数据从未缺失。

### 第八轮:native catalog 注册秩序根因收官(2026-08-22)

M3 引擎直调特例(native_id==2870/2871 走 shim 直调)的根因查明并修复:
**catalog ID 撞号** —— host.call 复用了 Random 段(2870-2874)已占的号,
bind_shims 后注册覆盖先注册,native_interface 表派发错位。修复:host.call/
call_value → 2930/2931(空闲段),**直调特例删除**,恢复正常表派发。

全面清点(catalog 完整性测试首次运行即抓获)另除三处同类地雷:
- **ID 129 撞号**:hashmap.drop ↔ hashset.new,后者覆盖前者 ——
  hashmap.drop 实际派发到 hashset_new(潜伏栈语义错乱);hashset.new
  → 1293。
- **三表错位**:str.len(catalog 170 ↔ bigvm/entries 1500)、str.upper
  (175↔1511)、str.new(172↔177,177 实为 string.new)。此前靠
  resolve_qualified 的 registry 优先序侥幸未爆,任何走 fallback 的
  新解析路径都会错派发;现全部对齐 catalog 权威 ID。

**防回归**:catalog_integrity_tests(catalog ID/名字唯一、bigvm 同名必
同 ID、NATIVE_ID_ENTRIES 交叉一致)—— 撞号/漂移今后测试期即拦截。

E2E:ash-runner 重编重启,host 桥走正常表派发 —— 81 侧栏钮/真 cwd/
pwd 执行/像素零重叠全通过。auto-lang 1a31218e。

### 第九轮:编译期字符串池 u32 化(2026-08-22,引擎债收口)

与运行期池(第七轮)同病的编译侧:codegen `add_string` 返回 u16,应用
常量超 65535 即编译期索引回绕、静默串写。所有承载池索引的字节码操作数
统一 2B→4B:LOAD_STR / CREATE_NODE / PUSH_ACCUM / ACCUM_PAIR / CREATE_OBJ /
GET_FIELD / CALL_SPEC / CLOSURE(名) / CAPTURE_* / LOAD|STORE_GLOBAL /
CREATE_FUTURE(捕获名)。涉及 codegen(含 59 处 `as u16` 强转清零、
PUSH_ACCUM 哨兵 0xFFFFu16→u32 —— 此哨兵曾致 config 测试组炸出)、engine
16 处解码、disasm、abt-disasm/asm(镜像对齐)。

教训:批量脚本中途断言失败会静默丢弃全部未落盘编辑(第一阶段 7 个操作
码的引擎改动丢失,靠 config 测试反查出)。宽度错位在池 <65536 时因
u32 高位补零被当成 0x00 操作码而"侥幸"通过 —— 更隐蔽。

防回归:tests_string_pool 增编译侧宽度锁(LOAD_STR 5 字节编码、
push.accum 9 字节流对齐)。遗留:add_string 线性去重 O(n²)(链接器
并池语义理清后换 HashMap)、abt-asm CALL_SPEC 形态债。

E2E:ash-runner 重编重启,新字节码格式全链路 81 钮/真 cwd/无 fallback。
auto-lang 8482021e / merge 2193834a。

### 第十轮:方法链标量派发修复 + 复合值返回债务销案(2026-08-22)

**修复:列表方法链 `l.len().str()` 静默 None** —— CALL_SPEC 把正数 i32
接收者一律按堆对象 id 解析(heap 无 3 号对象 → `<unknown:3>.str` →
派发失败静默推 None;ash-gui 侧栏计数曾因此串成垃圾串)。字符串接收者
因 codegen 类型推断走 TYPE_TO_STR 幸免,列表/未知类型方法结果退化
CALL_SPEC 即踩坑。修复:CALL_SPEC 入口加标量接收者(i32<4M/f32/f64/
bool)的 str/to_string 兜底,格式化与 TYPE_TO_STR 同款;TYPE_TO_STR
四处直推入池顺带改 add_string 去重。

**销案:M3 登记的 ".at fn return 无法携带复合值(退化 0)"** —— 当前
master 四种形态全不复现:脚本 fn 返回 struct / 模块 pub fn(use 接收)/
原生调用桩体 return(json.to_value)/ 带注解 var 接收。tests_known_limits
七项回归锁死(方法链三形态 + 复合值四形态),此项移出债务清单。

E2E:ash-runner 重编重启,81 钮/真 cwd/无 fallback。auto-lang f6147362。
剩余引擎债:add_string O(n²) 去重、abt-asm CALL_SPEC 形态、tag 关键字。

### 第十一轮:三项引擎债清偿(2026-08-22)

**①add_string O(n²) → HashMap**(codegen 池内容索引;不变量:池只追加,
直推站点仅致无害重复)+ 链接器并池去重 O(n×m) position → HashMap。

**②链接器 remap u32 化 + 共享跨度模型**:与 417-followup 会话同日撞车
(对方独立修复了 walker 操作数宽度、表留 u16 登记为债;本侧为超集:
remap 表/obj_remap 全 u32 + 共享 operand_span_with_pool_indices 双
walker 单表,变长操作数 CLOSURE/CREATE_FUTURE/ACCUM 系统一建模),
合并取超集并吸收对方 0xFFFF 哨兵守卫。

**③abt-asm/disasm CALL_SPEC 与 engine 统一**(u32 method + u8 argc)。

**④tag 软关键字上下文化**:语句位 `tag` 仅后随 Ident(枚举名)才走
enum 别名;`tag: value`/`tag.field`/`tag(...)` 按标识符。连带修参数名
(soft_ident)、struct 字面量 key()、双点访问解析器。计划 060§154
(TaggedCell.tag 不可访问)与 405/401(tag 不可作参数名)三笔登记项
一并销案。tests_tag 四项 + enum 别名守卫锁死。

E2E:ash-runner 重编重启,81 钮/真 cwd/无 fallback。
auto-lang 96add3f3 / merge db8a4600。剩余引擎债:无在册高危项;
观察项:巨串池 GC(池只增不减,量级可控)。

### 第十二轮勘误:字符串池无 GC 定性修正 — 是实现缺口,非设计取舍

第十一轮将"池只增不减"定性为设计取舍、未登记为债。经核对代码,定性
错误,应为**设计意图存在、实现未落地**:

- `DROP = 0x05` 操作码注释明言 "RAII cleanup: pops and frees owned
  value" —— 原设计就是要 RAII;
- 引擎实现为 `pop_i32()` 空壳(弹栈槽,不释放任何池/堆资源);
- codegen 全文零发射 `OpCode::DROP`(作用域尾无任何清理指令);
- `remove_heap_object` 唯一调用方是 hashmap/hashset/stringbuilder 的
  显式 `.drop()` native —— 运行时实际走手动清理路线。

背景:Auto 语言层模仿 Rust(var/let/mut/move/take + 编译期 borrow
check),但双后端分叉 —— a2r 转译路径由真 Rust 的 Drop 兑现生命周期;
VM 解释器路径所有权为编译期词汇、运行时擦除(裸索引自由拷贝,无唯一
属主),确定性析构无从挂起。

**登记为引擎债:VM 运行时确定性析构未实现**。可行路线(按侵入度):
①codegen 作用域尾发射 DROP + 池条目/堆对象引用计数(真 RAII,插桩
成本高);②标记-清除 GC(复用 operand_span_with_pool_indices 重映射
机器做压缩);③维持现状 + 长会话定期重建 VM 实例(ash-gui 重启即释
放)。现状影响:仅内存单调增长,正确性风险已被 u32 化 + 去重消除。

### 第十三轮调研:VM 三层生命周期设计现状核对(2026-08-22)

Auto 的内存设计(用户口径):无 Rust 级生命周期标注,三层兜底 ——
①周期内引用(~80%)作用域尾清理;②一级逃逸分析覆盖大部分逃逸;
③歧义项升级 Shared 引用(Rc<Cell<T>> 式,自带引用计数自动清理)。

**调研结论:VM 后端三层均未接线**(a2r 路径借 Rust 编译器意外全成立):

- ①RET 尾缘仅截断栈,无清理钩子;DROP 空壳+零发射(第十二轮);
  parser scope 栈仅符号表;无 defer。
- ②全仓无逃逸分析。旁注:CLOSURE 捕获为拷贝语义,arena 下自洽。
- ③Rc/RefCell/Cell/Mutex 仅活在 a2r 透传;VM 零 refcount;OnceCell
  natives 走堆对象无计数。
- 借用检查仅 check_unsafe_capture 一个窄点;.view/.mut/.move/.take
  四操作符在 codegen 直通编译内层(注释明言 MVP TODO)。

**设计已声明、未接线**:take/move/copy 参数模式与所有权操作符词汇
齐全。补线要点:.at 值的逃逸出口仅三个(闭包捕获/函数返回复合值/
存入容器)——②③层分析面即此三点;①层为工程主体(作用域尾发射
DROP + 池/堆条目引用计数;heap 可直接 remove_heap_object,字符串池
需压缩重映射 —— operand_span_with_pool_indices 机器已就绪)。
立项待定;本轮仅登记调研结论。
