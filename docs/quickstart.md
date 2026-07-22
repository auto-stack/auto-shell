# ASH 快速上手（5 分钟）

> 本指南假设你已经装好了 ash。如果还没，见 [安装指南](installation.md)。

## 1. 启动 ash

```bash
ash
```

你会看到提示符 `>`。跟 bash 一样，但更强大。

## 2. 基础命令（跟 bash 一样）

```bash
> ls                    # 列目录
> ls -la                # 长格式 + 隐藏文件
> cd /tmp               # 切目录
> pwd                   # 当前目录
> echo "hello"          # 打印
> cat file.txt          # 看文件
> grep "TODO" *.rs      # 搜索
```

**所有这些跟 bash 行为一致**。ash 实现了 80+ POSIX 命令，flag 兼容。

## 3. 第一个 "Aha" 时刻：结构化 pipeline

这是 ash 区别于 bash 的核心。试试：

```bash
> ls | sort .size | head -n 5
```

在 bash 里你要写 `ls -la | sort -k5 -rn | head -5`——四段文本管道 + 字段号。在 ash 里，`sort .size` 直接按语义字段排序，因为 **ls 的输出不是文本，是结构化数据**。

更多结构化操作：

```bash
> ls | filter .size > 10.mb              # 找大于 10MB 的文件
> ls | filter .type == "dir"             # 只看目录
> ps | filter .cpu > 1.0 | sort .mem     # 找吃资源的进程
> ls | select name size type             # 只看三列
```

**关键概念**：ash 的命令输出带**语义类型**（FileList / ProcessList / Table / ...）。pipeline 算子（filter/sort/select）按字段名操作，不是按文本列号。

## 4. 数据格式转换（不需要 jq）

ash 内置 JSON / CSV / YAML / TOML / XML 互转：

```bash
> cat data.json | from_json | filter .age > 30 | to_csv
> cat users.csv | from_csv | select name email | to_json
> cat config.yaml | from_yaml | get database.host
```

## 5. F3：自然语言 → 命令

按 **F3**（或 Alt+3），输入你想做的事的自然语言描述：

```
? 列出当前目录最大的 5 个文件
```

ash 会用 AI 翻译成命令，你确认后执行：

```
建议: ls | sort .size | head -n 5
[Enter] 执行  [e] 编辑  [Esc] 取消
```

> F3 需要配置 AI（设置 `ZHIPU_API_KEY` / `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` 环境变量，或启动 aaid daemon）。

## 6. F4：AI 对话

按 **F4**（或 Alt+4），进入多轮 AI 对话：

```
▌? 这个错误是什么意思？
▌? 帮我写一个批量重命名 .jpeg 为 .jpg 的脚本
```

按 **Esc** 退出对话模式。

## 7. 输入模式切换

| 键 | 模式 | 提示符 | 用途 |
|----|------|--------|------|
| F1 | Shell | `>` | 标准 shell（默认） |
| F2 | AutoScript | `#` | 写 AutoLang 脚本 |
| F3 | AI 翻译 | `?` | 自然语言 → 一条命令 |
| F4 | AI 对话 | `▌?` | 多轮 AI chat |

## 8. AutoLang 脚本

按 **F2** 进入 AutoScript 模式，或写 `.ash` 脚本文件：

```bash
# 写到 hello.ash
fn greet(name) {
    print("hello, " + name + "!")
}

greet("world")
```

```bash
ash hello.ash
# 输出: hello, world!
```

AutoLang 比 bash 强大：有类型、闭包、try/catch、递归。详见 [实例库](../examples/)。

## 9. Agent CLI（给 AI Agent 用）

ash 可以被外部 AI Agent（Claude Code / Cursor）安全调用：

```bash
# 拉取所有工具的 schema
ash agent describe-tools --format compact

# 安全探测（不执行）
ash agent check "rm -rf /tmp/old"

# 执行并拿结构化输出
ash agent run "ls -la /sandbox"
```

输出是**结构化 JSON 信封**（不是文本流），Agent 可以可靠解析。详见 [for-agents.md](for-agents.md)。

## 10. 安全沙箱

```bash
# 限制在 /tmp 内，禁网络，只读
ash --sandbox /tmp --no-network --read-only

# 白名单模式（只允许 ls 和 cat）
ash --allow ls --allow cat

# 审计日志
ash --audit /var/log/ash.jsonl -c "your command"
```

## 下一步

- [bash → ash 速查表](bash-to-ash.md) —— 从 bash 迁移
- [实例库](../examples/) —— 30+ 可抄的脚本
- [for-developers.md](for-developers.md) —— 深入功能
- [SKILL.md](../SKILL.md) —— 给 AI Agent 的完整说明

---

**恭喜，你已经掌握了 ash 的核心！** 🎉
