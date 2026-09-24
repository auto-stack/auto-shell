# 40 — AI 回合提速 + 工具 schema 兜底 设计文档

> 状态:Plan 083 交付物 · 2026-09-24
> 定位:ash AI 模式回合延迟与工具调用失败的总修复设计,含与 auto-ai
> (PLAN-083 跨仓部分,其仓 main `79ff93a`)的 wire 与行为契约。
> 落地:auto-shell 侧 T-01..T-04(本仓 worktree plan-083-dev),auto-ai 侧
> T-05/T-06(其仓 master/main,commit id 见 §7)。

## 1. 背景与动机

2026-09-24 实测复现(ash REPL AI 模式,glm-5.3-flash 经 aaid daemon)两大
问题:

1. **回合延迟 30s+**:用户提交"统计子目录磁盘占用"后,首个工具调用前
   42.6s 无任何输出。根因链:GLM 经 zhipu anthropic 兼容端点,请求不带
   `thinking_level` 时默认开启深度思考(1670 个 reasoning 事件);REPL/ask
   对 Thinking 事件直接丢弃 → 用户面对"静默 30s"。
2. **du 工具调用失败**:82 个命令工具暴露给模型的参数 schema 全为
   trait 默认空对象,模型无从得知 `du` 有 path/flags,连发三次无参调用
   (含被提示"指定目录"后仍无参),达循环阈值(3)被终止,且错误提示一律
   附 API-key footer,误导为鉴权问题。

## 2. 实测锚点(2026-09-24,本机,真实形态 payload)

真实形态 = Assistant 角色 + 上下文块 + 15 轮历史(31 messages)+ 全量命令
工具(当前注册表 79 个,带 T-01 生成的真 schema)+ du 问题。经 daemon
`POST /v1/chat/completions`(tier:mid → glm-5.3-flash),SSE 流计时:

| thinking_level | 首个 tool_call(3 次) | 中位 | reasoning 事件 |
|---|---|---|---|
| `off`(**ash 新默认**) | 5.88 / 2.20 / 6.22 s | **5.88 s** | 无(off 偶发一例见 §3.1 注) |
| `low` | 10.02 / 10.79 / 11.66 s | 10.79 s | 每次都有 |
| 不传(provider 默认) | 30.5+ s(历史锚点 42.6s) | — | 大量 |

**AC-01 结论:默认 off,中位 5.88s ≤ 10s 目标;low 约 10-12s 不达标。**

AC-02:三次运行首个调用均为 `du` 且 input 非空:
`{"args": ["-d","1","-h","."], "path": "."}` —— 模型从 schema 即可带参,
不再无参试错(修复前:三次 `input:{}`)。

## 3. 契约

### 3.1 thinking_level(wire 与 ash 策略)

- **字段名:daemon HTTP 边界为 `thinking_level`**(`ai-config` wire,
  PLAN-064 引入;值域 `off|low|high|max`,缺省 = provider 默认)。探针
  工具若传 `thinking` 会被**静默忽略**(serde 未知字段丢弃)——本次测量
  踩过,首批"off 仍出 reasoning"数据即此误用。
- agent → daemon 走 `CompletionRequest` 结构体序列化,字段名同上。
- daemon anthropic 映射(`anthropic.at`,zhipu
  `accepts_thinking_param: true` 时):`off` → 体
  `{"thinking":{"type":"disabled"}}`;`low/high/max` →
  `{"type":"enabled","budget_tokens":…}` 且必要时抬 `max_tokens`。
- **ash 默认策略:off**(`auto-shell::ai::thinking_override`)。REPL 聊天
  与 `ash ask` 的 Agent 构造点(build/clear/ask 三处)统一施加;环境变量
  `ASH_AI_THINKING=off|low|high|max|inherit` 覆盖,`inherit` = 跟随角色
  (回到 provider 默认),未设/非法值 = off。
- 注:off 偶发仍见 reasoning 事件(2026-09-24 big 文件任务一例),zhipu
  端点对 `disabled` 的执行不完全可靠;不影响延迟结论(off 中位 5.88s)。

### 3.2 命令工具 schema 生成规则(ash)

`AshCommandTool`/`ProposeTool` 持完整 `Signature`,`parameters()` 由
`Signature.arguments` 生成:

- 位置参数(`!is_flag && !is_option`)→ property `<name>`
  `{type:"string"}`,description 标注序数("1st positional argument");
  `required=true` 者进 schema `required` 数组。
- flag(`is_flag`)→ `{type:"boolean"}`,description 注明 CLI 形态
  `(-s, --summarize)`(无 short 则 `--name`)。
- option(`is_option`)→ `{type:"string"}`,注明 `--name VALUE` 与默认值。
- 根对象 description + `args` property 引导模型用
  **`{"args": [...]}` 顺序数组**形态:既有的 `json_args_to_cli` 对对象形态
  **按值展开、丢键**(flag 名不回传),数组形态才忠实;flag/option 以
  字面 CLI token(`"-s"`,`"--depth"`)入数组。
- `description()` = `Signature.description`(空则回退
  `ash command: <name>`)+ `extra_help`(若有),模型可见用法不劣于
  `--help` 人类输出。
- **已知皱褶(记录,不在 083 处理)**:模型偶发仍选对象形态并给 flag 传
  布尔(`{"human-readable": true, "path": "."}`)——对象按值展开会丢键,
  `true` 落成位置参数,du 报"cannot access 'true'",agent 见错后通常下轮
  改用 args 数组(自纠)。编组规则(`json_args_to_cli`)按计划冻结不动;
  命名形态→CLI flag 的智能映射留待后续计划(如需要)。

### 3.3 循环纠偏两段语义(auto-ai agent,跨仓)

ReAct 循环按 `(tool, args)` 精确匹配计数(阈值 3,不变):

- `count == 3`:**不执行**该调用,注入一次纠偏 `tool_result`(文本点名
  重复调用、原始 args、次数,指令"换参数/换工具/直接基于已有结果作答;
  再犯终止"),并经 `StreamEvent::Warning` 在流上宣告;本回合继续。
- `count > 3`(纠偏后仍完全相同):`AgentError::LoopDetected` 终止(原行为)。
- 有界性:纠偏每 key 一次;总调用预算(max_turns 软目标 ×5 硬上限)不变。
- 实现:`agent.at`(单源)+ `rust-ref/agent.rs`(ash 消费的编译体)+
  a2r 树,三处同步。**顺带修复 a2r 树潜伏缺陷**:原 `bump_seen` 以值传
  Map,a2r 调用点 clone 致计数不回传——转译树循环检测此前从未生效;改为
  append-only `seen_names` + 纯函数计数,三树语义对齐。

### 3.4 错误 footer 分类(ash REPL)

回合错误仅当错误串含连接/初始化特征才附 API-key/daemon footer:
`daemon unavailable`、`ai client init`、`connection refused`、
`api key`、`unauthorized`、`401`。`loop detected`、`max turns`、
`tool error`、upstream quota/refusal、`config error` 打印本体。
(修复前:du 循环终止读起来像鉴权故障。)

### 3.5 daemon 未知 model 路由 400(auto-ai,跨仓)

非 `tier:` 且不在任何 provider 模型表的 model id,daemon
`chat_completions` 路由层直接:

```
HTTP 400 {"error":{"message":"unknown model '<id>' (known: tier:max|pro|mid|lite|min, or a provider model id)"}}
```

不再落到 default_provider 透传(修复前上游报"模型不存在")。
`server.at`/`server.rs` 双写同步。

## 4. 提速交付验证(AC-01/AC-02 复现脚本)

payload 由 `ash/auto-shell/tests/dump_agent_payload.rs` 从真实 ChatSession
构建(一次性请求捕获 mock → `req_ash083.json`),计时探针(3 次取中位):

```python
import json, time, urllib.request
BASE = json.load(open('D:/tmp/req_ash083.json', encoding='utf-8'))
BASE['stream'] = True
URL = 'http://127.0.0.1:17654/v1/chat/completions'
for run in range(3):
    body = json.dumps(BASE, ensure_ascii=False).encode('utf-8')
    r = urllib.request.Request(URL, data=body, headers={'Content-Type': 'application/json'})
    t0 = time.perf_counter()
    with urllib.request.urlopen(r, timeout=180) as resp:
        for raw in resp:
            line = raw.decode('utf-8', 'replace').strip()
            if not line.startswith('data:'):
                continue
            try:
                ev = json.loads(line[5:])
            except Exception:
                continue
            if ev.get('tool_calls'):
                print(f"run{run+1}: {time.perf_counter()-t0:.2f}s "
                      f"{ev['tool_calls'][0]['name']} {ev['tool_calls'][0]['input']}")
                break
```

2026-09-24 实跑:5.88 / 2.20 / 6.22 s,三次 du 调用 input 均含 path+flags。

## 5. 回归

- `ash ask "只需回复两个字:ok"`(默认 off):5.9s,无思考输出(基线 ~5s
  不劣化);`ASH_AI_THINKING=low` 同命令:思考流灰显可见,回合尾折叠
  `· 思考 N 字`。
- REPL 冻结转录:思考行折叠为单行 `· 思考 N 字`(079 精神:思考/工具
  噪音不回放);思考关闭零新增输出。
- ash 单测:auto-shell 728 过/1 挂(`test_auto_expression_execution`,
  auto-lang master 列表字面量 `<obj#N>` 漂移,**预存**,与 083 无关);
  ash 135 过/1 挂(`tail_cmd::spill_writes` millis 唯一性,**预存 flaky**)。
- auto-ai 单测:agent rust-ref 118 过/1 挂(`registry_loads_builtins`
  预存 coder 角色缺失,main 同挂);mvp_harness 25/25;daemon 73/73;
  a2r 树 transpiled_harness 30/30。

## 6. GUI 兼容

GUI(ash-gui)聊天面板事件流兼容:Thinking 事件此前即存在于 StreamEvent,
GUI 未消费则维持原样(增量渲染留后续);CLI REPL 先行。

## 7. 跨仓登记(PLAN-083 §10-Q3)

| 仓 | 分支 | commit | 内容 |
|---|---|---|---|
| auto-ai | main(其仓 master) | `c789841` | T-05 循环纠偏两段 + a2r 计数缺陷修复 |
| auto-ai | main | `79ff93a` | T-06 daemon 未知 model 400 |
| auto-shell | main | `28e8796` | 前置:主检出遗留 WIP 路由落定(build fix/du-top 文档) |
| auto-shell | plan-083-dev | `907dc0b`/`f9f4cdb` | T-01..T-04 |
