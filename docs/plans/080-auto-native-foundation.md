---
plan_id: PLAN-080
status: drafting
feature_name: Auto 单源化迁移——设计、分期路线图与地基(对齐框架+quote 试点)
author: [zhaopuming]
created_at: 2026-09-07
updated_at: 2026-09-07

# 以下留给 /auto-plan:review 填写
supersedes_spec_components: []
new_spec_components: []
touched_goals: []

current_step: 0
total_steps: 8
---

# [PLAN-080] Auto 单源化迁移:设计 + 路线图 + 地基

## 变更摘要

为"Auto 生态独立"战略立项:**长期**用 Auto(经 a2r 转译回 Rust)单源化替换四个自研
crate(ash-core / auto-shell / ash / ash-server,合计约 60.3k 行手写 Rust),手写版
随迁移逐模块退役。**本计划不做迁移本身**,只交付三件近事:

1. **设计文档** `designs/037-auto-native-rewrite.md`——架构、L0-L5 分期路线图、
   单源化原则("替换而非镜像")、模块迁移准入/退役标准、风险登记;
2. **地基一:三方行为对齐框架** `tests/auto-parity/`——同一用例集在
   ①手写 Rust ②AutoLang VM ③a2r 编译产物 三方运行、统一比对
   `expected.json` 的 runner + 用例库(复用 auto-lang golden 思想,落到本仓);
3. **地基二:试点迁移** `ash-core/src/parser/quote.rs`(361 行 / 30 单测 / 纯逻辑)
   → `.at` 版本进 VM 与 a2r 双通道对齐,并落性能基线;顺带在 DEBTS.md 登记
   a2r 引擎侧前置欠账(本仓视角的外部依赖指针)。

不做:任何模块的正式迁移、ash-server/TUI 层触碰、auto-lang 仓的代码改动
(引擎侧欠账只登记指针,由 auto-lang 仓自己的 plan 体系承接)。

## 目标

1. `designs/037` 被评审通过,路线图与单源化原则成为后续所有迁移计划的引用基点;
2. `tests/auto-parity/` 可一键运行,试点用例集(quote)三方全绿或差异全部在册;
3. quote 试点给出**实测数据**:a2r 产物行为对齐率 + 与手写 Rust 的性能比,写入
   设计文档附录,作为 L1(引擎侧清偿)立项依据;
4. DEBTS.md 新增"Auto 单源化前置(a2r 引擎侧)"条目,含可验证的完成判据。

## 架构方案

### 三方对齐框架(本计划核心交付)

```
tests/auto-parity/
  README.md                 用例格式/三方运行说明
  run.py                    一键 runner(--case/--side rust|vm|a2r|all/--update-golden)
  cases/quote/              试点用例集(由 quote.rs 30 个单测语义转写)
    NNN-<name>.at           用例:读固定输入 → print 规范化输出
    NNN-<name>.cmd.json     输入参数(quote 函数名 + 入参)
    NNN-<name>.expected.json 三方共同比对的行为基准
  a2r-shell/                最小 cargo 壳(编译 auto trans 产物并运行用例)
    Cargo.toml              仅依赖 auto-lang(a2r_std)——零第三方
    src/main.rs             读 case → 调转译产物中的函数 → 打印
  report/                   运行报告(gitignore;结论手抄进 designs/037 附录)
```

三方通道:
- **rust**(基准):`ash-core` 新增 fixture 测试,对每用例调手写 `quote.rs` API,
  产 `expected.json`(基准由此生成,而非人工书写);
- **vm**:`auto run NNN-<name>.at`(auto.exe 来自 auto-lang 主检出);
- **a2r**:`auto trans rust --path X.at` 产物拷入 `a2r-shell/src/gen/`,
  `cargo run --release -- --case NNN` 执行同输出。

比对规则:三方 stdout 逐行全等;差异即红,红项登记进 DEBTS.md 而非绕过。

### 设计文档骨架(designs/037)

- 背景与动机(生态独立;60.3k 行盘点表);
- 单源化原则:**迁移完成即删手写版**;产物不入库、由构建再生 + golden 锁形状
  (对齐本仓 `gen/` 惯例);两份并存只允许出现在"受控 A/B 期"(有 parity 网覆盖);
- 分期路线图 L0-L5(见下);
- 模块准入标准(纯逻辑/零 FFI/零线程/有单测网)与退役标准(三方绿 + 性能达标
  + 下游 crate 编译绿);
- 风险登记:a2r 容器性能(List=RefCell 内可变)、async 锚定 tokio、
  `#[no_mangle]` 导出无发射、双源漂移史(shell.at 教训)。

### 分期路线图(写入 designs/037,本计划只完成 L0)

| 期 | 内容 | 主战场 |
|----|------|--------|
| L0 | 设计 + 对齐框架 + quote 试点 + 基线(本计划) | auto-shell 仓 |
| L1 | a2r 引擎侧前置清偿:容器性能实测与优化、codegen 欠账(含 API_FUNCTIONS 硬编码)、extern/#no_mangle 发射设计 | auto-lang 仓 |
| L2 | ash-core 纯逻辑层逐模块迁移(parser→data→completions;external.rs/external_stream.rs 的 4 处线程泵永留 Rust 壳) | auto-shell 仓 |
| L3 | auto-shell 命令层(80+ 命令逐个;job.rs Windows FFI 永留壳) | auto-shell 仓 |
| L4 | ash-server 逻辑上移(worker/桥;cdylib ABI 壳 5 处 no_mangle 永留手写) | auto-shell 仓 |
| L5 | ash TUI 层(reedline/ratatui 互嵌最深,最后或保持 Rust 壳,由 L0-L4 数据决定) | auto-shell 仓 |

## 技术栈

- AutoLang + a2r(`auto trans rust --path`,auto-lang 主检出 `target/debug/auto.exe`);
- Python 3 标准库(runner,风格对齐 `ash-gui/restore-vue-assets.py`:零第三方依赖);
- Rust/Cargo(a2r-shell 最小壳、ash-core fixture 测试、criterion 基准);
- 既有验证网复用:`examples_smoke` / `examples_parity`(L2+ 的模块级回归网)。

## 需求分析与背景调查

> spec 后端(127.0.0.1:8080 /api/specs/overview)不可用;本节以 2026-09-07
> 会话内的仓库实测摸底为据(数据来源:grep/wc 实测 + 双 Explore 代理全仓引用清查)。

1. **规模与硬特性盘点**(src/ 下 .rs 实测):

| crate | 行数 | unsafe | extern/no_mangle | thread spawn | Rc/RefCell | async | 判断 |
|---|---|---|---|---|---|---|---|
| ash-core | 14,686 | 2 | 1 | 4(external 泵) | 9 | 0 | L2 首选 |
| auto-shell | 33,300 | 28(全在 job.rs Win FFI) | 13 | 8 | 1 | 42 | L3 |
| ash | 9,311 | 0 | 0 | 3 | 8 | 0 | L5 |
| ash-server | 2,977 | 0 | 5(cdylib ABI) | 5 | 0 | 43 | L4 |

2. **a2r 能力现状**(auto-lang trans/rust.rs ≈1.9 万行,test/a2r/ 34+ golden 组):
   已支持泛型/spec→trait/闭包/模式匹配/view-mut-take;async 已发射但**锚定
   tokio**(`#[tokio::main]`、`expr.go`→`tokio::spawn`);`Rc/RefCell/Mutex`
   仅透传(plan 060 调研结论);`#[no_mangle]` 导出发射**未验证**;
   `a2r_std::List` 为 RefCell 内可变容器(热路径性能未实测)。
3. **先例与教训**:①shell.at 曾以 891 行 .at 复刻 79 命令语义,因与 ash-core
   反复漂移退役(双源镜像不可维护——单源化原则的直接依据);②ash-gui-auto
   以 Auto 复刻手写 Vue GUI 成功(15 个 MCP GUI 测试锁行为——parity 网有效
   的正面样本);③plan 061 已把"a2r 后端产线"列为"另议",前置为 a2r codegen
   修复;④plan 065 在册:codegen `API_FUNCTIONS` 硬编码 demo 列表,至今未清偿。
4. **试点选型**:quote.rs(361 行/30 单测/零 FFI/零线程/零 async,shell 引号
   解析为逐键热路径,行为面由 30 单测锁定)优于 bookmarks.rs(89 行/0 测试)。
5. **工具链入口实测**:`auto run <file>`(VM)、`auto trans rust --path <file>`
   (单文件 a2r)、`auto test`(项目级)均存在于当前 auto.exe。
6. **目录事实**:`tests/` 已因 spike-m3 清理而空出,可承载 auto-parity;
   `designs/` 编号顺延至 037;plan 编号 080(NEXT.md 登记)。

## 详细设计

### D1 用例格式

- 输入侧:`NNN-<name>.cmd.json`——`{"fn":"quote_for_exec","args":["a b\"c"]}`;
- `.at` 用例:读同目录 cmd.json → 调 `quote` 模块函数 → `print(json.dumps(out))`
  (Auto 侧 JSON 化输出,避免裸 print 的格式歧义);
- 基准侧:ash-core fixture 测试读同一 cmd.json → 调 Rust API → serde_json 序列化
  **同一形状** → 写 `expected.json`;`--update-golden` 仅允许在 rust 侧使用。

### D2 三方一致性

- 输出规范化:统一 `serde_json::to_string_pretty` 形状(VM 侧用 Auto 的 json
  库等价形状;字符串转义差异在 runner 内归一);
- 逐用例三方 diff;报告含每方耗时(L0 性能基线的数据来源)。

### D3 quote 的 .at 化边界

- quote.rs 全部 pub fn 与错误类型语义进 `.at`;测试专用 helper 不迁;
- a2r-shell 只依赖 auto-lang(a2r_std),验证"零第三方依赖壳"成立;
- 手写 quote.rs **本计划不删**(A/B 并存受控期,L2 正式迁移时退役)。

### D4 性能基线

- 手写侧:复用 criterion(ash-core/benches 已有 harness),新增
  `benches/quote_bench.rs` 跑 quote 热路径;
- .at 侧:runner 记录 VM 与 a2r-release 各用例耗时;
- 数据(含机器/编译参数)抄录 designs/037 附录 A,给出比值结论。

### D5 DEBTS.md 外部前置登记

新增条目"Auto 单源化前置(a2r 引擎侧,L1)",判据可验证:容器基准达标值、
API_FUNCTIONS 硬编码清偿、`#[no_mangle]` 发射设计定稿——各附 auto-lang 侧
承接指针(其 plan 体系自管)。

## 测试设计

- 框架自测:一个恒等用例(000-ping)三方必须全绿(验证 runner 本身无 bug);
- quote 全量:30 个单测语义 → 用例集(预计 30-40 个用例),三方全绿或在册;
- ash-core 侧:`cargo test -p ash-core --test quote_parity_fixture` 生成并
  校验 expected.json 可复现;
- runner 幂等:重复运行报告一致;`--side` 单通道可独立执行。

## 验收标准

- C1 designs/037 成稿且含路线图表/准入退役标准/风险登记,编号与引用无悬空;
- C2 `python tests/auto-parity/run.py --side all` 在 quote 用例集上可跑通,
  恒等用例三方绿;quote 用例三方全绿或每处差异在 DEBTS.md 有对应在册条目;
- C3 性能基线数据落入 designs/037 附录 A(手写/VM/a2r-release 三列);
- C4 DEBTS.md 新条目存在且判据可验证;NEXT.md 已登记 080 并顺延至 081;
- C5 本仓 `cargo check --all-targets`(ash-core / ash / ash-server 三工作区)
  0 新增警告——fixture 与 bench 不破坏现有编译面。

## 执行步骤

- [ ] **T1** 写 `designs/037-auto-native-rewrite.md`(按架构方案节骨架,附录 A
  占位"待 T7 填数");`docs/plans/NEXT.md` 080→081 并登记 080 条目。
  验证:`ls designs/037-auto-native-rewrite.md && grep -c "L5" designs/037-auto-native-rewrite.md`
  (≥1);`grep "081" docs/plans/NEXT.md`。
- [ ] **T2** 建 `tests/auto-parity/` 骨架:README.md(用例格式/三方运行/
  --update-golden 规则)、run.py(argparse:--case/--side/--update-golden;
  通道函数 run_rust/run_vm/run_a2r 先落 ping 通路)、cases/ping/(000-ping
  三件套,期望输出 `{"pong":true}`)、a2r-shell/(Cargo.toml 仅 auto-lang
  path 依赖 + src/main.rs 读 case 打印)。gitignore 增 `tests/auto-parity/report/`
  与 `tests/auto-parity/a2r-shell/src/gen/`。
  验证:`python tests/auto-parity/run.py --side all --case ping` → ping 三方绿
  (rust 侧可先以独立小测试二进制或 cargo test 输出比对实现)。
- [ ] **T3** ash-core fixture:新增 `ash-core/tests/quote_parity_fixture.rs`,
  遍历 `tests/auto-parity/cases/quote/*.cmd.json` 调手写 quote.rs API,以
  serde_json pretty 写出/校验 `expected.json`。
  验证:`cargo test --manifest-path ash-core/Cargo.toml --test quote_parity_fixture`
  (T4 有用例后为实跑;T3 时以 ping 类自例先通管线)。
- [ ] **T4** 用例转写:对照 `ash-core/src/parser/quote.rs` 的 30 个 `#[test]`,
  逐个写 `cases/quote/NNN-<name>.at + .cmd.json`(语义=输入断言对),
  runner `--update-golden --side rust` 生成全部 expected.json。
  验证:`ls tests/auto-parity/cases/quote/*.at | wc -l` ≥ 30;
  `python tests/auto-parity/run.py --side rust` 全绿。
- [ ] **T5** VM 通道打通:run.py 调 `auto run <case>`(AUTO_BIN 环境变量可覆写,
  默认 auto-lang 主检出),输出归一后与 expected.json 比对;逐用例记录差异。
  验证:`python tests/auto-parity/run.py --side vm` 报告落 report/,绿/红清点
  与 DEBTS.md 新增条目(vm 差异)一致。
- [ ] **T6** a2r 通道打通:run.py 调 `auto trans rust --path cases/quote/quote.at`
  产物落 `a2r-shell/src/gen/`,`cargo build --release` 后逐用例执行比对。
  验证:`python tests/auto-parity/run.py --side a2r` 报告落 report/;红项全部
  进 DEBTS.md(a2r 差异,标注"L1 引擎侧")。
- [ ] **T7** 性能基线:新增 `ash-core/benches/quote_bench.rs`(criterion,
  引用 benchs 现有 harness 惯例);runner 增计时;三方耗时表抄入
  designs/037 附录 A 并写结论段。
  验证:`cargo bench --manifest-path ash-core/Cargo.toml --bench quote_bench`
  出数;附录 A 无"待填"占位。
- [ ] **T8** 收尾:DEBTS.md 定稿"Auto 单源化前置(a2r 引擎侧)"条目(合并 T5/T6
  红项与既有 plan 065 codegen 欠账指针);全文引用核对(设计文档↔计划↔DEBTS)。
  验证:三文件 grep 互指无悬空;`git status` 仅预期新增/修改文件。

## 复审记录

(留空,/auto-plan:review 填写)

## 待澄清事项

1. **Auto 源码目录约定**:正式迁移(L2+)时 .at 源放哪(推荐
   `ash-core/src-at/`,产物 `src/gen/` 构建再生)——L0 仅试点不受影响,
   设计文档按推荐落稿,复审可改;
2. **性能达标线**:附录 A 出数后才定(预计手写版 1.5-3 倍内可接受,
   超出则 L1 容器优化前置)——不在本计划拍死;
3. VM 侧 json 输出形状与 serde_json 的归一规则若遇不可归一差异,
   以"语义等值断言集"降级处理,规则写进 README 并在册。
