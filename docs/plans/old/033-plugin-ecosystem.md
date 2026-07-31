# Plan 033: ASH 插件/扩展生态（data-only 目录包 + git 分发）— TDD 计划

> **日期**: 2026-07-31
> **分支**: `feat/033-plugin-ecosystem`
> **状态**: **✅ 完成（M0–M3 全部完成 + 复审修复），可归档**
> **来源设计**: [`designs/033-plugin-ecosystem.md`](../../designs/033-plugin-ecosystem.md)
> **预估**: M0-M3 约 3-4 周，~1800 行
> **回归基线（2026-07-31）**: ash-core 395 + auto-shell 817 = **~1212 全绿**
> **完成基线（2026-07-31）**: ash-core 396 + auto-shell 876（+59 测试）= **1272 全绿**

---

## 0. 设计 vs 现状偏差核实（2026-07-31）

设计文档写于 2026-07-21，此处对照当前代码修正：

| # | 设计原文 | 实际现状（已核实） | 影响 |
|---|---|---|---|
| 1 | SmartCommand 未实现，M1 smart 贡献"待 029 实现" | **029 已完成**：`smart_command/loader.rs:39` `load_all()` + `load_all_from(cwd, home)` | 风险解除，M1 可直接测 smart 贡献 |
| 2 | 补全 `load_dir` | ✅ `spec_tiers.rs:54` `load_dir(&Path)` 任意路径 | 与设计一致 |
| 3 | `Shell::source_file` 公开 | ✅ `shell.rs:2472` `pub fn source_file(&mut self, path)` | 与设计一致 |
| 4 | `auto_config::ash_dir()` | ✅ `auto_config.rs:164` | 与设计一致 |
| 5 | SmartCommand loader 需加 `extra_dirs` 参数 | 现状 `load_all()` / `load_all_from(cwd, home)`，无插件目录 | 需小改：加 `load_all_with_extra(extra_dirs)` 或扩展 `load_all_from` |

**结论**：设计可实施，唯一需适配的是 SmartCommand loader 加插件搜索路径。

### 实施记录（2026-07-31，M0–M2 完成）

实施中对照代码发现并处理的额外偏差：

| # | 计划假设 | 实际 | 处理 |
|---|---|---|---|
| A | 复用 `parse_smart_command` | 实为 `parse_at`（`smart_command/config.rs:66`），错误类型 `Result<_, String>` | manifest 解析沿用同款手写解析器，但用类型化 `PluginError` |
| B | 复用 028/029 的 `Capabilities` | **代码中不存在**，仅见于设计文档 | 在 `plugin/manifest.rs` 新建 `Capabilities` + `is_empty()` |
| C | `Shell::completion_provider_mut()` 接入补全 | **不存在**——provider 由 `ShellCompleter` 持有 | 补全贡献接入 `ShellCompleter::load_tier_specs`（现成 tier 扫描，天然契合） |
| D | SmartCommand 启动时加载 | **懒加载**——每次 `ash smart` 才 `load_all()` | 扩展为 `load_all_with_extra(extra_dirs)`，懒加载器自动拾取插件 `smart/` |

**交付物**：
- `ash/auto-shell/src/plugin/`（mod/manifest/loader/cli，~1800 行）
- `ash/auto-shell/tests/plugin_e2e.rs`（5 个集成测试）
- 接线：`lib.rs`（pub mod）、`main.rs`（`ash plugin` 分发）、`repl.rs`（启动加载）、`completions_reedline.rs`（补全第四层）、`smart_command/loader.rs`（`load_all_with_extra`，原签名保持兼容）
- `ash plugin install/list/show/enable/disable/remove/update` 全流程经手测验证（install --local → list/show → disable/enable → `smart list` 见插件命令 → remove）

---

## 1. 目标与范围

**愿景**：一个插件就是一个目录——含补全 spec、AutoLang 函数、SmartCommand、配置段。`ash plugin install <git-url>` 克隆到本地，ash 启动时自动加载。零编译、零动态库。

**范围**：插件包格式 + 加载器 + `ash plugin` CLI + git 分发。

**范围外**（明确不做）：动态库/native 插件（v2）、中央 registry（v2）、插件签名/沙箱（v2）、热加载、插件依赖关系。

### 测试约定
- **auto-shell**：内联 `#[cfg(test)] mod tests`（与 029/034 一致）+ `ash/auto-shell/tests/` 集成测试
- **TDD 纪律**：每个任务先写失败测试 → 实现 → 回归全绿
- **回归基线**：ash-core 395 + auto-shell 817 = ~1213

---

## M0：插件包格式 + manifest（1 周，~400 行）

### 任务 M0.1 — `plugin/manifest.rs`：PluginManifest + parse

- **先写失败测试**：manifest 解析测试（完整字段 + 缺省字段 + 错误处理 + 往返 serialize）
- **实现**：
  - `pub struct PluginManifest { name, version, author: Option, description: Option, contributions: PluginContributions, capabilities, min_ash_version: Option, enabled: bool }`
  - `pub struct PluginContributions { completions, functions, smart, config: bool }`
  - `pub enum PluginError { MissingName, MissingVersion, InvalidFormat(String), ... }`
  - `pub fn parse_plugin_manifest(content: &str) -> Result<PluginManifest, PluginError>`
- **复用**：照搬 029 `parse_smart_command` 的 Atomic DSL typed-parse 模式
- **落点**：`ash/auto-shell/src/plugin/`（新增目录）+ `mod.rs`

### 任务 M0.2 — `ash plugin show <name>` + `ash plugin list`

- **先写失败测试**：CLI 分发测试（list 输出、show 输出字段）
- **实现**：`plugin/cli.rs` 的 `dispatch` 骨架 + `cmd_list` + `cmd_show`（复用 M0.1 的 manifest 解析，从 `ash_dir()/plugins/<name>/plugin.at` 读取）
- **注意**：CLI 全功能在 M2 完成，此处只搭 list/show 骨架

**M0 验收**：manifest 解析 + 往返测试通过；`ash plugin list` 能列出 `~/.config/ash/plugins/` 下现有插件。

---

## M1：加载器（1 周，~500 行）

### 任务 M1.1 — `plugin/loader.rs`：load_all_plugins

- **先写失败测试**：构造临时插件目录（含 plugin.at），验证：
  - 无 manifest 的目录被跳过
  - `enabled: false` 的插件被跳过
  - `min_ash_version` 不满足被跳过 + 警告
  - 正常插件被加载进 report
- **实现**：`pub fn load_all_plugins(shell: &mut Shell) -> Result<PluginLoadReport>`
  - 扫 `ash_dir()/plugins/*/`，读 `plugin.at`，跳过非法/禁用/版本不符
  - 按 `contributions` 分发到 4 种加载器

### 任务 M1.2 — 4 种贡献类型加载

- **补全**：`load_plugin_completions` → 复用 `spec_tiers::load_dir` + 注册进 provider。优先级：built-in < cache < generated < user < **plugin**
- **函数**：`shell.source_file(&plugin_dir.join("functions.ash"))` 一行复用
- **SmartCommand**：扩展 `smart_command::loader` 加 `load_all_with_extra(extra_dirs)`，plugin loader 传入所有插件 `smart/` 目录
- **配置**：`merge_plugin_config` 简单覆盖同名 key（v1）

**测试**：构造含 4 种贡献的模拟插件目录，验证全部加载。SmartCommand 贡献直接可测（029 已实现）。

### 任务 M1.3 — 启动集成

- `Shell::new()` 之后、REPL 开始前调用 `load_all_plugins`
- 加载报告打印到 stderr（skipped/loaded/capability_warning）

**M1 验收**：模拟插件目录（含补全 + 函数 + SmartCommand），启动后补全可用 + 函数可调 + SmartCommand 可执行。

---

## M2：`ash plugin` CLI（1 周，~600 行）

### 任务 M2.1 — install（git clone）

- `cmd_install(url, --name)`：`git clone --depth 1` 到 `ash_dir()/plugins/<name>/`，验证 `plugin.at` 存在，失败时清理已克隆目录
- 本地路径安装：`ash plugin install ./my-plugin --local`（复制目录）
- **测试**：本地路径安装一个测试插件，验证目录复制 + manifest 存在

### 任务 M2.2 — enable / disable / remove / update

- `enable/disable`：改 `plugin.at` 的 `enabled` 字段（读写 manifest）
- `remove`：删目录（`ash plugin remove <name>`，确认提示）
- `update`：`git pull`；`--all` 遍历所有 git 插件
- **测试**：enable/disable 往返（改文件再解析）；remove 删目录

### 任务 M2.3 — 端到端

- `ash plugin install ./test-plugin --local` → 重启（或手动 load）→ 插件功能可用

**M2 验收**：`ash plugin install` 克隆成功，重启后加载；enable/disable/remove/update 全流程。

---

## M3：安全 + 文档（0.5-1 周，~300 行，可选但推荐）✅ 完成

- ✅ capabilities 声明 + 加载警告（`PluginLoadReport.print_to_stderr`/`render` 打印声明的能力，v1 不强制确认）— 测试覆盖 `render` 全分类
- ✅ `docs/plugin-development.md`（作者文档：目录结构、manifest 字段、4 种贡献类型、安装发布、安全模型、2 个示例链接）
- ✅ 2+ 示例插件：`examples/plugins/git-extras`（补全增强）+ `examples/plugins/deploy-pack`（SmartCommand + 函数，子目录 smart 布局）— 均经 `ash plugin install` 手测可装可跑
- ✅ **安全验证测试**：`tests/plugin_e2e.rs` 三个测试验证插件加载进 shell 后，`--read-only` 拦截写、`--no-exec` 拦截外部进程、restrictive policy 下插件仍可 source（复用 028 SecurityPolicy，无新机制）

**M3 验收**：作者文档完整；2+ 示例插件可安装；安全拦截测试通过。**全部达成。**

---

## 依赖与顺序

```
M0.1 manifest ─→ M0.2 list/show ─→ M1.1 loader ─→ M1.2 4种贡献 ─→ M1.3 启动集成
                                          │
M2.1 install ─→ M2.2 enable/disable/remove/update ─→ M2.3 端到端
M3 安全+文档（依赖 M1 加载器 + M2 install）
```

M0 与 M2.1（install）无强依赖，可部分并行；M1.2 的 SmartCommand 贡献依赖 029（已完成）。

## 与其他 Plan 的接触点

| 接触点 | 守护方式 |
|---|---|
| Plan 021 补全 spec（三层目录） | 插件补全加为**第四层**，不改现有三层语义；回归 `completions_cmd.rs`/`completion_runtime.rs` |
| Plan 029 SmartCommand | `smart_command::loader::load_all` 加 `load_all_with_extra`，保持原有调用兼容；回归 smart_command/* 43 测试 |
| Plan 028 SecurityPolicy | M3 安全拦截复用 028 沙箱；不新造安全机制 |
| Plan 032 智能补全 | 插件可贡献动态补全源（ssh hosts 等），M3 示例插件展示 |

## 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 恶意插件执行任意代码 | 高 | 高 | v1 仅警告 + SecurityPolicy 约束 system()；v2 沙箱/签名 |
| 插件破坏主 config | 中 | 中 | v1 简单覆盖；v2 namespace |
| Windows 无 git | 低 | 中 | 依赖系统 git，缺失时报清晰错误 |
| load_all 签名改动破坏 029 调用 | 低 | 中 | 加新方法不删旧的，回归 smart_command 测试 |

## 成功指标

1. **M0**：`parse_plugin_manifest` 解析 + 往返测试通过
2. **M1**：模拟插件目录（补全 + 函数 + SmartCommand），启动后全部可用
3. **M2**：`ash plugin install` 克隆/复制成功，重启后加载；全 CLI 命令可测
4. **M3**：作者文档完整；2+ 示例插件；恶意 system() 被 SecurityPolicy 拦截
5. **回归**：~1213 测试全绿 + 新增插件测试 ≥ 30
