# ASH for Developers（终端用户指南）

> 给把 ash 当日常 shell 用的开发者。假设你已读过 [快速上手](quickstart.md)。

## REPL 交互

### 输入模式（F1-F4）

| 键 | 模式 | 提示符 | 用途 |
|----|------|--------|------|
| **F1** | Shell | `>` | 标准 shell（跟 bash 一样，默认） |
| **F2** | AutoScript | `#` | 写 AutoLang 表达式/脚本 |
| **F3** | AI 翻译 | `?` | 自然语言 → 一条命令（一次性） |
| **F4** | AI 对话 | `▌?` | 多轮 AI chat |

模式自动检测：输入像 shell 命令走 Shell 模式，像 AutoLang 表达式走 AutoScript 模式。F1-F4 是手动锁定。

### 补全

- **Tab**：命令/flag/路径补全（Plan 021 的三层 spec + help-probe）
- **Ctrl+R**：历史搜索（fzf 风格）
- **Ctrl+F**：接受灰色 ghost-text（历史前缀建议）
- **Ctrl+→**：接受 ghost-text 的下一个词

### 其他快捷键

| 键 | 行为 |
|----|------|
| Ctrl+E | 用外部编辑器编辑当前输入 |
| Ctrl+L | 清屏 |
| Ctrl+D | 退出（空行时） |
| Esc | 退出 AI 模式 / 取消 |

## 结构化 Pipeline DSL

这是 ash 的核心差异化。命令输出是**带语义类型的结构化数据**，pipeline 算子按字段名操作：

```bash
# filter —— 按字段过滤
> ls | filter .size > 10.mb
> ps | filter .cpu > 1.0
> cat data.json | from_json | filter .age > 30

# sort —— 按字段排序
> ls | sort .size              # 升序
> ls | sort .size descending   # 降序

# select —— 选列
> ls | select name size type
> ps | select pid name cpu

# group-by + 聚合
> ps | group-by .user | sum .cpu
> ls | group-by .type | count

# take / skip
> ls | sort .size | take 5
> ls | skip 10 | take 5        # 分页

# 链式组合
> ls | filter .type == "file" | sort .size descending | select name size | take 10
```

### 字段引用语法

| 语法 | 含义 |
|------|------|
| `.field` | 字段名 |
| `>`, `<`, `>=`, `<=`, `==`, `!=` | 比较运算符 |
| `contains`, `starts-with`, `ends-with` | 字符串匹配 |
| `10.mb`, `1.gb`, `500.kb` | 带单位的数字（字节） |

## 数据格式转换

ash 内置 5 种格式互转，不需要 jq/python：

```bash
# JSON ↔ CSV
> cat users.json | from_json | to_csv > users.csv
> cat data.csv | from_csv | to_json --pretty

# YAML ↔ JSON
> cat config.yaml | from_yaml | to_json

# TOML（Cargo.toml！）
> cat Cargo.toml | from_toml | get package.dependencies

# XML
> cat pom.xml | from_xml | select artifactId version
```

## AutoLang 脚本

按 F2 进 AutoScript 模式，或写 `.ash` 文件。AutoLang 比 bash 强大得多：

```bash
# 变量与类型
var count = 0
var name = "ash"
var items = [1, 2, 3]

# 条件
if count > 0 {
    print("positive")
}

# 循环
for item in items {
    print(item)
}

# 函数
fn greet(name) {
    return "hello, " + name
}

# try/catch
try {
    system("cargo build")
} catch(e) {
    print("build failed: " + e)
    exit(1)
}

# 调 shell
var files = system("ls")
var result = system("grep TODO *.rs")
```

### Shell bridge natives

AutoLang 脚本里可调 shell 能力：

| Native | 作用 |
|--------|------|
| `system(cmd)` | 执行 shell 命令，返回 stdout |
| `export(key, val)` | 设置环境变量 |
| `exit(code)` | 退出 |
| `print(...)` | 打印 |

→ 更多实例见 [实例库](../examples/)

## 配置

### `~/.ashrc`（启动脚本，类似 .bashrc）

首次启动自动创建，含示例函数。编辑它定义自己的函数/别名：

```bash
# ~/.ashrc
fn gs() {
    system("git status")
}

fn mkcd(dir) {
    system("mkdir -p " + dir)
    system("cd " + dir)
}
```

### `~/.config/ash/config.at`（主配置，Atomic DSL）

```autolang
shell {
    history_size             : 10000
    autosuggestion           : true
    autosuggestion_min_chars : 1
    edit_mode                : emacs     // emacs | vi
    syntax_highlighting      : true
}

aliases {
    ll : "ls -la"
    gs : "git status"
}

ls {
    icons : nerdfont          // plain | nerdfont | emoji | off
}
```

### 安全配置

```autolang
security {
    sandbox_dir : "/tmp/sandbox"
    no_network  : true
    read_only   : false
}
```

或用 CLI flag 覆盖：`ash --sandbox /tmp --no-network`。

## AI 功能

### F3：自然语言 → 命令

按 F3，描述你想做什么：

```
? 找出所有大于 1MB 的 .rs 文件
```

ash 用 AI 翻译成命令，你确认后执行：

```
建议: find . -name "*.rs" | filter .size > 1.mb
[Enter] 执行  [e] 编辑  [Esc] 取消
```

### F4：AI 对话

按 F4 进多轮对话。可以问问题、要解释、写脚本：

```
▌? 这个 grep 命令是什么意思？
▌? 帮我写一个 cron 表达式，每天凌晨 3 点运行
▌? /clear   （清空对话历史）
▌? /exit    （退出 chat，或按 Esc）
```

> AI 功能需要配置后端（aaid daemon 或 API key 环境变量）。见 [安装指南](installation.md#ai-功能配置可选)。

## 实用技巧

### 把结构化输出转成 JSON

```bash
> ls | to_json --pretty > files.json
> ps | filter .cpu > 1.0 | to_json
```

### 用 `show` 看带语法高亮的文件

```bash
> show main.rs          # 语法高亮（像 bat）
> show data.json        # JSON 高亮
```

### 管道接外部命令

```bash
# ash 结构化命令 → 外部命令
> ls | filter .type == "file" | select name | grep "\.rs$"

# 外部命令 → ash 结构化
> curl -s api.example.com/data | from_json | select name
```

## 下一步

- [bash → ash 速查表](bash-to-ash.md)
- [实例库](../examples/) —— 30+ 可抄的脚本
- [for-agents.md](for-agents.md) —— Agent CLI 用法
