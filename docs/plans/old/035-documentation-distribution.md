# Plan 035: ASH 文档 + 分发 — TDD 计划

> **日期**: 2026-07-23（M0-M2）／ 2026-08-01（M3 收尾）
> **分支**: `feat/035-docs-distribution`（M0-M2）／ `feat/033-plugin-ecosystem`（M3 收尾，主线已合并）
> **状态**: **✅ 完成（M0-M3 全部完成），可归档**
> **来源设计**: [`designs/035-documentation-distribution.md`](../../designs/035-documentation-distribution.md)
> **回归基线**: 文档 + 脚本为主，未触碰 ash 代码 → auto-shell 876 / ash-core 396 不变

---

## 0. 目标与范围

让 ash 能被外部用户**找到、装上、上手**。文档门面 + 三类用户入口 + 安装分发 + CI/release。

| 范畴 | 包含 | 不包含 |
|---|---|---|
| **README** | 根 README（定位 + 30 秒上手 + 三类用户入口） | 完整文档网站 |
| **安装** | 安装脚本 + build from source + cargo install --path | brew/winget（留 v2） |
| **quickstart** | `docs/quickstart.md`（5 分钟教程） | 完整教程 |
| **速查表** | `docs/bash-to-ash.md`（与 Plan 034 共建） | 迁移指南 |
| **三类入口** | for-agents / for-developers / for-internal | 营销素材 |

---

## 1. 交付物总览

### M0：README + quickstart + installation ✅
- `README.md`（项目门面：30 秒上手、三类用户入口、功能矩阵、文档索引）
- `docs/quickstart.md`（5 分钟教程）
- `docs/installation.md`（安装指南）

### M1：三类用户入口 + 速查表 ✅
- `docs/for-agents.md`（AI Agent 集成）
- `docs/for-developers.md`（终端用户）
- `docs/for-internal.md`（生态内部）
- `docs/bash-to-ash.md`（bash → ash 速查表，与 Plan 034 共建）

### M2：CI + release ✅
- `.github/workflows/ci.yml`（三平台测试 matrix + sibling 仓库 clone + clippy）
- `.github/workflows/release.yml`（tag 触发，三平台二进制）
- CI 通过 sibling-clone 布局解决路径依赖（`${{ github.repository_owner }}/auto-lang|auto-ai`）

### M3：`cargo install` 验证 + 安装脚本 ✅（2026-08-01 收尾）

**核心问题**：ash 的 `Cargo.toml` 用相对路径依赖姊妹仓库（`../../../auto-lang`、`../../../auto-ai`），`cargo install --git`（单仓 clone 到临时目录）解析不了这些路径。而 **Cargo 禁止 `path` + `git` 同时指定**（[官方文档](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)列为 INVALID），且无自动 fallback（[Issue #8747](https://github.com/rust-lang/cargo/issues/8747)）。

**解法**：安装脚本（v1 主线），不是改 Cargo.toml。
- `install.sh`（POSIX sh，macOS/Linux/Git Bash）：克隆三个 sibling 仓库到同级布局 → `cargo install --locked --path auto-shell/ash/auto-shell` → 清理
- `install.ps1`（PowerShell，Windows 原生）：同逻辑
- 已**实测验证**：`cargo install --path ash/auto-shell --root <tmp>` 在 sibling 布局下成功编译 auto-lang+auto-ai 并产出可用的 `ash --version` → `ash (AutoShell) v0.1.0`
- `docs/installation.md` 更新：安装脚本为方式一（推荐），源码构建为方式二；明确标注 `cargo install --git` 单仓暂不可用（待 crates.io 发布）

---

## 2. 已知小缺口（不阻塞归档）

| 缺口 | 说明 |
|---|---|
| `cargo install --git <单仓>` | Cargo 的 `path`+`git` 限制使其不可行；待 ash + sibling 发布 crates.io 后用 `cargo install ash` 解决 |
| `cargo install` 的真实端到端（clone github）验证 | 本地 `--path` 已验证成功；真实 `curl | sh` 跨网络验证留待公开后做 |
| M3 记录的 auto-lang 不稳定 | 已解除（auto-shell 当前编译通过），M3 已完成 |
| brew/winget/scoop | 设计明确标 v2 |

---

## 3. 附：实施过程记录（原 `035-implementation-status.md`）

> 本节保留 M0-M2 实施时的状态日志与 auto-lang 阻塞排查，作为历史记录。

### M0-M2 完成情况（2026-07-23）

- **M0**：README + quickstart + installation ✅
- **M1**：三类入口 + 速查表 ✅
- **M2**：CI + release ✅

### M3 曾受阻于 auto-lang 不稳定（已解除）

`035-implementation-status.md` 记录 M3 验证时 auto-lang 有编译错误（`parse_view_fragment_decl` 不存在 + `parse_view_block_inner` 变私有），导致 auto-shell 无法编译。这是 auto-lang 进行中开发导致的临时问题。**2026-08-01 核实：auto-shell 当前编译通过，auto-lang 已稳定，M3 已完成。**

文档里宣传的功能（agent CLI / 结构化 pipeline / 安全沙箱 / AutoLang 脚本 / F3/F4 AI）全部已实现，auto-lang 不稳定是临时的外部依赖问题，不影响文档准确性。

---

## 4. v2 / 后续

- `cargo install --git <单仓>` / `cargo install ash`（待 crates.io 发布 ash + sibling）
- brew / winget / scoop 包管理器
- 完整文档网站、营销素材
