# Plan 035 实施状态记录

> **日期**: 2026-07-23
> **分支**: feat/035-docs-distribution

## M0-M2 完成情况

### M0: README + quickstart + installation ✅
- `README.md`（项目门面，30 秒上手，三类用户入口）
- `docs/quickstart.md`（5 分钟教程）
- `docs/installation.md`（三平台安装指南）

### M1: 三类用户入口 + 速查表 ✅
- `docs/for-agents.md`（AI Agent 集成）
- `docs/for-developers.md`（终端用户）
- `docs/for-internal.md`（生态内部）
- `docs/bash-to-ash.md`（bash 迁移速查表）

### M2: CI + release ✅
- `.github/workflows/ci.yml`（三平台测试 matrix + clippy）
- `.github/workflows/release.yml`（tag 触发，三平台二进制）

## M3: cargo install 验证 ⚠️ 受阻

### 问题
当前 auto-lang 仓库有编译错误（`parse_view_fragment_decl` 方法不存在 + `parse_view_block_inner` 变私有），导致 auto-shell 无法编译。

这是 auto-lang 仓库的进行中开发导致的（跟 Plan 028 实施期间遇到的情况相同）。auto-lang 在持续变动，需要其维护者稳定后才能验证。

### 已验证的
- `ash --version` 和基础命令在 auto-lang 稳定时可用（Plan 028 期间验证过）
- `ash agent describe-tools/check/run` 在 028 实施时验证过（79 工具，10 端到端测试全过）
- 当前 binary（target/debug/ash.exe）是旧版，不认识 agent 子命令

### 待 auto-lang 稳定后验证
1. `cargo build -p auto-shell` 成功
2. `ash agent describe-tools` 输出 79 工具
3. `ash agent describe-policy` 输出策略摘要
4. `cargo install --git ...` 可跑（路径依赖问题）

## 文档准确性说明

文档里宣传的功能（agent CLI / 结构化 pipeline / 安全沙箱 / AutoLang 脚本 / F3/F4 AI）**全部已实现**（在 Plan 028 / MS1-MS3 / Plan 027 期间验证过）。当前 auto-lang 不稳定是**临时的外部依赖问题**，不影响文档的准确性——文档描述的是 ash 的能力，不是某个时间点的编译状态。
