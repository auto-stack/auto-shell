# bash → ash 速查表

> 从 bash 迁移到 ash 的快速参考。ash 的 80+ 命令跟 bash POSIX 兼容，大部分命令直接一样。本表聚焦**差异和 ash 独有能力**。

---

## 命令对照

### 基础命令（完全相同）

| bash | ash | 说明 |
|------|-----|------|
| `ls -la` | `ls -la` | ✅ 相同 |
| `cd /path` | `cd /path` | ✅ 相同 |
| `pwd` | `pwd` | ✅ 相同 |
| `cat file` | `cat file` | ✅ 相同 |
| `echo "hi"` | `echo "hi"` | ✅ 相同 |
| `cp -r src dst` | `cp -r src dst` | ✅ 相同 |
| `mv old new` | `mv old new` | ✅ 相同 |
| `rm -rf dir` | `rm -rf dir` | ✅ 相同 |
| `mkdir -p a/b/c` | `mkdir -p a/b/c` | ✅ 相同 |
| `head -n 20 file` | `head -n 20 file` | ✅ 相同 |
| `tail -f log` | `tail -f log` | ✅ 相同 |
| `wc -l file` | `wc -l file` | ✅ 相同 |
| `sort -u` | `sort -u` | ✅ 相同 |
| `which python` | `which python` | ✅ 相同 |

> 💡 **`find` 和 `grep` 兼容 GNU/POSIX 标志。** `find . -name "*.rs" -type f -maxdepth 2`
> 和 `grep -rn "pat" .` 都能直接用(Plan 034 修复了 find 的单横杠长 flag 兼容)。
> ash 的 `find`/`grep` 是内置重实现,既认 POSIX 标志,也支持 ash 的结构化输出——
> 管道里 `find . -name "*.rs" | select .path` 可直接用。详见各命令的 `help <cmd>`。

### ash 增强版（结构化输出）

| bash | ash | 说明 |
|------|-----|------|
| `du -a \| sort -rn \| head` | `ls \| sort .size \| head` | ash 按语义字段排序 |
| `ls -la \| awk '{print $5}'` | `ls \| select .size` | ash 不用 awk 取列 |
| `ps aux \| sort -k3 -rn \| head` | `ps \| sort .cpu \| head` | ash 按字段名 |
| `ls \| grep "^d"` | `ls \| filter .type == "dir"` | ash 按语义过滤 |
| `find . -size +10M` | `ls \| filter .size > 10.mb` | ash 带单位 |
| `cat f.json \| jq '.[].name'` | `cat f.json \| from_json \| select .name` | ash 原生 JSON |
| `cat f.csv \| cut -d, -f1` | `cat f.csv \| from_csv \| select .col1` | ash 原生 CSV |

### ash 独有命令（bash 没有）

| ash 命令 | 说明 |
|----------|------|
| `from_json` / `to_json` | JSON 解析/序列化 |
| `from_csv` / `to_csv` | CSV 解析/序列化 |
| `from_yaml` / `to_yaml` | YAML 解析/序列化 |
| `from_toml` / `to_toml` | TOML 解析/序列化 |
| `from_xml` / `to_xml` | XML 解析/序列化 |
| `show file` | 带语法高亮看文件（像 bat） |
| `http_get URL` | HTTP GET（内置） |
| `sys` | 系统信息（CPU/内存/磁盘） |
| `ash agent run "cmd"` | Agent CLI（结构化信封输出） |
| `ash agent describe-tools` | 工具 catalog（给 AI Agent） |

---

## 语法对照

### 变量

| bash | ash (AutoLang) | 说明 |
|------|----------------|------|
| `VAR="hello"` | `var name = "hello"` | AutoLang 用 var |
| `echo $VAR` | `print(name)` 或 `echo $name` | shell 模式里 `$` 仍可用 |
| `$(command)` | `system("command")` | AutoLang shell bridge |
| `$1`, `$2`, `$@` | `args[0]`, `args[1]`, `args` | AutoLang 参数 |

### 条件

| bash | ash (AutoLang) |
|------|----------------|
| `if [ -f file ]; then ...; fi` | `if exists("file") { ... }` |
| `if [ $x -gt 5 ]; then ...; fi` | `if x > 5 { ... }` |
| `if [ -z "$var" ]; then ...; fi` | `if var == "" { ... }` |

### 循环

| bash | ash (AutoLang) |
|------|----------------|
| `for f in *.txt; do ...; done` | `for f in glob("*.txt") { ... }` |
| `for i in $(seq 1 10); do ...; done` | `for i in range(1, 10) { ... }` |
| `while true; do ...; done` | `while true { ... }` |

### 函数

| bash | ash (AutoLang) |
|------|----------------|
| `name() { ... }` | `fn name() { ... }` |
| `function name { ... }` | `fn name() { ... }` |
| `return $value` | `return value` |
| `local var` | `var var`（默认局部） |

### 管道与重定向

| bash | ash | 说明 |
|------|-----|------|
| `cmd1 \| cmd2` | `cmd1 \| cmd2` | ✅ 相同 |
| `cmd > file` | `cmd > file` | ✅ 相同 |
| `cmd >> file` | `cmd >> file` | ✅ 相同 |
| `cmd 2>&1` | `cmd 2>&1` | ✅ 相同 |
| `cmd1 && cmd2` | `cmd1 && cmd2` | ✅ 相同 |
| `cmd1 \|\| cmd2` | `cmd1 \|\| cmd2` | ✅ 相同 |

### 错误处理

| bash | ash (AutoLang) |
|------|----------------|
| `set -e` | `try { ... } catch(e) { ... }` |
| `trap cleanup EXIT` | （后续支持） |
| `if ! cmd; then ...; fi` | `try { cmd } catch(e) { ... }` |

---

## ash 独有特性（bash 完全没有的）

### 1. 结构化 Pipeline DSL

```bash
# bash 做不到 —— 必须靠 awk/sort 文本解析
# ash 原生支持
> ls | filter .size > 10.mb | sort .name | select name size type
> ps | filter .cpu > 1.0 | group-by .user | sum .cpu
```

### 2. 语义类型系统

ash 的命令输出带类型标签（FileList / ProcessList / Table / Record / ...），pipeline 算子据此决定操作。bash 永远只有文本。

### 3. 安全沙箱

```bash
ash --sandbox /project --no-network --read-only
ash --allow ls --allow cat        # 白名单
ash --audit /var/log/ash.jsonl    # 全审计
```

bash 没有任何内置安全机制。

### 4. Agent CLI

```bash
ash agent describe-tools          # 79 工具的 JSON Schema
ash agent run "ls"                # 结构化信封输出
```

bash 没有 Agent 接口。

### 5. 内置 AI

F3 自然语言翻译 + F4 多轮 chat。bash 无 AI。

### 6. 跨平台一致

同一个 `ls -la`，三平台行为完全一致。bash 在 Windows 上要靠 WSL/Git Bash，行为有差异。

---

## 迁移建议

### 可以直接用的

你的 bash 习惯大部分可以直接用——ls/cd/grep/sort/find/cat/echo/cp/mv/rm 都兼容。先正常用，遇到差异查本表。

### 值得改用 ash 写法的

1. **数据提取**：用 `from_json | select .field` 替代 `jq`
2. **文件过滤**：用 `ls | filter .size > 10.mb` 替代 `find -size +10M`
3. **脚本**：用 AutoLang 的 `fn`/`try-catch` 替代 bash 的 function/trap

### 暂时不兼容的（ash 还没有的）

- **进程替换** `<(cmd)` —— 计划中
- **数组** `${arr[@]}` —— AutoLang 有更强的数据结构
- **here-document** `<<EOF` —— ash 用 `>` 前缀的脚本语法
- **complete -F**（自定义补全函数）—— 用 ash 的补全 spec `.at` 文件

---

## 相关文档

- [快速上手](quickstart.md)
- [开发者指南](for-developers.md)
- [实例库](../examples/) —— 30+ 实例，含 bash 对照
