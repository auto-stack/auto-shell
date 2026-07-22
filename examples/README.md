# ASH 脚本实例库

30+ 个 AutoLang 脚本实例，展示 ash 的结构化 pipeline、AutoLang 编程能力、以及对照 bash 的优势。

## 运行方式

```bash
# 在 ash REPL 里
ash> source examples/bigfiles/bigfiles.ash

# 或从命令行
ash examples/bigfiles/bigfiles.ash
```

## 实例分类

### 文件操作
| 实例 | 说明 |
|------|------|
| [bigfiles](bigfiles/) | 找出目录下最大的 N 个文件 |

### 文本处理
| 实例 | 说明 |
|------|------|
| [loggrep](loggrep/) | 日志提取（grep + 上下文 + 时间过滤） |

（更多实例持续添加中，完整清单见 [designs/034-script-examples.md](../designs/034-script-examples.md)）

## 每个实例的结构

```
example-name/
├── README.md     # 说明 + bash 对照 + 运行方式
└── name.ash      # ash 脚本（可直接运行）
```

## ash vs bash 的核心差异

ash 脚本的优势在于**结构化数据**——命令输出不是文本流，是带类型的对象：

```bash
# bash：四段文本管道 + 字段号
du -a | sort -rn | head -10 | cut -f2

# ash：一行语义化 pipeline
ls | sort .size | head -n 10
```

更多对照见 [docs/bash-to-ash.md](../docs/bash-to-ash.md)。
