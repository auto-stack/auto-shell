# auto-parity — 三方行为对齐框架

> Plan 080 L0 地基一。同一用例集在 **①手写 Rust(基准)②AutoLang VM ③a2r
> 编译产物** 三方运行,统一比对 `expected.json`;为 Auto 单源化迁移
> (designs/037)提供行为锁定网。

## 目录

```
run.py            一键 runner(--case/--side rust|vm|a2r|all/--update-golden)
cases/<模块>/     用例集(每用例三件套,见下)
a2r-shell/        最小 cargo 壳:编译 auto trans 产物并作为 bin 运行
report/           运行报告(gitignore;结论抄录 designs/037 附录)
```

## 用例三件套

- `NNN-<name>.at` —— 用例程序:读同目录 `NNN-<name>.cmd.json` 的输入,
  调被测模块函数,`print` 出 **JSON 文本**(一行一个 JSON 对象);
- `NNN-<name>.cmd.json` —— `{"fn": "<函数名>", "args": [...]}`(输入侧,
  三方共用);
- `NNN-<name>.expected.json` —— 行为基准。**由 rust 侧生成**
  (`--update-golden`,仅允许 rust 侧写),三方共同比对。

比对规则:stdout 解析为 JSON(整体或逐行)后与 expected 深比较,字符串
转义/空白差异在归一层吸收,不进入用例。差异即红,红项登记 DEBTS.md
(标注 vm/a2r 通道与所属引擎侧域),不允许为绕差异改 expected。

## 三条通道

| 通道 | 运行方式 | 说明 |
|---|---|---|
| rust | `cargo test --manifest-path ash-core/Cargo.toml --test quote_parity_fixture -- --nocapture`(env `PARITY_CASE` 可过滤;`PARITY_UPDATE=1` 写 golden) | 手写 Rust 为行为基准源 |
| vm | `AUTO_BIN <case>.at`(AUTO_BIN 默认 auto-lang 主检出 debug/auto.exe) | VM 直跑 |
| a2r | `AUTO_BIN trans --path <case>.at rust` → 产物拷入 `a2r-shell/src/bin/<case>.rs` → `cargo run --release --bin <bin>` | 单文件转译 + 最小壳 |

注:`auto trans` 的实参顺序为 `trans --path <file> rust`(option 在子命令前),
产物写输入同目录 `<stem>.a2r.rs`;runner 在 `report/tmp-trans/` 副本上转译,
不污染 cases/。

## 一键运行

```bash
python tests/auto-parity/run.py --side all                 # 全部用例 × 三方
python tests/auto-parity/run.py --side vm --case 000-ping  # 单用例单通道
python tests/auto-parity/run.py --side rust --update-golden  # 仅 rust 侧可写 golden
```

- 000-ping 为框架自测用例(恒等输出),三方必须全绿——它红说明 runner/通道
  本身有 bug,与被测模块无关;
- report/ 下每次运行产出 JSON 报告(含每方耗时,L0 性能基线数据来源)。
