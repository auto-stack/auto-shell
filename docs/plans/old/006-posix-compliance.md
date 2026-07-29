# Plan 006: POSIX 命令补全（flag + 行为偏差）

- **日期**: 2026-06-26
- **状态**: ✅ 已完成（P0+P1，2026-06-26）
- **目标**: 补齐 ash 传统 Unix 工具命令缺失的 POSIX flag/option，并修正 echo 的行为偏差。
- **范围**: 补缺 flag + 修行为偏差。**不含** head/tail/cut 的纯实现 bug（数值解析 stub 等，单独处理）。

## 1. 背景

调研了 23 个有 POSIX 对照的命令（详见各命令小节）。问题分三类：
- **第一类 缺 flag**（计划主体，机械补全，低风险）
- **第二类 行为偏差**（echo 默认不换行、split 不写文件）——需决策，本计划处理 echo
- **第三类 实现 bug**（head/tail 的 `-n` 硬编码 10、cut 参数解析混乱）——**不在本计划**，单独处理

调查发现的两个关键背景：
- **echo 有两套实现**：`cmd/commands/echo.rs`（Command trait）和 `cmd/builtin.rs:35 echo_command`（legacy 路径，`is_legacy_builtin` 优先）。两套都是 `positionals.join(" ")`，**默认不加结尾换行**，与 POSIX（默认输出后加 `\n`）冲突。
- **split 是有意的设计偏离**：ash 把它改造成"返回 chunk 数组"而非"写文件"（POSIX 语义）。这是 ash 管道哲学的一部分，**不对齐 POSIX 写文件行为**，只补参数。

## 2. 设计决策

### 决策 D1：echo 改为 POSIX 默认（加结尾换行）+ 支持 -n
**变更**：`echo` 默认输出后加 `\n`；`-n` 抑制换行。
**破坏性**：是。现有脚本若依赖 echo 不加换行会受影响。但 ash 处于早期，且这是正确的 POSIX 行为，值得对齐。
**实施**：两套实现（echo.rs + builtin echo_command）都要改，保持一致。

### 决策 D2：split 保持"返回数组"语义，不写文件
**不变更**行为，仅补 `-b`/`-a` 参数。split 的 POSIX 写文件语义与 ash 管道理念冲突，刻意保留 ash 风格。

### 决策 D3：仅补"用户会期望"的项，不强求 100% POSIX
聚焦高频/必选项，跳过极罕见的（如 tr 的等价类、date 的 `-r`）。

## 3. 分命令补全清单（按优先级）

### P0 — POSIX 必选 + 行为偏差（最该先做）

#### echo（行为偏差 + 缺 -n）★破坏性
- 改默认行为：输出末尾加 `\n`
- 加 `-n`（不输出结尾换行，POSIX XSI 必选）
- 加 `-e`/`-E`（启用/禁用反斜杠转义解析，GNU 常见）— 可选，P0 只做 `-n`
- **改两处**：`echo.rs` 的 run/run_atom + `builtin.rs` 的 `echo_command`
- 难度：低，但需全量回归（echo 被广泛用于测试）

#### touch（缺 POSIX 必选 -a/-m/-r/-t）
- 加 `-m`（只改 modification time）— 低
- 加 `-a`（只改 access time）— 中（跨平台 atime）
- 加 `-r ref_file`（用参考文件时间）— 中
- `-t time`（指定时间）— 高（时间格式解析），**P0 只做 -a/-m/-r，-t 延后**
- 难度：中（依赖 filetime shim 现状）

#### date（缺 -u 短形式、+format 操作数）
- 加 `-u` 短形式（POSIX 必选 UTC，目前只有 --utc）— 低
- 支持 `+format` 操作数语法（`date +"%Y-%m-%d"`，POSIX 唯一格式语法）— 低（解析首字符为 `+` 的 positional）
- 难度：低

#### cut（缺 POSIX 必选 -b/-c/-s）
- 加 `-c`（按字符切）— 低
- 加 `-b`（按字节切，注意 UTF-8 边界）— 低
- 加 `-s`（抑制无分隔符的行）— 低
- 难度：低

#### grep（缺 POSIX 必选 -E/-F/-q 等）
- 加 `-E`（extended regex）— 低
- 加 `-F`（fixed string）— 低（regex::escape）
- 加 `-q`（quiet，仅退出码）— 低
- 加 `-x`（whole-line match）— 低
- 加 `-w`（word match，加 `\b`）— 低
- `-e`/`-f`（显式模式/模式文件）— 低，可放 P1
- 难度：低（底层已有 regex crate）

### P1 — 高频缺失项

#### mkdir（缺 POSIX 必选 -m）
- 加 `-m mode`（创建时设权限）— 中（Unix 权限 + Windows 兼容）

#### mv / rm / cp（统一缺 -i）
- 三者都加 `-i`（interactive，覆盖前询问，POSIX 必选）— 低
- cp/rm 加 `-R`（大写递归别名，POSIX 标准形式）— 低
- mv/cp/rm 的 `--verbose` 补 `-v` 短形式 — 低

#### ls（缺高频项）
- 加 `-A`（almost all，不含 `.`/`..`）— 低
- 加 `-d`（目录本身不展开）— 低
- 加 `-1`（每行一项）— 低
- 加 `-F`（classify：目录加 `/` 等）— 低
- 加 `-p`（目录加 `/`，-F 子集）— 低
- `-S`（按大小排序）、`-i`(inode) 可放 P2

#### wc（已 POSIX 完备）
- 仅可选加 `-L`（最长行长度，GNU）— 低，P2

### P2 — 完整度（可延后）

#### sort（缺 -c/-o/-b/-d/-s 等）
- `-c`(check)/`-o`(output file)/`-b`(ignore leading blanks)/`-d`(dictionary)/`-s`(stable)
- 注意 `-b` 已被 Plan 003 的 `--ignore-case` 占用短名？需核查冲突（sort 的 POSIX `-b` = ignore blanks，但 ash 可能已用 -b 做别的）— **实施前核查短名冲突**

#### uniq（缺 -f/-s/-w）
- `-f`(skip fields)/`-s`(skip chars)/`-w`(compare N chars) — 低

#### cd/pwd（缺 -L/-P）
- 符号链接逻辑相关，低频，可延后

#### tee（缺多文件 + -i）
- 改 `required` 为支持多文件 operand — 低
- `-i`(ignore SIGINT) — 中（信号处理）

#### split（补参数，不改语义）
- `-b`(byte size)/`-a`(suffix length) — 低
- 保留返回数组语义

#### 其他基本完备的命令
- **cat**：仅可选 `-u`（POSIX 必选但无实际作用）— 跳过或加 stub
- **tr**：flag 完备，缺字符类 `[:alpha:]`（功能非 flag）— 不在本计划
- **ln/sleep/head/tail**：flag 基本够用，补 `--verbose` 短形式等小项
- **which/fmt/rev**：非 POSIX，按需

## 4. 实施策略

### 4.1 分期
- **本期（Plan 006 实施）**：P0（echo/touch/date/cut/grep）+ P1（mkdir/mv/rm/cp/ls 的核心项）
- **延后**：P2（sort/uniq/cd/tee/split 等）作为 Plan 006b 或后续

### 4.2 通用实施模式（每个命令）
1. TDD：加新 flag 的失败测试
2. 改 Signature：加 `flag_with_short` / `option_with_short`
3. 改 run：实现新 flag 行为
4. 回归：现有测试 + 全量

### 4.3 echo 破坏性变更的专项处理
- 改前先 grep 所有依赖 echo 不换行的测试，逐一评估
- 改两套实现（echo.rs + builtin echo_command）保持一致
- commit message 明确标注 BREAKING

## 5. 验收标准（P0+P1）

### 行为变更
- [ ] `echo hello` 输出 `hello\n`
- [ ] `echo -n hello` 输出 `hello`（无换行）
- [ ] echo 两套实现行为一致

### 新增 flag（抽样）
- [ ] `date -u` / `date +"%Y"` 工作
- [ ] `cut -c1-3` / `cut -s -d, -f1` 工作
- [ ] `grep -F "a.b" file` 字面匹配 / `grep -q` 仅退出码
- [ ] `touch -m` / `touch -a` / `touch -r ref`
- [ ] `mkdir -m 755 dir`
- [ ] `mv -i` / `rm -i` / `cp -i` / `cp -R`
- [ ] `ls -A` / `ls -d` / `ls -1` / `ls -F`

### 回归
- [ ] 全量 cargo test 通过（echo 改动后重点回归）
- [ ] 现有脚本兼容性评估完成

## 6. 风险

- **echo 破坏性变更**（最大风险）：改默认换行可能破坏依赖现状的测试/脚本。需全量回归 + grep 评估。
- **短名冲突**：补 flag 时可能与现有短名冲突（如 sort 的 `-b`）。每个命令实施前核查 `flag_with_short` 已用字母。
- **跨平台权限**：`mkdir -m`/`touch -a` 涉及 Unix 权限概念，Windows 上需降级处理（no-op 或 best-effort）。
- **工作量大**：P0+P1 约 10 个命令、30+ 个 flag。建议逐命令提交，便于回滚。

## 7. 不在本计划（YAGNI / 单独处理）

- head/tail/cut 的**实现 bug**（`-n` 硬编码 10、参数解析混乱）——单独 plan
- tr 的字符类 `[:alpha:]`（功能完整度，非 flag）
- sort 完整 keydef 语法（`-k 2.3,4`）
- `--color` 类渲染层功能（grep/ls 高亮）
- split 写文件语义（有意偏离，决策 D2）
- date `-d` 时间字符串解析（需日期库）
