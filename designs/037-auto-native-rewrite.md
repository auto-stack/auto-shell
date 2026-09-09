# 037 — Auto 单源化迁移(Auto-Native Rewrite)设计文档

> 状态:设计稿(Plan 080 L0 交付物) · 2026-09-07
> 定位:长期路线的总设计。本文不是实施计划——每一期的落地由当期 plan 承接,
> 本文是它们的共同引用基点。附录 A 的性能数据由 Plan 080 T7 填入。

## 1. 背景与动机

AutoStack 生态的终局目标之一是**独立**:核心资产不锁死在某种宿主语言的
手写实现上。当前 auto-shell 仓的四个自研 crate 全部为手写 Rust:

| crate | 行数(src/ 实测) | unsafe | extern | 线程 | Rc/RefCell | async | 分期归属 |
|---|---|---|---|---|---|---|---|
| ash-core | 14,686 | 2 | 1 | 4 | 9 | 0 | L2 |
| auto-shell | 33,300 | 28 | 13 | 8 | 1 | 42 | L3 |
| ash | 9,311 | 0 | 0 | 3 | 8 | 0 | L5 |
| ash-server | 2,977 | 0 | 5 | 5 | 0 | 43 | L4 |
| **合计** | **≈60.3k** | | | | | | |

"Auto 复刻"的准确定义:**源码改用 AutoLang(.at)书写,经 a2r 转译为 Rust
参与编译,行为与被替换的手写 Rust 一致;手写版随迁移退役**。最终形态:

```
.at 源(单源) ──a2r──▶ 生成 Rust ──cargo──▶ ash 二进制/cdylib
        │
        └──auto run──▶ VM 直跑(开发/调试形态,同一语义)
```

### 1.1 历史教训(为什么必须"单源化")

- **shell.at 镜像之死**:曾以 891 行 .at 复刻 79 命令语义做 GUI 假后端,与
  ash-core 反复漂移(ls 分类、show 高亮均出过不一致),最终整体退役
  (shell.at 头注释留档)。结论:**两份行为等价实现必然漂移,镜像不可维护**。
- **ash-gui-auto 复刻之活**:以 Auto 复刻手写 Vue GUI,15 个 MCP GUI 测试
  锁行为,成功上位。结论:**有 parity 网 + 单源化决心,复刻可行**。

### 1.2 a2r 现状(2026-09 实测)

已支持:泛型 / spec→trait / 闭包 / 模式匹配 / view-mut-take 所有权
(借 Rust 编译器兑现)/ async 发射(`#[tokio::main]`、自动 await、
`expr.go`→`tokio::spawn`)/ `use.rust` 直接绑定外部 crate。
欠账(L1 域):`a2r_std::List` 为 RefCell 内可变容器,热路径性能未实测;
codegen `API_FUNCTIONS` 硬编码 demo 列表(Plan 065 在册);
`#[no_mangle]` extern 导出发射未验证;34+ golden 组覆盖语言语义但无
"替换手写库"级先例。

## 2. 单源化原则(全路线共同约束)

1. **替换而非镜像**:模块迁移完成的同一变更里删除对应手写 Rust;两源并存
   只允许出现在"受控 A/B 期"——必须有 auto-parity 用例网覆盖,且 A/B 期
   以周计,不允许常态化双源。
2. **产物不入库**:a2r 生成的 Rust 是构建产物,gitignore + 构建再生
   (对齐本仓 `gen/`、ash-core/Cargo.lock 惯例);形状由 golden 锁定
   (a2r 侧 `test/a2r/` 的 `.expected.rs` 机制延伸到本仓)。
3. **壳与核分离**:OS FFI / 线程泵 / cdylib ABI 导出**永久留在手写 Rust 壳**,
   Auto 只写核逻辑。壳清单(随迁移维护):
   - ash-core:`external.rs` / `external_stream.rs`(4 处进程泵);
   - auto-shell:`job.rs` 全部 Windows Toolhelp32 FFI(28 unsafe/13 extern);
   - ash-server:`backend.rs` 的 5 处 `#[no_mangle]` cdylib 导出 + 线程装配;
   - ash:TUI 事件循环与 reedline/ratatui 绑定(L5 评估后可能整层为壳)。
4. **热路径性能门**:任何模块迁移,发布构建的性能衰减不得超过手写版的
   1.5-3 倍区间(达标线由附录 A 数据定稿);超标则先触发 L1 容器优化,
   不带病迁移。
5. **VM 直跑等价**:同一 .at 源在 VM 直跑是开发/调试形态,语义差异属于
   引擎侧 bug(DEBTS 登记),不允许在业务侧写模式分叉。

## 3. 分期路线图

| 期 | 内容 | 主战场 | 出口判据 |
|----|------|--------|----------|
| **L0** | 设计(本文)+ 三方对齐框架(tests/auto-parity)+ quote 试点 + 性能基线 | auto-shell(Plan 080) | 附录 A 出数;框架三方绿;DEBTS 登记引擎侧前置 |
| **L1** | a2r 引擎侧清偿:容器性能(List/AutoStr)实测优化、codegen 欠账(API_FUNCTIONS 等)、`#[no_mangle]` 发射设计 | auto-lang(其 plan 体系自管) | DEBTS 对应条目判据全达 |
| **L2** | ash-core 纯逻辑层逐模块迁移:parser → data → completions;external 壳保留 | auto-shell | ash-core 纯逻辑模块 100% .at 源;examples_smoke/parity 全绿;性能达标 |
| **L3** | auto-shell 命令层:80+ 命令逐个迁移(准入序=依赖少→依赖多);job.rs 壳保留 | auto-shell | 命令层 .at 化率与 parity 全绿;FFI 壳边界冻结 |
| **L4** | ash-server 逻辑上移(worker/桥/路由);cdylib ABI 壳永留 | auto-shell | ash-server 业务逻辑 .at 源;guard_http 15 测试全绿 |
| **L5** | ash TUI 层:由 L0-L4 实测数据决定"迁移 / 永久 Rust 壳" | auto-shell | 决议文档 + 执行 |

依赖关系:L1 与 L2 前几个模块可并行(纯逻辑模块对容器性能不敏感);
L3 依赖 L2 的 data/pipeline 就位;L4 依赖 L1 的 async/no_mangle 结论;
L5 最后。

## 4. 模块准入/退役标准

**准入**(可开始迁移):
- 纯逻辑:零 FFI、零线程、零 `#[no_mangle]`;
- 有行为锁定网:现存单测,或可低成本转写为 auto-parity 用例;
- 依赖闭包内的已迁移模块或稳定外部 crate(use.rust 绑定)。

**退役**(手写版可删):
- auto-parity 用例三方全绿(或残留差异全部在册且判定为引擎侧);
- 发布构建性能达标(§2.4);
- 下游 crate 编译绿 + 既有套件(examples_smoke/parity、crate 单测)不红;
- 迁移变更内同时删除手写源(§2.1)。

## 5. 目录与产物布局(L2+ 起生效;L0 试点不受影响)

- Auto 源:`<crate>/src-at/`(与手写 `src/` 并置,逐模块挪移,挪完即删);
- 生成物:`<crate>/src/gen/`(gitignore;构建脚本驱动 `auto trans rust`);
- 壳:`<crate>/src/` 保留的手写文件列表在 crate 根 `AUTO-SHELL.md` 登记;
- 三方对齐网:`tests/auto-parity/`(L0 交付)按模块分子目录持续扩充。

## 6. 风险登记

| 风险 | 等级 | 缓解 |
|---|---|---|
| a2r 容器(List=RefCell)热路径性能不达标 | 高 | L0 试点先行实测;不达标则 L1 前置,壳内手写热路径兜底 |
| 双源漂移(A/B 期失控) | 高 | §2.1 受控 A/B 期 + parity 网门禁;超期未收敛即回滚 |
| async 锚定 tokio,L4 需解耦评估 | 中 | L4 前置调研项,不阻塞 L2/L3 |
| `#[no_mangle]` 发射缺失 | 中 | L1 设计项;壳永留方案兜底(现状即壳) |
| 行为对齐验证成本被低估 | 中 | 每模块准入先估用例数;quote 试点校准人时比 |
| VM 与 a2r 语义分叉(引擎 bug) | 中 | 差异一律在册转 auto-lang;不在业务侧写分叉 |

## 附录 A — quote 试点性能基线(Plan 080 T7,2026-09-09)

环境:本机 dev 机(Windows);Rust 手写侧 criterion `--release`
(`cargo bench --manifest-path ash-core/Cargo.toml --bench quote_bench`);
VM 侧为 auto-lang **debug** 构建 `auto.exe`,差分计时
(`cases/quote/_bench-loop.at` − `_bench-empty.at`,各 5 轮取最小墙钟,
2000 次/op);a2r 侧为 `cargo build --release`。

### A.1 手写 Rust(发布基线)

| 输入档 | parse_args | parse_args_preserve_quotes |
|---|---|---|
| simple(echo hello world) | 296 ns | 170 ns |
| quoted(cmd "arg with spaces" another) | 250 ns | 253 ns |
| winpath(open C:\Users\…\data.csv) | 250 ns | 219 ns |
| mixed(引号+转义+变量混合) | 545 ns | 405 ns |

### A.2 AutoLang VM(解释执行)

parse_args(quoted 档)≈ **801 µs/op**(差分:(1693.3 − 91.5) ms / 2000)。
相对手写 Rust 同档(250 ns)约 **3200×**——解释器 + 逐字节 substring
分配的复合开销,且 VM 引擎为 debug 构建(发布版会收窄但不改量级)。

### A.3 a2r 编译产物

**不可测**:30/30 用例编译阻塞于 E7(自定义函数调用点 owned→引用适配
缺失,DEBTS 在册)。E7 清偿后由 `tests/auto-parity/run.py --side a2r`
直接补数,本附录留待更新。

### A.4 结论

1. VM 直跑只适合开发/调试形态;quote 这类逐键热路径(补全、逐行解析)
   生产形态必须走 a2r 编译——与 designs/037 §1 的终局图一致。
2. a2r 产物的达标线锚定 A.1(0.25–0.55 µs/op 档),designs/037 §2.4 的
   1.5–3× 暂定区间**维持**,待 A.3 出数后定稿(E3 判据不变)。
3. L1 清偿的优先序由本次实测固化:**E7 > E1 > E6 > E5**(E7 阻塞整个
   a2r 通道的编译面,是单点闸门)。
