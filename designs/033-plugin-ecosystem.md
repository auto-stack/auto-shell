# Plan 033: ASH 插件/扩展生态设计(data-only 目录包 + git 分发)

> **日期**: 2026-07-21
> **状态**: 设计中(待评审)
> **战略驱动**: 让第三方能扩展 ash(补全/函数/SmartCommand/配置),无需动态库,通过目录包 + git clone 分发,建立 ash 的扩展生态护城河
> **范围**: 插件包格式 + 加载器 + `ash plugin` CLI + git 分发
> **预估**: M0-M3 共约 3-4 周(详见 §6)
> **形态**: v1 = data-only 目录包(无动态库),v2 预留 native 接口

---

## 愿景

> **一个插件就是一个目录**——含补全 spec、AutoLang 函数、SmartCommand、配置段。`ash plugin install <git-url>` 克隆到本地,ash 启动时自动加载。零编译、零动态库、零 ABI 问题。SmartCommand 是插件的一种内容类型。

### 核心洞察:探勘揭示的可行性

探勘证实 ash 的 4 种扩展贡献方式中,**3 种已有现成 hook**:

| 贡献 | 现状 hook | 插件难度 |
|---|---|---|
| 补全 spec | `spec_tiers::load_dir(&Path)` 接受任意路径 | 微不足道 |
| AutoLang 函数 | `Shell::source_file(any_path)` 公开,`.ashrc` 已示范 | 微不足道 |
| SmartCommand | 029 设计的 `load_all()` 搜索路径列表 | 低(功能本身未建) |
| 原生 Command | `Shell` 无 `register_command`,binary crate 耦合 | **中高(v1 不做)** |

**所以 v1 是 data-only 目录包**——全部通过扩展现有 dir-scan + source_file 加载,零动态库。

### 三个核心决策(已在 brainstorming 阶段确认)

1. **v1 = data-only 目录包**(补全/函数/SmartCommand/配置),不做动态库。原生 Command 留 v2。
2. **分发用 git clone**:`ash plugin install <git-url>` 克隆到 `~/.config/ash/plugins/<name>/`。无中央 registry(v1)。
3. **SmartCommand 是插件的一种内容类型**,不是独立于插件的东西。插件可以含 0 到 N 个 SmartCommand。

### 范围内 / 范围外

| 范畴 | 本 Plan 包含 | 不包含 |
|---|---|---|
| **插件包格式** | 目录结构 + `plugin.at` manifest | 动态库/编译插件 |
| **贡献类型** | 补全 spec + AutoLang 函数 + SmartCommand + 配置段 | 原生 Command(留 v2) |
| **加载器** | 启动时扫 `~/.config/ash/plugins/*/`,扩展 4 个现有 hook | 运行时热加载(留后续) |
| **CLI** | `ash plugin install/list/enable/disable/remove/update` | 插件市场网站 |
| **分发** | git clone(公开/私有 repo) | 中央 registry(留 v2) |
| **安全** | manifest 声明 capabilities,SecurityPolicy 约束 | 签名/沙箱(留后续) |

---

## 第 1 节:子能力总览(给阶段 2 横向检查用)

| 子能力 | 主要消费者 | 依赖 | 跟其他方向的接触点 |
|---|---|---|---|
| **插件包格式** | 插件作者 | 029 SmartCommand 格式 + 021 补全 spec 格式 | 跟 029 的 `command.at` 共享 Atomic DSL;跟 021 的 `.at` spec 共享格式 |
| **补全 spec 贡献** | 用户(补全) | Plan 021 `load_dir` | 跟 032 补全的动态源协同(插件可贡献 ssh hosts 等动态源) |
| **函数贡献** | 用户(自定义函数) | `source_file` + AutoLang | 跟 029 NL→AutoLang 接触:插件函数是 AI 可调用的 |
| **SmartCommand 贡献** | 用户/Agent | 029 SmartCommand loader | SmartCommand 是插件内容类型之一 |
| **git 分发** | 用户(install) | git CLI | 跟方向 A(文档+分发)接触:插件的 README |

**阶段 2 要检查的接触点**:
1. **plugin.at manifest 跟 SmartCommand 的 command.at 格式统一** —— 两者都是 Atomic DSL,阶段 2 检查是否共用 parser
2. **插件贡献的函数 vs 029 NL→AutoLang** —— AI 生成的脚本能否自动打包成插件?长期融合点

---

## 第 2 节:现状(探勘确认)

### 2.1 已有的扩展 hook(3 个 trivial,1 个 hard)

**补全 spec**:`spec_tiers::load_dir(dir: &Path)` 接受任意路径,返回 `Vec<CompletionSpec>`。三层目录(user/generated/cache)的 `load_tier_specs` 就是调它。插件加一层目录即可。

**AutoLang 函数**:`Shell::source_file(path: &Path)` 公开,执行脚本内容(含 `fn` 定义)。`~/.ashrc` 就是用它加载的。`.ashrc` 模板甚至写了"source ~/.config/ash/work.at"的示例。插件 source 一个 `.ash` 文件即可。

**SmartCommand**(029 设计):`load_all()` 搜索路径是列表(`$CWD/smart/ > ~/.config/ash/smart/ > built-in`)。插件加一层目录即可。但 SmartCommand 本身未实现。

**原生 Command**:`Shell` 没有 `register_command`,`registry()` 只读。Command trait 的 `run(&self, &mut Shell)` 需要 Shell 访问,且 `auto-shell` 是 binary crate,插件 crate 无法依赖它。**v1 不做。**

### 2.2 零现有插件机制

grep `plugin|dlopen|libloading|wasm|linkme|inventory` 全零命中。唯一提及是 `docs/plans/017` 的 `[plugins]` 占位注释("未来插件系统")。

### 2.3 安装现状(影响分发)

ash 只能 build from source(无 crates.io,无 release 二进制)。路径依赖 sibling repos(`../../../auto-lang/`)。所以:
- data-only 插件可行(下载目录包到 `~/.config/ash/plugins/`)
- compiled 插件 crate 不可行(需要先发布 ash-core/sdk 到 crates.io,v2)

---

## 第 3 节:插件包格式

### 3.1 目录结构

```
~/.config/ash/plugins/<plugin-name>/
├── plugin.at              # manifest(必需)
├── completions/           # 补全 spec(可选)
│   ├── git.at
│   └── docker.at
├── functions.ash          # AutoLang 函数(可选)
├── smart/                 # SmartCommand(可选,029 格式)
│   └── my-deploy/
│       ├── command.at
│       ├── body.ash
│       └── skill.md
├── config.at              # 配置段(可选;**v1 占位,声明但暂不 merge**)
└── README.md              # 给人读(可选)
```

### 3.2 `plugin.at` manifest

```autolang
plugin {
    name        : "my-git-extras"
    version     : "0.1.0"
    author      : "zhaopuming"
    description : "Extra git SmartCommands and completion enhancements"
    homepage    : "https://github.com/zhaopuming/ash-git-extras"

    // 声明插件贡献的内容(加载器据此决定加载什么)
    contributions : {
        completions : true      // 扫 completions/*.at
        functions   : true      // source functions.ash
        smart       : true      // 扫 smart/*/
        config      : false     // 不贡献配置段
    }

    // 安全声明(加载时展示给用户确认)
    capabilities : {
        reads_fs       : true
        writes_fs      : true
        spawns_process : true
        uses_network   : false
    }

    // 兼容性
    min_ash_version : "0.5.0"

    // 启用状态(ash plugin enable/disable 改这个)
    enabled : true
}
```

### 3.3 manifest 解析

照搬 029 的 `parse_smart_command` 模式(Atomic DSL typed parse):

```rust
// ash/auto-shell/src/plugin/manifest.rs(新增)
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub contributions: PluginContributions,
    pub capabilities: Capabilities,  // 复用 028/029 的 Capabilities
    pub min_ash_version: Option<String>,
    pub enabled: bool,
}

pub struct PluginContributions {
    pub completions: bool,
    pub functions: bool,
    pub smart: bool,
    pub config: bool,
}

pub fn parse_plugin_manifest(content: &str) -> Result<PluginManifest, PluginError>;
```

---

## 第 4 节:加载器

### 4.1 启动时加载流程

```rust
// ash/auto-shell/src/plugin/loader.rs(新增)

/// 扫 ~/.config/ash/plugins/*/,加载所有 enabled 插件。
/// 在 Shell::new() 之后、REPL 开始前调用。
pub fn load_all_plugins(shell: &mut Shell) -> Result<PluginLoadReport> {
    let plugins_dir = ash_dir().join("plugins");
    let mut report = PluginLoadReport::new();

    for entry in std::fs::read_dir(&plugins_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() { continue; }
        let plugin_dir = entry.path();
        let manifest_path = plugin_dir.join("plugin.at");

        let manifest = match std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| parse_plugin_manifest(&s).ok())
        {
            Some(m) => m,
            None => { report.skipped(plugin_dir, "invalid manifest"); continue; }
        };

        if !manifest.enabled {
            report.disabled(&manifest.name);
            continue;
        }

        // 版本检查
        if let Some(min) = &manifest.min_ash_version {
            if !ash_version_meets(min) {
                report.skipped(&manifest.name, "version too old");
                continue;
            }
        }

        // 安全确认(首次加载)
        if !manifest.capabilities.is_empty() {
            // v1:打印警告,用户可在 config 关插件
            report.capability_warning(&manifest.name, &manifest.capabilities);
        }

        // 按贡献类型加载
        if manifest.contributions.completions {
            load_plugin_completions(shell, &plugin_dir.join("completions"));
        }
        if manifest.contributions.functions {
            let funcs = plugin_dir.join("functions.ash");
            if funcs.exists() {
                shell.source_file(&funcs)?;  // 复用现有 source_file
            }
        }
        if manifest.contributions.smart {
            load_plugin_smart_commands(&plugin_dir.join("smart"));
            // 委托给 029 的 SmartCommand loader(加一层搜索路径)
        }
        if manifest.contributions.config {
            merge_plugin_config(shell, &plugin_dir.join("config.at"));
        }

        report.loaded(&manifest.name);
    }

    Ok(report)
}
```

### 4.2 各贡献类型的加载

**补全**:扩展现有 `load_tier_specs`,加一层"plugin tier":

```rust
fn load_plugin_completions(shell: &Shell, dir: &Path) {
    // 复用 spec_tiers::load_dir
    for spec in spec_tiers::load_dir(dir) {
        shell.completion_provider_mut().register(spec);
    }
}
```

优先级:built-in < cache < generated < user < **plugin**(插件补全优先级最高,因为用户主动装了它)。

**函数**:`shell.source_file(&funcs)` —— 一行,复用现有。

**SmartCommand**:扩展 029 的 `load_all()`,加插件目录到搜索路径:
```rust
// 029 的 load_all 改为接受额外搜索路径
pub fn load_all(extra_dirs: Vec<PathBuf>) -> Result<Vec<SmartCommandSpec>>;
// plugin loader 传入所有插件的 smart/ 目录
```

**配置**:merge 插件 config.at 到主 config。需要 `auto_config` 加 merge 能力(v1 简单:覆盖同名 key)。

---

## 第 5 节:`ash plugin` CLI

```bash
# 安装(git clone)
ash plugin install <git-url> [--name <custom-name>]
# 例:ash plugin install https://github.com/zhaopuming/ash-git-extras

# 列出已安装
ash plugin list
ash plugin list --enabled
ash plugin list --format json

# 启用/禁用(改 plugin.at 的 enabled 字段)
ash plugin enable <name>
ash plugin disable <name>

# 更新(git pull)
ash plugin update <name>
ash plugin update --all

# 卸载(删目录)
ash plugin remove <name>

# 查看详情
ash plugin show <name>

# 从本地路径安装(开发插件时)
ash plugin install ./my-plugin --local
```

### CLI 实现

```rust
// ash/auto-shell/src/plugin/cli.rs
pub fn dispatch(args: &[String]) -> i32 {
    match args.get(0).map(|s| s.as_str()) {
        Some("install") => cmd_install(&args[1..]),
        Some("list") => cmd_list(&args[1..]),
        Some("enable") => cmd_enable(&args[1..]),
        Some("disable") => cmd_disable(&args[1..]),
        Some("update") => cmd_update(&args[1..]),
        Some("remove") => cmd_remove(&args[1..]),
        Some("show") => cmd_show(&args[1..]),
        _ => { eprintln!("usage: ash plugin <install|list|enable|disable|update|remove|show>"); 2 }
    }
}

fn cmd_install(args: &[String]) -> i32 {
    let url = match args.get(0) { Some(u) => u, None => { eprintln!("usage: ash plugin install <git-url>"); return 2; } };
    let plugins_dir = ash_dir().unwrap().join("plugins");
    std::fs::create_dir_all(&plugins_dir)?;
    // git clone <url> <plugins_dir>/<name>
    let name = derive_name_from_url(url);  // 或 --name 指定
    let target = plugins_dir.join(&name);
    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", url, &target.to_string_lossy()])
        .status()?;
    if !status.success() { eprintln!("git clone failed"); return 1; }
    // 验证 plugin.at 存在
    if !target.join("plugin.at").exists() {
        eprintln!("warning: no plugin.at manifest found");
    }
    println!("✓ installed {} to {}", name, target.display());
    0
}
```

---

## 第 6 节:里程碑、风险、非目标

### 6.1 里程碑

#### M0:插件包格式 + manifest(1 周)
- `plugin/manifest.rs`:PluginManifest + parse_plugin_manifest
- 目录结构约定
- `ash plugin show <name>` + `ash plugin list`
- 测试:manifest 解析 + 往返

#### M1:加载器(1 周)
- `plugin/loader.rs`:load_all_plugins
- 4 种贡献类型加载(补全/函数/SmartCommand/config)
- 启动时集成(Shell::new 后)
- 测试:模拟插件目录,验证 4 种贡献加载

#### M2:`ash plugin` CLI(1 周)
- install(git clone)/list/enable/disable/update/remove/show
- 本地路径安装
- 测试:install 一个测试插件,验证加载

#### M3:安全 + 文档(0.5-1 周,可选)
- capabilities 声明 + 首次加载警告
- 插件作者文档(`docs/plugin-development.md`)
- 几个示例插件

### 6.2 工作量

| 里程碑 | 代码行 | 估算 |
|---|---|---|
| M0 格式+manifest | ~400 | 1 周 |
| M1 加载器 | ~500 | 1 周 |
| M2 CLI | ~600 | 1 周 |
| M3 安全+文档 | ~300 | 0.5-1 周 |
| **总计** | **~1800** | **3-4 周** |

### 6.3 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| **恶意插件**(source_file 执行任意代码) | 高 | 高 | v1 仅警告;v2 加沙箱/签名;插件函数受 SecurityPolicy 约束(system() 调用被拦) |
| **插件破坏主 config**(merge 冲突) | 中 | 中 | v1 简单覆盖;v2 加 namespace(插件配置段独立) |
| **SmartCommand 未实现**(M1 的 smart 贡献不可测) | 确定 | 中 | M1 smart 贡献标"待 029 实现";M1 只测补全+函数 |
| **git clone 跨平台**(Windows 无 git?) | 低 | 中 | 依赖系统 git;无 git 时报清晰错误 |
| **插件版本不兼容**(min_ash_version) | 中 | 低 | 版本检查;不满足跳过+警告 |

### 6.4 非目标

- ❌ **动态库/native 插件** —— v2(需 ash-plugin-sdk + ABI 稳定)
- ❌ **中央 registry** —— v2(类似 crates.io)
- ❌ **插件签名/沙箱** —— v2(安全增强)
- ❌ **热加载** —— 后续(启动加载已满足)
- ❌ **插件依赖关系**(插件 A 依赖插件 B) —— 后续
- ❌ **插件市场网站** —— 远期

### 6.5 成功指标

1. **M0**:`parse_plugin_manifest` 解析 + 往返测试通过
2. **M1**:模拟插件目录(含补全+函数),启动后补全可用 + 函数可调
3. **M2**:`ash plugin install <test-repo>` 克隆成功,重启后加载
4. **M3**:作者文档完整,有 2+ 示例插件
5. **安全**:恶意插件的 system() 调用被 SecurityPolicy 拦(--read-only/--sandbox 模式)

### 6.6 跟其他方向的关系

| 方向 | 关系 |
|---|---|
| **Plan 029**(SmartCommand) | SmartCommand 是插件内容类型之一;插件加载器委托 029 的 loader |
| **Plan 021**(补全) | 插件贡献补全 spec,扩展三层目录为四层(+plugin tier) |
| **Plan 032**(智能补全) | 插件贡献的动态源(ssh hosts 等)跟 032 协同 |
| **方向 A**(文档+分发) | 插件 README;ash 主 README 提及插件机制 |
| **方向 #3**(实例库) | 实例可打包成插件分发 |

---

## 附录 A:实施前置勘探记录(2026-07-21)

### A.1 关键发现

1. **4 种贡献 hook**:补全(`load_dir` 任意路径)+ 函数(`source_file` 公开)+ SmartCommand(029 搜索路径)+ config(需加 merge)。3 个 trivial,1 个 medium。
2. **原生 Command 是硬骨头**:Shell 无 register_command,binary crate 耦合。v1 不做。
3. **零现有插件机制**:全 grep 零命中。
4. **安装现状**:build from source,无 crates.io。data-only 插件可行,compiled 不可行。

### A.2 关键文件路径

- `ash/auto-shell/src/cmd/registry.rs` —— CommandRegistry(register 只在 Shell::new)
- `ash/auto-shell/src/shell.rs:1253` —— registry() 只读;`source_file` 公开(line 2237)
- `ash/auto-shell/src/completions/spec_tiers.rs:54` —— load_dir 任意路径
- `ash/auto-shell/src/auto_config.rs:160` —— ash_dir() + config 加载(固定路径)
- `ash/auto-shell/src/frontend/repl.rs:46-62` —— .ashrc 加载(插件函数的范例)

---

## 参考

- `designs/029-ai-capabilities.md` —— SmartCommand 是插件内容类型;command.at 格式共用
- `docs/plans/021-ash-arbitrary-command-completion.md` —— 补全 spec 格式 + 三层目录(扩展为四层)
- `designs/032-intelligent-completion.md` —— 插件贡献动态源
