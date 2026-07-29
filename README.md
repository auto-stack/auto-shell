# ASH — AI 时代的跨平台结构化 Shell

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue.svg)](#安装)

> **ASH (AutoShell)** 是一个用 Rust 编写的现代 shell，专为 AI 时代设计。命令之间交换的是**结构化数据**而非纯文本流；跨平台行为一致；内置安全沙箱，适合作为 AI Agent 的命令执行层。

---

## 为什么选择 ASH？

| 特性 | bash / pwsh | ASH |
|------|-------------|-----|
| **命令输出** | 文本流 | 结构化数据（18 种语义类型） |
| **Pipeline** | 文本管道 | 结构化管道（`ls \| filter .size > 10.mb \| sort .name`） |
| **跨平台** | bash≠pwsh≠zsh | 三平台同一行为 |
| **安全沙箱** | 无 | `--sandbox` / `--read-only` / `--no-network` / `--audit` |
| **AI 集成** | 无 | 内置 F4 chat + SmartCommand + Agent CLI |
| **脚本语言** | bash（陷阱多） | AutoLang（完整编程能力：闭包、try/catch、类型） |

### 30 秒上手

```bash
# 结构化 pipeline —— 不是文本，是带类型的数据
> ls | sort .size | head -n 5          # 按大小排序，取前 5
> ls | filter .type == "dir"           # 只看目录
> ps | filter .cpu > 1.0 | sort .mem   # 找吃资源的进程

# 数据格式转换 —— 原生支持，不需要 jq
> cat data.json | from_json | filter .age > 30 | to_csv

# AI Agent 接口 —— 让 Claude Code / Cursor 安全地调命令
$ ash agent describe-tools              # 79 个工具的 JSON Schema
$ ash agent check "rm -rf /tmp/old"     # 先问后做
$ ash agent run "ls -la /sandbox"       # 结构化信封输出

# 安全沙箱 —— 给 AI Agent 用，不失控
$ ash --sandbox /tmp --read-only -c "ls -la"
```

---

## 你是？

ASH 服务三类用户，各有专属入口：

### 🤖 AI Agent 开发者（Claude Code / Cursor / Codex）

把 ASH 当作**安全的命令执行层**。ASH 提供 Agent CLI 接口：

```bash
ash agent describe-tools          # 拉取 79 个命令的 JSON Schema
ash agent describe-policy         # 查看安全策略摘要
ash agent check "rm -rf /old"     # Dry-run：会被允许吗？
ash agent run "ls -la" --json     # 执行，拿结构化信封
```

→ 详细用法见 [docs/for-agents.md](docs/for-agents.md) · [SKILL.md](SKILL.md)（给 Agent 读的）

### 💻 终端用户（替代 bash / pwsh）

把 ASH 当**日常 shell**。80+ 内置命令（ls/grep/find/sort/...），POSIX 兼容 flag，AutoLang 脚本比 bash 强大：

```bash
# F1/F2/F3/F4 切换输入模式
# F1 = Shell 模式（默认，跟 bash 一样）
# F2 = AutoScript 模式（写 AutoLang）
# F3 = AI 一次翻译（自然语言 → 命令）
# F4 = AI 对话（多轮 chat）
```

→ 快速上手见 [docs/quickstart.md](docs/quickstart.md) · 从 bash 迁移见 [docs/bash-to-ash.md](docs/bash-to-ash.md)

### 🔧 AutoStack 生态内部（AutoCoder 等）

把 `ash-core` 当**纯逻辑引擎**（零终端依赖），或把 `auto-shell` 当完整 Shell：

```toml
# Cargo.toml
ash-core = { path = "../auto-shell/ash-core" }   # 纯逻辑
```

→ 详细见 [docs/for-internal.md](docs/for-internal.md)

---

## 安装

### 方式一：从源码构建（当前主推）

```bash
git clone https://github.com/zhaopuming/auto-shell.git
cd auto-shell/ash
cargo build --release
# 二进制：target/release/ash（Windows: target/release/ash.exe）
```

> **注意**：ASH 依赖姊妹仓库 `auto-lang` 和 `auto-ai`。目前需要它们在同级目录：
> ```
> autostack/
> ├── auto-shell/   ← 本仓库
> ├── auto-lang/    ← 语言引擎
> └── auto-ai/      ← AI 基础设施
> ```
> 未来发布到 crates.io 后将解除此限制。

### 方式二：cargo install（计划中）

```bash
cargo install --git https://github.com/zhaopuming/auto-shell.git ash
```

### 三平台支持

| 平台 | 状态 |
|------|------|
| Windows 10/11 | ✅ 主开发平台 |
| macOS | ✅ 支持 |
| Linux | ✅ 支持 |

→ 详细安装指南见 [docs/installation.md](docs/installation.md)

---

## 核心功能

### 80+ 内置命令

文件操作（ls/cp/mv/rm/find/grep/...）、文本处理（sort/uniq/cut/tr/...）、数据格式（from_json/to_json/from_csv/to_csv/from_yaml/to_yaml/from_toml/to_toml/from_xml/to_xml）、HTTP（http_get/post/put/delete）、系统（ps/sys/date/...）——全部跨平台一致。

### 结构化 Pipeline DSL

```bash
# Plan 024 DSL：filter / sort / select / group-by / sum / ...
> ls | filter .size > 10.mb | sort .name | select name size
> ps | filter .cpu > 1.0 | group-by .user | sum .cpu
> cat log.json | from_json | filter .level == "ERROR" | select timestamp message
```

### 安全沙箱

```bash
ash --sandbox /project --no-network     # 路径限制 + 禁网
ash --read-only                          # 只读
ash --allow ls --allow cat               # 白名单
ash --audit /var/log/ash-audit.jsonl    # 全审计
ash --dry-run                            # 只看不做
```

### AutoLang 脚本

比 bash 强大的脚本语言（闭包、try/catch、类型系统、递归）：

```bash
# .ash 脚本
fn deploy(env) {
    try {
        system("cargo build --release")
        system("scp target/release/app " + env + ":/opt/app")
        print("✓ deployed to " + env)
    } catch(e) {
        print("✗ failed: " + e)
        exit(1)
    }
}
```

→ 30+ 可抄的实例见 [examples/](examples/)

### AI 能力（Plan 027/029）

- **F4 chat**：多轮 AI 对话（Plan 027，已实现）
- **F3 NL→command**：自然语言翻译成命令（已实现）
- **SmartCommand**：AI 增强的结构化命令（Plan 029，设计中）

### Agent CLI（Plan 028，已实现）

```bash
ash agent describe-tools     # 79 个工具的 JSON Schema catalog
ash agent describe-policy    # 安全策略能力位摘要
ash agent check "<cmd>"      # Dry-run 策略探测
ash agent run "<cmd>"        # 执行 + 结构化信封
```

---

## 文档

| 文档 | 说明 |
|------|------|
| [docs/quickstart.md](docs/quickstart.md) | 5 分钟上手 |
| [docs/bash-to-ash.md](docs/bash-to-ash.md) | bash → ash 速查表 |
| [docs/for-agents.md](docs/for-agents.md) | AI Agent 集成指南 |
| [docs/for-developers.md](docs/for-developers.md) | 终端用户指南 |
| [docs/for-internal.md](docs/for-internal.md) | 生态内部集成 |
| [docs/installation.md](docs/installation.md) | 详细安装 |
| [docs/roadmap.md](docs/roadmap.md) | 项目路线图 |
| [examples/](examples/) | 脚本实例库 |
| [SKILL.md](SKILL.md) | 给 AI Agent 的技能说明 |

---

## 项目结构

```
auto-shell/
├── ash-core/          纯逻辑引擎（零终端依赖）：parser、pipeline、security、completions
├── ash/               CLI/TUI workspace：reedline REPL + ratatui 渲染 + 80 命令
│   └── auto-shell/    ash 二进制 crate
├── ash-gui/           GUI workspace（iced/AutoUI，Plan 030 设计中）
├── designs/           设计文档（Plan 029-035）
├── docs/              用户文档
│   └── plans/         实施计划（Plan 014-030）
└── examples/          脚本实例
```

---

## 开发

```bash
# 测试
cd ash && cargo test -p ash-core && cargo test -p auto-shell

# 构建
cargo build --release

# 运行
cargo run -- -c "ls | sort .size | head"
```

### 设计与计划文档

所有扩展方向的设计文档在 [designs/](designs/)，实施计划在 [docs/plans/](docs/plans/)。横向一致性检查见 [designs/000-cross-cutting-review.md](designs/000-cross-cutting-review.md)。

---

## License

MIT
