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

## M2:bash 等价校验 — 🟡 暂缓(被 auto-lang plan 378 阻塞)

### 演进:从"system() 桥接阻塞"到"plan 378 阻塞"

M2 一度被 5 个 system() 桥接 bug 阻塞(find 不认 `-name`、redirect 吞 stdout、`||` 链返回空、`ls -1` 报错、`$1` 传不进)。**这 5 个在 034 期间已全部修复**(见各 commit)。修完后验证发现:cwd 其实是对的(此前"cwd 错"是测试时的误判),`find -name`/`ls -1`/`||`/`$1` 都工作了。

但深入验证"有数据等价校验"(cleanup 找到 N 个文件、filestats 统计分布)时,暴露出**真正的阻塞:auto-lang 的 `.to_uint()`/`.len()` VM bug(plan 378)**。

### 当前阻塞:plan 378(native 方法返回 I64 的栈错位)

不只是 `filestats` 的 `to_uint()` 返回垃圾值——**`cleanup` 的 `str.lines().len()` 也是垃圾值**(`3-2147483647`)。根因同一:native 方法调用(`Expr::Dot`)返回 I64 时,codegen 的 `contains_u64`/`is_u64_expr` 不识别 `Expr::Dot`,误判为 I32(1 slot)→ 栈错位。

实测分类(2026-07-31,仓库根无参数跑):
- 🟢 **可跑(11)**:cleanup/cron-list/deploy/deps-check/disk-clean/du-top/fmt-check/git-batch/loccount/smart-commit/svc-status/user-activity — 但"可跑"指不崩溃,**多数的计数/聚合输出仍是垃圾值**(因 `.len()`/`.to_uint()`)
- 🟡 **缺参数(11)**:正常优雅报错
- 🔴 **to_uint 直接报错(4)**:buildtest/deploy-ai/filestats(`Invalid string ID`)

### cleanup 改造(避开 find -o)

cleanup 原用 `find \( -name *.tmp -o -name *.bak \)`(ash 的 find 不支持 `-o`/分组)。已改为**循环扩展名列表多次 find 再合并**(等价语义),验证:在有 a.tmp/b.log 的目录正确找到两个文件 ✅。但"找到 X 个"的 `lines.len()` 仍是垃圾值(plan 378)。

### M2 暂缓决定

"有数据等价校验"被 plan 378 全面阻塞——任何用 native 方法返回值(`.len()`/`.to_uint()`)做计数/聚合的脚本都产出垃圾值。固化垃圾值无意义,逐个改脚本绕过每个 `.len()` 是治标不治本。

**M2 待 plan 378(to_uint/len 栈错位修复)落地后恢复**。届时 filestats/loccount/csvsum/cleanup 等都能产出正确数值,M2 可做真正的 bash 等价。

详见 `designs/034-script-examples.md` 附录 B Bug 1 + 附录 C。

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
