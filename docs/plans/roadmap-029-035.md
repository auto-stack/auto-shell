# ASH 扩展方向实施路线图(029-035)

> **日期**: 2026-07-21
> **状态**: 路线图(待阶段 4 确定实施顺序后,逐个展开详细 TDD plan)
> **范围**: 汇总 6 个方向(029/031/032/033/034/035)的里程碑、依赖、动作项
> **关联**: 各方向 design 在 `designs/029-035-*.md`;横向检查在 `designs/000-cross-cutting-review.md`

---

## 总览:7 个方向的 design 状态

| Plan | 方向 | Design | Plan(详细 TDD) | 估算 |
|---|---|---|---|---|
| **028** | Agent 执行引擎 | (已删,M1+M2 已落地) | (已删) | M3+M4 待定 |
| **029** | AI 能力增强(含 SmartCommand) | ✅ 1064 行（v1 已过时，§0 已重评） | ✅ 完成（5 子能力全落地，见 `old/029-ai-capabilities.md`） | ✅ 完成（仅 1 个不阻塞的小缺口：`.at` 配 preferred_provider） |
| **030** | ash-gui(Shell-native UI) | ✅ 1048 行 | 🟡 M0-M5 完成（feature 隔离 + Renderer trait + 最小 GUI + Block 列表 + 全 AtomType + 工具浏览器），AI 面板 deferred | 核心假设已验证（M2）；AI 面板待 AI 后端 |
| **031** | 数据处理框架(lazy pipeline) | ✅ 538 行 | ✅ 已归档（M0-M3 完成） | ✅ 完成 |
| **032** | 智能补全(AI 层) | ✅ 405 行 | ✅ 已实施并归档(M0-M3 + 审计修复,见 `old/032-intelligent-completion.md`) | ✅ 完成 |
| **033** | 插件生态(data-only) | ✅ 437 行 | ✅ 完成（M0-M3 全部完成 + 复审修复，见 `old/033-plugin-ecosystem.md`） | ✅ 完成（v1） |
| **034** | 脚本实例库 | ✅ 194 行 | ✅ 已实施并归档(M0/M1/M3 + M2 核心等价,见 `old/034-script-examples.md`) | ✅ 完成 |
| **035** | 文档+分发 | ✅ 240 行 | ✅ 完成（M0-M3，见 `old/035-documentation-distribution.md`） | ✅ 完成 |

---

## 依赖图(无循环,自底向上)

```
Layer 0(已落地):
  028 Agent 引擎 M1+M2(已落地 main)
  Plan 021 补全引擎(已落地)
  MS1-MS3(80 命令 + 沙箱 + AutoLang)

Layer 1(地基,无 ash 内部依赖):
  029 §2 共享基础设施
    ├─ OllamaProvider + preferred_provider(auto-ai 改造)
    ├─ AshCommandTool(ash → auto-ai-agent 桥)
    └─ 上下文 builder + Shell 公开访问器
  031 M0 Stream bug 修复 + Format trait

Layer 2(核心功能,依赖 Layer 1):
  029 §3 SmartCommand(依赖 §2)
  029 §4 F4 tool-calling(依赖 §2)
  031 M1-M2 LazyNode + 谓词下推(依赖 M0)
  032 M0-M1 上下文 plumbing + 排序(依赖 029 §2.3 访问器)

Layer 3(增强,依赖 Layer 2):
  029 §5 F3 NL→pipeline(依赖 §4 的 LLM 调用模式)
  029 §6 NL→AutoLang(依赖 Shell::eval_auto)
  029 §7 上下文感知 + Warp 式建议(依赖 §2.3)
  032 M2 AI 补全(依赖 029 NL 共享层)
  033 插件加载器(依赖 029 SmartCommand loader + 021 load_dir)

Layer 4(生态+采用,依赖 Layer 1-3):
  033 CLI + 分发(依赖加载器)
  034 实例库(依赖 029/031 展示其能力)
  035 文档+分发(依赖所有,引用所有)
  030 ash-gui M0-M5(依赖 028/029,独立 workspace)

Layer 5(独立后续):
  028 M3 批量 NDJSON(独立,效率优化)
  028 M4 跨平台测试(独立,质量守护)
```

---

## 各方向里程碑速查

### 029 AI 能力增强(最大,12-16 周) ✅ 完成（5 子能力全落地）

| M | 内容 | 依赖 | 状态 |
|---|---|---|---|
| M0 | 共享基础设施(OllamaProvider + 桥 + 上下文) | auto-ai 改造 | ✅ |
| M1 | SmartCommand 完整 + git.finish-worktree | M0 | ✅ |
| M2 | F4 tool-calling(ChatSession → Agent::run) | M0 | ✅ |
| M3 | F3 增强 + NL→AutoLang | M2 | ✅ |
| M4 | 上下文感知 + Warp 式建议 | M0 | ✅ |

详见 `old/029-ai-capabilities.md`（auto-shell ~2700 行 + 43 测试，auto-ai 侧前置依赖已合并）。唯一小缺口：`.at` 配置层未加 `preferred_provider` 字段（不阻塞，低优先级）。

### 030 ash-gui(已有详细 plan) 🟡 M0-M5 完成（核心假设已验证，AI 面板 deferred）

| M | 内容 | 状态 |
|---|---|---|
| M0 | auto-shell feature 隔离（`frontend-tui` feature，`--no-default-features` lib 可编译） | ✅ |
| M1 | Renderer trait + RenderedOutput（ash-core 纯逻辑）+ TuiRenderer + golden 对比（视觉零变化） | ✅ |
| M2 | 最小 GUI（**关键检查点**：ash-gui-bin 跑起来，ls → iced 表格 widget；核心假设验证通过） | ✅ |
| M3 | Block 列表（命令历史 + 状态着色）+ 历史导航（↑↓）+ 命令名补全 | ✅ |
| M4 | 全 AtomType 渲染（Record 路由 + MemoryInfo 进度条 + atom_type 分派）+ CellTag 点击打开文件 | ✅ |
| M5 | 工具浏览器侧边栏（79 命令 + 描述，可折叠）+ SmartCommand 浏览器（列出 smart 命令 + 插入运行） | ✅ |
| M5（deferred） | AI 面板（F4 chat + NL→SmartCommand）—— 需 auto-ai-client + 运行的 daemon | 未做 |

详见 `030-ash-gui.md`。**M2 关键检查点通过**——"结构化 Atom → 富 widget"的核心假设成立。M3-M5 把它从单输出演示打磨成日常可用的 GUI 终端：Block 历史、历史导航、补全、全 AtomType 渲染、MemoryInfo 仪表、点击打开文件、工具/SmartCommand 浏览器侧边栏。

**M5 的 AI 面板（F4 chat + NL→SmartCommand）deferred**：GUI 当前无 auto-ai-client 依赖、Shell 无 AI client，AI 面板需要运行的 daemon 才能验证。这是 M5 中唯一需要 AI 后端的部分；工具浏览器 + SmartCommand 浏览器（纯 UI + Shell 能力）已完成。

### 031 数据处理(独立性强,4-6 周)

| M | 内容 | 估算 |
|---|---|---|
| M0 | Stream bug 修复 + Format trait | 1-2 周 |
| M1 | LazyNode + 基础算子 | 2 周 |
| M2 | 谓词下推 + shell.rs 集成 | 1-2 周 |
| M3 | ExternalStream → lazy(可选) | 1 周 |

### 032 智能补全(4-5 周) ✅ 完成（M0-M3 + 审计修复）

| M | 内容 | 依赖 | 状态 |
|---|---|---|---|
| M0 | 上下文 plumbing | 029 §2.3 访问器 | ✅ |
| M1 | 排序 + 历史 ghost-text | M0 | ✅ |
| M2 | AI 补全(LLM/NL) | 029 NL 共享层 | ✅ |
| M3 | 缺失动态源(可选) | 无 | ✅ |

**v2 / 后续**（v1 有意不做，记录于此防遗忘；详见 `old/032-intelligent-completion.md` §非目标与 `designs/032-intelligent-completion.md` §范围内/范围外）：
- AI 实时 ghost-text（打字时调 LLM）—— 延迟不可接受，不做
- 云端 LLM 补全 —— 只用本地 Ollama
- 重写 Plan 021 补全引擎 —— 只在其后加 AI 层
- 命令后建议（💡）—— 029 §7.3 suggest.rs 已实现，不重复
- 所有命令的动态源 —— v1 只补高频（ssh/kubectl/env var）

### 033 插件生态(3-4 周) ✅ v1 完成（M0-M3）

| M | 内容 | 依赖 | 状态 |
|---|---|---|---|
| M0 | plugin.at manifest + parse | 无 | ✅ |
| M1 | 加载器(4 贡献类型) | 029 SmartCommand loader(可选) | ✅ |
| M2 | ash plugin CLI | M0+M1 | ✅ |
| M3 | 安全测试 + 作者文档 + 2 示例插件 | M2 | ✅ |

**v2 / 后续**（v1 有意不做，记录于此防遗忘；详见 `old/033-plugin-ecosystem.md` §非目标与 `designs/033-plugin-ecosystem.md` §6.4）：
- 动态库 / native 插件（需 ash-plugin-sdk + ABI 稳定）
- 中央 registry（类 crates.io）
- 插件签名 / 沙箱（v1 仅 capabilities 警告 + 复用 028 SecurityPolicy）
- `config.at` merge（v1 占位，声明但不合并）
- 热加载、插件依赖关系、插件市场网站

### 034 实例库(1-2 周,纯写作) ✅ 完成（M0/M1/M3 + M2 核心等价）

| M | 内容 | 依赖 | 状态 |
|---|---|---|---|
| M0 | 基础设施 + 速查表 | 无 | ✅ |
| M1 | 核心实例 17 个 | 无 | ✅ |
| M2 | 高级实例 13 个 | 029/031(展示能力) | 🟡 核心等价（bash 等价校验暂缓） |
| M3 | smoke 测试回归网 | M0 | ✅ |

**v2 / 后续**（v1 有意不做或暂缓，记录于此防遗忘；详见 `old/034-script-examples.md` §非目标/暂缓与 `designs/034-script-examples.md` §范围内/范围外）：
- M2 bash 等价校验 —— 待 system() 桥接 / find-grep 语法兼容修复落地后恢复
- auto-lang 的 `ext` parser bug —— 脚本侧绕过，不修（属 auto-lang 仓库）
- auto-lang 的 `.to_uint()` bug —— disk-clean 改用原生单位绕过（属 auto-lang 仓库）
- 完整应用级脚本、性能基准、完整迁移指南 —— 超出 v1 范围

### 035 文档+分发(1.5-2 周) ✅ 完成（M0-M3）

| M | 内容 | 依赖 | 状态 |
|---|---|---|---|
| M0 | README + quickstart + installation | 所有(引用) | ✅ |
| M1 | 三类入口 + 速查表 | 034(速查表共建) | ✅ |
| M2 | CI + release（sibling-clone 解路径依赖） | 无 | ✅ |
| M3 | cargo install 验证 + 安装脚本 | M2 | ✅ |

详见 `old/035-documentation-distribution.md`。M3 用 `install.sh`/`install.ps1`（克隆三个 sibling 仓库 + `cargo install --path`）解决 `cargo install --git` 单仓的路径依赖限制（Cargo 禁止 path+git，见 [Issue #8747](https://github.com/rust-lang/cargo/issues/8747)）。

**v2 / 后续**：`cargo install ash`（待 crates.io 发布 ash + sibling）、brew/winget/scoop、完整文档网站。

---

## 阶段 2 的 5 个动作项(写 plan 时必须处理)

1. **定义 `Capabilities` 类型**(ash-core)—— 029/033 共用。阶段 3 第一个跨方向任务。
2. **统一 NL→命令 能力**—— 029 定义共享 `ai_complete` + NL prompt,032 §4.2 引用。
3. **Shell 公开访问器只定义一次**(029 §2.3)—— 032 §3 引用,不重复定义。
4. **Atomic config parser helper**(可选)—— 029/033/021 实施时统一。
5. **031 M0 Stream bug 修复后,跑 028 回归测试**—— 确保信封数据改善不破坏。

---

## 可以并行的方向组

基于依赖图,以下方向组可以并行实施(无互相依赖):

| 并行组 | 方向 | 理由 |
|---|---|---|
| **A** | 031(数据处理) | 完全独立(只依赖现有 Atom/DSL) |
| **B** | 035(文档)M0-M1 | 纯写作,不依赖未完成功能 |
| **C** | 034(实例库)M0-M1 | 纯写作,展示现有能力 |
| **D** | 028 M4(跨平台测试) | 独立,质量守护 |

这 4 个可以**同时启动**,互不阻塞。

**不能并行的**(有依赖链):
- 029 的 M0→M1→M2→M3→M4 是串行链
- 032 的 M0 依赖 029 §2.3
- 033 的 M1 依赖 029 SmartCommand loader
- 030 的 M5 依赖 029 SmartCommand

---

## 下一步:阶段 4(讨论实施顺序)

本路线图为阶段 4 提供:
- 每个方向的里程碑 + 估算
- 依赖关系(什么能并行,什么必须串行)
- 动作项(跨方向统一点)
- 并行组(可同时启动的低耦合方向)

阶段 4 需要决定的:**给定有限资源(人/时间),先实施哪个方向?**
