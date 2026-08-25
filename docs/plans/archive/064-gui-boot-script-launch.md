# 064 — GUI 外部脚本启动:`ash-gui script.ash` 形态

- 日期:2026-08-25
- 状态:**已完成,验收通过**(直接 main;含 auto-lang 引擎侧一处块匹配修复)
- finish-plan 复审(2026-08-25 晚,归档前):
  - T1/T2/T3 代码逐项核对通过;BS-01/02(boot 轮)+ BS-03(默认轮)复跑
    全绿;063 全族 + 062 test_cli_parity(he02/03 键盘 flake 在册豁免)
    回归通过。
  - **BS-01/02 曾在复审首轮失败,根因是主检出 auto.exe 过期**(065 会话
    中段树态的构建,缺 plan-032 系 VM 修复):worktree 重建当前 master
    后双绿,主检出 cargo build 后亦双绿 —— 非代码缺口,教训是"验收前
    确认引擎二进制与 master 同步"。
  - 下方"已知残留"(SN 族主仓间歇失败)已由 94e3b71(chips 空拉守卫)+
    plan 065 测试侧修复收口,本次复审主仓 SN-01/02/03 全绿,销案。
- 实施记录(2026-08-25):
  - **T1/T2/T3 全部落地**;验收 BS-01(开窗零输入即跑)/BS-02($1 透传)
    boot 轮 2 passed + BS-03(手输 script 命令)默认轮 passed;063 全族 +
    062 基线关键族回归通过。
  - **引擎侧一处修复(计划外,经确认后实施)**:merged 模式下 Init 期
    store 内部提交的命令块是 VM 堆引用(Value::VmRef),renderer 的
    update_block_in_state 只按 Value::Obj 匹配 → 块永远 Running("开窗即跑"
    的唯一堵点;第四条引擎边界)。修法:匹配 miss 时把目标 VmRef 块物化
    为 Obj 并仅替换该槽位(dynamic.rs 加 materialize_obj_value passthrough,
    renderer.rs 匹配循环加物化分支;与 renderer 用户路径对尾块的替换语义
    同构)。首版"全量物化写回"会让 VM 侧后续操作崩溃(ST-02/03 复现),
    收窄为仅目标块后全绿。注意:该修复已随用户 882223fab(git add -A)
    进入 auto-lang master。
  - **连带修复**:backend 事件泵 inject 失败(UI 泵未就绪)改 200ms×25
    有限重试(boot 期事件不再静默丢);worker boot 窗口(15s)内识别
    boot 命令延迟 1.5s 执行(等 UI 稳态,双保险)。
  - **已知残留**:~~063 的 SN 族(suggest-next chips)在本主仓环境间歇
    失败~~(finish-plan 复审销案:94e3b71 chips 空拉守卫 + plan 065 测试侧
    修复后,2026-08-25 晚主仓复跑 SN-01/02/03 全绿)。
- 背景:用户需求 —— `ash` 是 CLI,脚本结果输出到 terminal;ash-gui 需要等价的外部调用
  形态:**带脚本启动 GUI,脚本的过程与结果在界面块流里展示**。
- 调研结论(2026-08-25,实测):
  - 入口不存在:`auto run -r vm` 的 CLI 参数集(部署类)无脚本/exec 通道;pac.at
    无 run/startup 配置;前后端零开机执行钩子(`.ashrc` 只加载定义不执行)。
  - 底器件已齐(063 验证):`execute_script_content` 跑 .ash(AutoLang + `>` 行
    + `$1` 传参)、StreamingOutputHook + 块槽 = 输出流式落块、worker 主循环
    拦截词法模式(`smart` 先例)。
  - 绕开 063 调研发现的两个坑(输入框直跑路径 pre-check 错杀、source 哑输出):
    本计划不经过输入框快路径,worker 拦截后直调脚本执行并激活输出槽。

## 1. 任务分解

### T1 `script <路径> [args…]` 命令(worker 拦截,通用件)

- 主循环 Run 分支拦截(与 `smart` 平级、history 展开之前):
  - 路径解析相对 **shell 会话 cwd**(063 教训:进程 cwd 是 src/front);引号
    词法走 `ext::parse_command`(`smart run` 同款)。
  - 读文件失败 → Failed 块;成功 → `smart_block` 槽 = block_id + `smart_acc`
    清零(`script` 上下文复用 SmartCommand 的输出通道)→
    `set_script_args(rest)`(`$1/$@/$#`)→ `execute_script_content`。
  - 事件收尾带 `Text(smart_acc 全量)`(Empty 清空 streamed_text 的 DEBT,
    063 RunSmart 同款);块标题 = `script <路径> [args]`。
- 语义:这也是 GUI 里手输跑脚本的**正式入口**(补上 063 调研发现的
  "直接路径被 pre-check 拦"缺口的词法化解法,零引擎)。

### T2 boot 启动链(ASH_BOOT_SCRIPT)

- 新端点 `GET /api/boot_script() -> str`:worker 侧静态读 env ——
  `ASH_BOOT_SCRIPT` 非空时返回 `"script <路径>[ args…]"` 完整命令串
  (`ASH_BOOT_ARGS` 空格分词透传),否则空串。http.rs 直读(env 同进程),
  backend.rs 桥同;不占 worker 队列。
- 前端 `store.Init` 末尾:拉 `boot_script()`,非空 → `.RunCommand(boot_cmd)`
  (store 内 sibling 调用,063 RunAiStep→RunCommand 已验证)。merged 模式
  Init 时 cdylib 已装载(worker 就绪);HTTP 模式 Init 在 command_list 轮询
  成功后,同样就绪。
- 启动器包装:run_vm.ps1 加 `-Script <path> [-ScriptArgs "..."]`(设 env 再
  起 VM)。

### T3 验收(tests/test_boot_script.py,fake 不依赖 AI)

- BS-01(boot 轮,`ASH_BOOT_SCRIPT=…`):VM 启动后**无需任何输入**,块自动
  出现,`>` 行输出落块流式/收尾可见。
- BS-02(boot 轮):`-ScriptArgs` 透传 → `$1` 在脚本里可见。
- BS-03(默认轮):手输 `script <路径> <arg>` → 输出落块(通用入口)。
- 基线:063 test_ai_parity + 062 test_cli_parity 关键族不破;vue-tsc 0 错。

## 2. 明确不做(附理由)

- **独立 `ash-gui.exe` 二进制**:需新 crate + auto.exe 路径解析,价值低 ——
  run_vm.ps1 包装即得同体验(`.\run_vm.ps1 -Script xxx.ash`);未来打包期
  (单 exe 发行)再顺势而为。
- **脚本逐行分块**(过程感更强的"每 `>` 行一块"形态):块模型支持,但首版
  单块流水已满足"过程滚动可见";视觉增强留后续。
- **脚本 `exit` 语义映射关窗**:GUI 生命周期不随脚本,收尾即终态。

## 3. 风险与对策

| 风险 | 对策 |
|---|---|
| boot 脚本执行阻塞主循环,期间用户输入排队 | 与普通命令同语义(串行);脚本秒级可接受 |
| smart_block 槽与用户命令输出串台 | 槽只在 script/smart 分支置位,普通命令不碰;主循环串行天然隔离(063 同款) |
| 脚本路径含空格/引号 | parse_command 引号词法;路径含空格用引号包裹(注释说明) |
| env 拼接的命令串被二次解析 | boot_script 只由 worker 构造、前端整体提交,不经用户输入 |

## 4. 验证

- 两轮 pytest(063 SN 双轮同款惯例):默认轮(BS-03 + 全套回归)+
  boot 轮(`ASH_BOOT_SCRIPT=… pytest -k "bs01 or bs02"`,BS-03 skip)。
- cargo build(ash-server cdylib)+ auto gen + vue build 全绿。
