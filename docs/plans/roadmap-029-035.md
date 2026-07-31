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
| **029** | AI 能力增强(含 SmartCommand) | ✅ 1064 行（v1 已过时，§0 已重评） | 🟡 已有（核心交付物完成，余 P0-P2） | auto-ai 侧 ✅ + auto-shell 侧 🟡（核心 ~2700 行已落地，余 ~4 周补缺口） |
| **030** | ash-gui(Shell-native UI) | ✅ 1048 行 | ✅ 1671 行(M0-M2) | 13-20 周(M0-M5) |
| **031** | 数据处理框架(lazy pipeline) | ✅ 538 行 | ✅ 已归档（M0-M3 完成） | ✅ 完成 |
| **032** | 智能补全(AI 层) | ✅ 405 行 | ❌ 待写 | 4-5 周 |
| **033** | 插件生态(data-only) | ✅ 437 行 | ❌ 待写 | 3-4 周 |
| **034** | 脚本实例库 | ✅ 194 行 | ✅ 已实施并归档(M0/M1/M3 + M2 核心等价,见 `old/034-script-examples.md`) | ✅ 完成 |
| **035** | 文档+分发 | ✅ 240 行 | ❌ 待写 | 1.5-2 周 |

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

### 029 AI 能力增强(最大,12-16 周)

| M | 内容 | 依赖 | 估算 |
|---|---|---|---|
| M0 | 共享基础设施(OllamaProvider + 桥 + 上下文) | auto-ai 改造 | 3-4 周 |
| M1 | SmartCommand 完整 + git.finish-worktree | M0 | 3-4 周 |
| M2 | F4 tool-calling(ChatSession → Agent::run) | M0 | 2-3 周 |
| M3 | F3 增强 + NL→AutoLang | M2 | 2-3 周 |
| M4 | 上下文感知 + Warp 式建议 | M0 | 2 周 |

### 030 ash-gui(已有详细 plan)

| M | 内容 | 估算 |
|---|---|---|
| M0 | auto-shell feature 隔离 | 1 周 |
| M1 | Renderer trait + RenderedOutput | 2-3 周 |
| M2 | 最小 GUI(**关键检查点**) | 2-3 周 |
| M3-M5 | 日常可用/全 AtomType/AI 面板 | 8-12 周 |

### 031 数据处理(独立性强,4-6 周)

| M | 内容 | 估算 |
|---|---|---|
| M0 | Stream bug 修复 + Format trait | 1-2 周 |
| M1 | LazyNode + 基础算子 | 2 周 |
| M2 | 谓词下推 + shell.rs 集成 | 1-2 周 |
| M3 | ExternalStream → lazy(可选) | 1 周 |

### 032 智能补全(4-5 周)

| M | 内容 | 依赖 |
|---|---|---|
| M0 | 上下文 plumbing | 029 §2.3 访问器 |
| M1 | 排序 + 历史 ghost-text | M0 |
| M2 | AI 补全(LLM/NL) | 029 NL 共享层 |
| M3 | 缺失动态源(可选) | 无 |

### 033 插件生态(3-4 周)

| M | 内容 | 依赖 |
|---|---|---|
| M0 | plugin.at manifest + parse | 无 |
| M1 | 加载器(4 贡献类型) | 029 SmartCommand loader(可选) |
| M2 | ash plugin CLI | M0+M1 |
| M3 | 安全 + 文档 | M2 |

### 034 实例库(1-2 周,纯写作)

| M | 内容 | 依赖 |
|---|---|---|
| M0 | 基础设施 + 速查表 | 无 |
| M1 | 核心实例 17 个 | 无 |
| M2 | 高级实例 13 个 | 029/031(展示能力) |

### 035 文档+分发(1.5-2 周)

| M | 内容 | 依赖 |
|---|---|---|
| M0 | README + quickstart | 所有(引用) |
| M1 | 三类入口 + 速查表 | 034(速查表共建) |
| M2 | CI + release | 无 |
| M3 | cargo install 验证 | M2 |

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
