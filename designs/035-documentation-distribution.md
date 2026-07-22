# Plan 035: ASH 文档 + 分发设计(采用门槛)

> **日期**: 2026-07-21
> **状态**: 设计中(待评审)
> **战略驱动**: 补齐采用漏斗前段(知道存在 → 能装上 → 能跑起来),让三类用户(AI Agent / 开发者 / 生态内部)都能迈过最低门槛。这是所有功能方向的**前置条件**——029/030/031 做完了也得有 README 才有人下载
> **范围**: README + quickstart + 安装入口 + bash→ash 速查表 + 三类用户入口文档
> **预估**: 1-2 周

---

## 愿景

> **让任何人在 5 分钟内装上 ash、看到价值、知道下一步去哪**。这是"产品化"的最低门槛——从"一个能跑的内部工具"变成"一个能被采用的产品"。

### 现状(采用漏斗前段是空的)

```
知道 ash 存在  →  能装上  →  能跑起来  →  觉得好用  →  留下来
   ❌ 无 README    ❌ 无安装    ✅ CLI 可用    ⚠️ 缺实例    ⚠️ 缺独家能力
```

- **全项目根目录无 README**(只有 `SKILL.md` 给 AI Agent 读)
- 无 `cargo install` 入口,无 release 二进制,无安装文档
- 无 quickstart,无 bash→ash 迁移桥
- `docs/` 只有 `roadmap.md` + release notes

### 范围内 / 范围外

| 范畴 | 包含 | 不包含 |
|---|---|---|
| **README** | 根 README.md(定位 + 30 秒上手 + 三类用户入口) | 完整文档网站 |
| **安装** | `cargo install` + build from source + release 二进制 | brew/winget(留 v2) |
| **quickstart** | `docs/quickstart.md`(5 分钟教程) | 完整教程 |
| **速查表** | `docs/bash-to-ash.md`(跟 Plan 034 共享) | 迁移指南 |
| **三类入口** | for-agents.md / for-developers.md / for-internal.md | 营销素材 |

---

## 第 1 节:文档结构

```
README.md                    ← 项目门面(给所有人)
docs/
├── quickstart.md            ← 5 分钟上手(给开发者)
├── bash-to-ash.md           ← 速查表(Plan 034 共建)
├── for-agents.md            ← AI Agent 入口(跟 SKILL.md 对齐)
├── for-developers.md        ← 开发者入口(脚本/补全/SmartCommand)
├── for-internal.md          ← 生态内部入口(ash-core/auto-shell 作为依赖)
├── installation.md          ← 详细安装(三平台)
├── roadmap.md               ← (已有)
└── release_notes/           ← (已有)
```

### 1.1 README.md(根,最重要)

```markdown
# ASH — AI 时代的跨平台结构化 shell

[badges: CI / license / version]

ASH 是一个用 Rust 写的现代 shell,专为 AI 时代设计:
- **结构化输出**(不是文本流,是带类型的数据)
- **跨平台一致**(Windows/macOS/Linux 同一行为)
- **安全沙箱**(给 AI Agent 当 tool use 层,不失控)
- **内置 AI**(SmartCommand + F4 tool-calling + 自然语言)

## 30 秒上手

    cargo install ash
    ash
    > ls | sort .size | head        # 结构化 pipeline
    > ash agent describe-tools      # 给 AI Agent 的 tool catalog

## 你是?

- **[AI Agent 开发者](docs/for-agents.md)** —— 把 ash 当安全的命令执行层
- **[终端用户](docs/for-developers.md)** —— 用 ash 替代 bash/pwsh
- **[AutoStack 生态](docs/for-internal.md)** —— 把 ash-core 当底层引擎

## 安装

见 [installation.md](docs/installation.md)。三平台支持。

## 从 bash 迁移

见 [bash-to-ash 速查表](docs/bash-to-ash.md)。

## 功能

- 80+ 内置命令(ls/grep/find/... + from_json/to_csv/...)
- 结构化 pipeline DSL(`ls | filter .size > 10.mb | sort .name`)
- 安全沙箱(--sandbox/--read-only/--no-network/--audit)
- AutoLang 脚本(比 bash 强大的编程能力)
- AI: F4 chat + F3 NL→command + SmartCommand(Plan 029)
- Agent CLI(`ash agent run/check/describe-tools`,Plan 028)
- [实例库](examples/)(30+ 可抄的脚本)
```

### 1.2 quickstart.md(5 分钟)

- 安装(一行)
- 基础命令(ls/cat/grep,跟 bash 一样)
- 结构化 pipeline(第一个 aha moment:`ls | sort .size | head`)
- 脚本(写个 .ash)
- AI(F4 chat 或 F3)
- 下一步(链接到实例库/速查表/for-developers)

### 1.3 for-agents.md(AI Agent 入口)

- `ash agent describe-tools` / `describe-policy` / `check` / `run`(Plan 028)
- SKILL.md 的内容(给 Agent 读的 ash 说明)
- 信封 schema(schema_version/status/data.kind/error.kind)
- 示例:Claude Code / Cursor / Codex 怎么调 ash

### 1.4 for-developers.md(终端用户入口)

- REPL 交互(F1/F2/F3/F4 模式切换)
- 补全系统(Tab + ghost-text + Plan 032 AI 补全)
- SmartCommand(`ash smart`,Plan 029)
- AutoLang 脚本(链接实例库 Plan 034)
- 配置(~/.ashrc + config.at)

### 1.5 for-internal.md(生态内部入口)

- ash-core 作为依赖(纯逻辑,零终端依赖)
- auto-shell 作为依赖(Shell 引擎)
- Plan 014 分层架构(TUI/GUI 双前端)
- 跟 auto-lang/auto-ai/auto-ui 的关系

---

## 第 2 节:安装入口

### 2.1 cargo install(v1 主线)

```bash
cargo install --git https://github.com/zhaopuming/auto-shell ash
```

或 ash 上 crates.io 后:
```bash
cargo install ash
```

**前置条件**:需要 auto-lang + auto-ai 在 sibling 目录(当前路径依赖)。v1 文档说明这一点;v2 发布到 crates.io 后无此限制。

### 2.2 build from source(已有)

```bash
git clone https://github.com/zhaopuming/auto-shell
cd auto-shell/ash
cargo build --release
# 二进制在 target/release/ash
```

### 2.3 release 二进制(GitHub Actions CI)

GitHub Actions 三平台 matrix(ubuntu/macos/windows),每个 release 自动构建 `ash` / `ash.exe`,附到 GitHub Release。

### 2.4 brew/winget(v2,不在本 Plan)

留后续。v1 用 cargo install + release 二进制覆盖。

---

## 第 3 节:CI 配置(GitHub Actions)

### 3.1 三平台测试 matrix

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, macos-latest, windows-latest]
steps:
  - uses: actions/checkout@v4
    with: { submodules: recursive }  # auto-lang/auto-ai sibling
  - run: cargo test -p ash-core
  - run: cargo test -p auto-shell
  - run: cargo test -p auto-shell --no-default-features  # Plan 030 M0 feature 隔离
```

### 3.2 release 构建

```yaml
on: { push: { tags: ['v*'] } }
jobs:
  build:
    strategy: { matrix: { os: [ubuntu, macos, windows] } }
    steps:
      - run: cargo build --release
      - uses: actions/upload-release-asset
        with: { asset_path: target/release/ash${{ exe_suffix }} }
```

---

## 第 4 节:里程碑

### M0:README + quickstart(2-3 天)
- 根 README.md
- docs/quickstart.md
- docs/installation.md

### M1:三类用户入口 + 速查表(2-3 天)
- for-agents.md / for-developers.md / for-internal.md
- bash-to-ash.md(跟 Plan 034 共建)

### M2:CI + release(2-3 天)
- GitHub Actions 三平台测试
- release 二进制自动构建
- 首个 release(打 tag)

### M3:cargo install 验证(1 天)
- 验证 `cargo install --git ...` 可跑
- 修路径依赖问题(如果需要)

**总计**:约 1.5-2 周。

---

## 第 5 节:跟其他方向的关系

| 方向 | 关系 |
|---|---|
| **所有方向** | A 是前置:README 要引用所有功能;quickstart 展示核心价值 |
| **Plan 034**(实例库) | 实例是 quickstart/README 的素材;速查表共建 |
| **Plan 028**(Agent) | for-agents.md 的核心内容 |
| **Plan 029**(AI) | for-developers.md 的 SmartCommand/F4 部分 |
| **Plan 030**(ash-gui) | README 提及 GUI(链接到 design) |
| **Plan 033**(插件) | for-developers.md 提及插件机制 |

---

## 附录:现有资产

- 根 `SKILL.md` —— 给 AI Agent 的 ash 说明(已有,for-agents.md 跟它对齐)
- `docs/roadmap.md` —— 项目战略 roadmap(已有)
- `docs/release_notes/v0.4.md` / `v0.5.md` —— 发布说明(已有)
- `examples/deploy.ash` —— 唯一实例(Plan 034 扩充)
- `ash/auto-shell/README.md` —— **存在但极简**(只有安装 + 基础说明,非项目门面)
