# ASH 安装指南

## 前置要求

- **Rust 1.75+**（安装：https://rustup.rs）
- **Git**
- 三个姊妹仓库需要同级放置（当前阶段）：

```
autostack/                        ← 你的工作根目录（名字随意）
├── auto-shell/                   ← 本仓库
├── auto-lang/                    ← AutoLang 语言引擎
└── auto-ai/                      ← AI 基础设施（aaid daemon）
```

> 未来发布到 crates.io 后，将解除姊妹仓库的路径依赖。

## 方式一：从源码构建（当前主推）

### 1. 克隆三个仓库

```bash
mkdir autostack && cd autostack

git clone https://github.com/zhaopuming/auto-shell.git
git clone https://github.com/zhaopuming/auto-lang.git
git clone https://github.com/zhaopuming/auto-ai.git
```

> 如果仓库未公开，用你有的访问方式（SSH / 本地路径）。

### 2. 构建 ash

```bash
cd auto-shell/ash
cargo build --release
```

构建产物：
- **Linux/macOS**：`target/release/ash`
- **Windows**：`target/release/ash.exe`

### 3. 加入 PATH（可选）

```bash
# Linux/macOS（加到 ~/.bashrc 或 ~/.zshrc）
export PATH="$PATH:/path/to/autostack/auto-shell/ash/target/release"

# Windows（加到系统环境变量，或用 PowerShell）
$env:PATH += ";D:\autostack\auto-shell\ash\target\release"
```

### 4. 验证

```bash
ash --version
ash -c "echo hello"
```

## 方式二：cargo install（计划中）

```bash
cargo install --git https://github.com/zhaopuming/auto-shell.git ash
```

> 此方式在 ash 发布到 crates.io 后完全可用。当前因路径依赖可能需要手动调整。

## 平台注意事项

### Windows

- 推荐用 **Git Bash** 或 **PowerShell** 构建
- ash 在 Windows 上原生运行（不依赖 WSL）
- `ash.exe` 是标准 Windows 二进制

### macOS

- 标准 cargo build 即可
- 路径处理使用 POSIX 风格（`/` 而非 `:`）

### Linux

- 标准 cargo build 即可
- 所有命令跨发行版一致

## AI 功能配置（可选）

ash 的 AI 功能（F3/F4 chat、SmartCommand）需要 AI 后端。三种方式：

### 方式一：aaid daemon（推荐）

aaid 是 auto-ai 的 LLM 网关 daemon，管理多个 provider：

```bash
cd auto-ai
cargo run -p auto-ai-daemon   # 启动 daemon（默认 127.0.0.1:17654）
```

配置文件：`~/.config/autoos/ai-daemon.at`

### 方式二：环境变量（直连 provider）

```bash
export ZHIPU_API_KEY="your-key"        # 智谱 GLM
# 或
export ANTHROPIC_API_KEY="your-key"    # Anthropic Claude
# 或
export OPENAI_API_KEY="your-key"       # OpenAI
```

### 方式三：不用 AI

ash 的 80 命令 + pipeline + 脚本不依赖 AI。AI 是可选增强。不配置 AI 时 F3/F4 会报错，但其他功能完全可用。

## 卸载

```bash
# 删除构建产物
rm -rf auto-shell/ash/target/

# 删除配置（可选）
rm -rf ~/.config/ash/
rm -f ~/.ashrc ~/.auto-shell-history
```

## 常见问题

### Q: `cargo build` 报找不到 auto-lang / auto-ai

确保三个仓库同级放置。检查 `auto-shell/ash/auto-shell/Cargo.toml` 里的 path 依赖指向正确（`../../../auto-lang/crates/auto-lang`）。

### Q: ash 启动时报 "aaid daemon unavailable"

如果你不用 AI 功能，忽略此警告。如果要用 AI，按上面的"AI 功能配置"启动 aaid 或设环境变量。

### Q: Windows 上构建慢

首次构建需要编译大量依赖（reedline/ratatui/syntect 等），约 3-5 分钟。后续增量构建很快。

### Q: 如何更新到最新版

```bash
cd auto-shell && git pull
cd ash && cargo build --release
```

---

**安装遇到问题？** 请提 [GitHub Issue](https://github.com/zhaopuming/auto-shell/issues)。
