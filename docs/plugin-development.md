# ASH 插件开发指南

> 给想给 ash 写插件的开发者。一个插件就是一个**目录**——含补全 spec、AutoLang 函数、SmartCommand、配置段。零编译、零动态库。
>
> 这是 Plan 033 的 v1（data-only 目录包 + git 分发）。设计细节见 [`designs/033-plugin-ecosystem.md`](../designs/033-plugin-ecosystem.md)。

## 1. 一个插件长什么样

一个插件就是一个目录，根上有一个 `plugin.at` 清单，加上你想贡献的内容文件：

```
my-plugin/
├── plugin.at              # 清单（必需）
├── completions/           # 补全 spec（可选）
│   └── git.at
├── functions.ash          # AutoLang 函数（可选）
├── smart/                 # SmartCommand（可选，每个命令一个子目录）
│   └── my-deploy/
│       ├── command.at
│       └── deploy.ash
├── config.at              # 配置段（可选；v1 占位，声明但暂不 merge）
└── README.md              # 给人读（可选）
```

装好之后它会被放进 `~/.config/ash/plugins/<插件名>/`，ash 启动时自动加载。

> 💡 **最简插件**：只要一个 `plugin.at`。其它都可选。完整示例见 `examples/plugins/`。

## 2. `plugin.at` 清单

清单用 ash 的 `.at` 格式（Atomic DSL）。**`name` 和 `version` 必填**，其它都有缺省值：

```autolang
plugin {
    name        : "my-git-extras"
    version     : "0.1.0"
    author      : "你的名字"
    description : "一句话描述这个插件干什么"
    homepage    : "https://github.com/你/my-git-extras"

    // 声明贡献哪些内容（加载器据此决定加载什么）
    contributions : {
        completions : true      // 扫 completions/*.at
        functions   : true      // source functions.ash
        smart       : true      // 扫 smart/*/
        config      : false     // 不贡献配置段
    }

    // 安全声明（首次加载时展示给用户，v1 仅警告不强制确认）
    capabilities : {
        reads_fs       : true
        writes_fs      : true
        spawns_process : true
        uses_network   : false
    }

    // 兼容性（缺省 = 任意版本都行）
    min_ash_version : "0.1.0"

    // 启用状态（`ash plugin enable/disable` 改这个）
    enabled : true
}
```

### 字段说明

| 字段 | 必填 | 说明 |
|---|---|---|
| `name` | ✅ | 插件名（也作为安装后的目录名身份） |
| `version` | ✅ | 语义化版本 |
| `author` / `description` / `homepage` | ❌ | 元信息，`ash plugin list/show` 展示 |
| `contributions.*` | ❌（全 false） | 声明四类贡献各是否启用 |
| `capabilities.*` | ❌（全 false） | 声明能力，加载时警告用户 |
| `min_ash_version` | ❌ | 要求的最低 ash 版本，不满足则跳过 |
| `enabled` | ❌（true） | 启用状态 |

> ⚠️ **格式约束**：清单的嵌套块（`contributions {}` / `capabilities {}`）**每行只写一个字段**。不要写成一行逗号分隔（`{ a : true, b : true }` 会被拒绝）。

## 3. 四种贡献类型

### 3.1 补全 spec（`completions/*.at`）

放 `.at` 补全规范文件，每个文件描述一个命令的子命令/flag/参数。格式同 Plan 315 的补全 spec：

```autolang
spec {
    command : "mytool"
    desc    : "My custom tool"
    subcommands : [
        sub { name : "build",  desc : "构建",  subcommands : [] }
        sub { name : "deploy", desc : "部署",  subcommands : [] }
    ]
}
```

插件补全是**第四层**，优先级最高：built-in < cache < generated < user < **plugin**（你主动装的插件，覆盖前面所有层）。

### 3.2 函数（`functions.ash`）

一个普通的 AutoLang 脚本，定义 `fn`。加载时被 `source` 进会话，prompt 下可直接调用：

```autolang
fn deploy_msg(env) {
    return "准备部署到 " + env
}
```

### 3.3 SmartCommand（`smart/<cmd>/command.at`）

每个 SmartCommand 一个子目录，含 `command.at` 清单 + body 脚本（**子目录布局**，与 `examples/finish-worktree/` 一致）。详见 Plan 029：

```autolang
command "my-deploy" {
    description : "打包并部署到目标环境"
    args        : ["target"]
    body        : "deploy.ash"
}
```

body 脚本通过 `system("echo $1")` 取位置参数。装好后用 `ash smart run my-deploy prod` 调用。插件 SmartCommand 的搜索路径在项目本地和用户全局之后，所以同名时前者优先。

### 3.4 配置段（`config.at`）—— v1 占位

v1 **声明但暂不 merge** 进主配置（`ash plugin show` 会标注 "not merged in v1"）。留作后续实现。

## 4. 安装、发布、管理

```bash
# 从 git 仓库安装（推荐分发方式）
ash plugin install https://github.com/你/my-plugin

# 从本地目录安装（开发时）
ash plugin install --local ./my-plugin [--name 自定义名]

# 列出 / 查看 / 启用 / 禁用 / 更新 / 卸载
ash plugin list [--enabled]
ash plugin show <name>
ash plugin enable <name>
ash plugin disable <name>
ash plugin update <name> | --all
ash plugin remove <name>
```

**发布**：把插件目录推到一个 git 仓库即可。用户 `ash plugin install <url>` 会 `git clone --depth 1` 下来。v1 没有中央 registry。

## 5. 安全模型（v1）

- 插件函数通过 ash 的 shell 执行，**受当前 SecurityPolicy 约束**。`--read-only` / `--no-exec` / `--sandbox` 模式下，插件里 `system()` 想做的写文件、起进程会被拦截（复用 Plan 028 沙箱）。
- 插件声明的 `capabilities` 在加载时打印**警告**给用户，但 v1 **不强制确认**（v2 会加签名/沙箱）。
- 所以：**只装你信任的来源的插件**。

## 6. 完整示例

仓库里有两个现成示例，可直接装来跑：

| 插件 | 贡献 | 看哪 |
|---|---|---|
| [`examples/plugins/git-extras`](../examples/plugins/git-extras) | 补全增强 | 最小插件：`plugin.at` + 一个补全 spec |
| [`examples/plugins/deploy-pack`](../examples/plugins/deploy-pack) | SmartCommand + 函数 | 子目录 smart 布局 + `functions.ash` |

```bash
ash plugin install --local ./examples/plugins/git-extras
ash plugin install --local ./examples/plugins/deploy-pack
ash smart list   # 看到 deploy.run
```

## 7. 不在 v1 范围内

以下留作后续：动态库/native 插件、中央 registry、插件签名/沙箱、热加载、插件依赖关系、`config.at` merge。
