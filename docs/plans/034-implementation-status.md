# Plan 034 实施状态记录

> **日期**: 2026-07-31
> **分支**: `feat/034-script-examples`(待合并 main)
> **设计**: [`designs/034-script-examples.md`](../../designs/034-script-examples.md)
> **状态**: 🟡 M0/M1/M3 完成;M2 暂缓(被 system() 桥接架构问题阻塞,见附录 C)

## 总览

| 里程碑 | 内容 | 状态 |
|---|---|---|
| **M0** | 修复损坏脚本(ext bug)+ 补 3 个 README + 修瑕疵 | ✅ |
| **M1** | 冒烟测试(31 脚本不崩溃) | ✅ |
| **M2** | bash 等价校验 | 🟡 暂缓(根因超出 034 范围) |
| **M3** | bash→ash 速查表修正 + README 一致性 + deploy 收拢 | ✅ |

## M0:修复损坏脚本 + 补 README + 修瑕疵 ✅

- **M0.1** filestats.ash 是唯一有 ash 解析错误的脚本(标识符 `ext` 重赋值触发 auto-lang parser bug)。改 `ext`→`extension`(filestats + loccount 同模式)。
- **M0.2** 补 3 个 README 缺口:csvsum 新建;du-top/filestats 补 bash 对照段(27/30 → 30/30 有 bash 对照)。
- **M0.3** 修瑕疵:disk-clean 显示 `100-2147483647MB`(to_uint VM bug 污染算术)→ 改用 find `-size +NM` 原生单位;loggrep/bigfiles 的 `$@` 字面量 → 当空处理显示用法。

**结果**:32 个脚本零 ash 解析错误(原 1 个损坏)。

## M1:冒烟测试守护 ✅

新建 `tests/examples_smoke.rs`:跑全部 31 个 example 脚本,断言不崩溃(parse error / VM panic / >15s 超时)。

关键设计:
- **不要求 exit 0**:环境依赖脚本(deploy-ai/git-batch/csvsum)在缺前置条件时正确 `exit(1)` 是优雅退出,非崩溃。
- **超时保护**:每个脚本线程+channel 跑,15s 上限,避免交互/循环脚本 hang 死整个套件。
- **失败判据**:ash parse error(`unexpected token`)、VM panic、超时。

**结果**:31/31 通过(~12s)。这是 example 库此前完全缺失的回归网。

## M2:bash 等价校验 — 🟡 暂缓(根因记录在 design 附录 C)

实施 M2 时深入调查发现一个**比预期严重得多的架构问题**,导致 bash 等价校验不可行。

### 根因(2026-07-31 实测)

example 脚本普遍用 `system("find . -name *.rs")` 等,假设 system() 走真 bash。实测:`system()` 走 `Shell::execute_capture → execute()`,即**让 ash 自己执行命令**。ash 对 find/grep/wc/du 有**内置重实现**,语法与 GNU 不同。

**决定性证据**(在含 163 个 .rs 文件的目录):

| 命令 | 真 bash | ash `system()` |
|------|---------|---------------|
| `find . -name '*.rs'` | 163 个 | **0** |
| `find . -n *.rs` | (bash 不认) | 4452 字符(正常) |
| `ls src` | 列出 | 列出(兼容) |
| `git status` | 正常 | 正常(无内置,fallback 外部) |

即 ash 的 find 用 `-n`/`name`,**不认 GNU 的 `-name`**。脚本写 GNU 语法 → find 返回空 → filestats/loccount/cleanup 等恒输出"0/没找到"(错误结果)。

### 为什么阻塞 M2

M2 要求脚本产出正确稳定结果。当前多数 example 因语法不匹配产出**错误结果**(恒 0/空),固化它等于把 bug 固化成期望,bash 等价会大面积失败。

### 三种修复方向(均超 034 范围,需单独决策)

1. 改 example 脚本用 ash 语法(`find -n`)
2. 让 ash 内置命令兼容 GNU 语法别名
3. system() 增加"真 shell-out"模式

详见 `designs/034-script-examples.md` 附录 C。

## M3:文档收尾 ✅

- **速查表修正**:`docs/bash-to-ash.md` 原把 `find . -name "*.rs"` 标为"相同"(错误),改为明确警告 find/grep 标志位不同。
- **deploy 收拢**:散落的 `examples/deploy.ash` 移进 `examples/deploy/` + 配 README。
- **README 一致性**:`examples/README.md` 加 system() 语法注意事项 + deploy 实例;`README.md` 加速查表链接。

## 成功指标对照(design §4)

1. ✅ **M0**:损坏脚本修复(32/32 无解析错误)
2. ✅ **M1**:冒烟测试守护(31/31 不崩溃)
3. 🟡 **M2**:bash 等价校验被架构问题阻塞(根因已查明并记录)
4. ✅ **M3**:速查表修正 + README 一致

## 影响文件

- 改:`examples/{filestats,loccount,disk-clean,loggrep,bigfiles}/*.ash`
- 新/改:`examples/{csvsum,du-top,filestats,deploy}/*.md`、`examples/deploy/deploy.ash`(从根移入)
- 改:`examples/README.md`、`docs/bash-to-ash.md`、`README.md`
- 新:`ash/auto-shell/tests/examples_smoke.rs`
- 改:`designs/034-script-examples.md`(附录 C:system() 调查)

## 非目标 / 暂缓

- ❌ 不修 auto-lang 的 `ext` parser bug(脚本侧绕过)
- ❌ 不修 auto-lang 的 `.to_uint()` bug(disk-clean 改用原生单位绕过)
- 🟡 **M2 bash 等价校验暂缓**:待 system() 桥接 / find-grep 语法兼容的修复方向落地后恢复
