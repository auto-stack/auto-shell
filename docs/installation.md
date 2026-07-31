# ASH 安装指南

## 前置要求

- **Rust 1.75+**（安装：https://rustup.rs）
- **Git**

ash 依赖两个姊妹仓库（auto-lang 语言引擎、auto-ai AI 基础设施）。当前阶段它们尚未发布到 crates.io，通过 `Cargo.toml` 的相对路径依赖引用，所以三个仓库需要同级放置：

```
autostack/                        ← 你的工作根目录（名字随意）
├── auto-shell/                   ← 本仓库
├── auto-lang/                    ← AutoLang 语言引擎
└── auto-ai/                      ← AI 基础设施（aaid daemon）
```

> 下面的「安装脚本」会自动处理这个布局。未来发布到 crates.io 后将解除姊妹仓库的路径依赖。

## 方式一：安装脚本（推荐）

一行命令克隆三个姊妹仓库并 `cargo install`，装完自动清理。最适合只想用 ash 的用户。

**macOS / Linux（或 Windows 上的 Git Bash）：**

```bash
curl -fsSL https://raw.githubusercontent.com/auto-stack/auto-shell/main/install.sh | sh
```

**Windows（PowerShell）：**

```powershell
irm https://raw.githubusercontent.com/auto-stack/auto-shell/main/install.ps1 | iex
```

脚本逻辑：克隆 `auto-stack/auto-shell`、`auto-stack/auto-lang`、`auto-stack/auto-ai` 到临时目录的同级布局 → `cargo install --locked --path auto-shell/ash/auto-shell` → 装到 `~/.cargo/bin`（cargo 默认）。

可选环境变量：

```bash
OWNER=myorg BRANCH=dev sh install.sh   # 用别的 owner / 分支
```

## 方式二：从源码构建（开发/自定义）

### 1. 克隆三个仓库（同级放置）

```bash
mkdir autostack && cd autostack

git clone https://github.com/auto-stack/auto-shell.git
git clone https://github.com/auto-stack/auto-lang.git
git clone https://github.com/auto-stack/auto-ai.git
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

或者直接 `cargo install` 到 `~/.cargo/bin`：

```bash
cargo install --locked --path auto-shell/ash/auto-shell
```

### 3. 加入 PATH（仅 build 方式需要；cargo install 方式装到 ~/.cargo/bin）

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

## 关于 `cargo install --git`（暂不支持）

```bash
cargo install --git https://github.com/auto-stack/auto-shell.git ash   # ❌ 当前不可用
```

**为什么不行**：`cargo install --git` 只克隆本仓库到临时目录，而 ash 的 `Cargo.toml` 用相对路径依赖姊妹仓库（`../../../auto-lang`、`../../../auto-ai`），这些路径在临时目录里解析不了。Cargo 也不支持 `path` + `git` 同时指定做 fallback。

**等 ash（及其姊妹仓库）发布到 crates.io 后**，`cargo install ash` 就能一行装好。在那之前，请用上面的**安装脚本**或**源码构建**。

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

**安装遇到问题？** 请提 [GitHub Issue](https://github.com/auto-stack/auto-shell/issues)。
