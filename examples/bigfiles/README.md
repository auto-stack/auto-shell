# bigfiles —— 找出目录下最大的 N 个文件

## 运行

```bash
# 默认当前目录，取前 10 个
ash examples/bigfiles/bigfiles.ash

# 指定目录和数量
ash examples/bigfiles/bigfiles.ash /path/to/dir 20
```

## ash 版本亮点

- 用 `ls | sort .size | head` 结构化 pipeline，按语义字段排序
- 输出是结构化 Table（可进一步 `| to_json` 或 `| select name size`）
- AutoLang 包装成可复用函数 + 参数处理

## bash 对照

```bash
# bash 需要四段文本管道 + 字段号解析
du -a /path | sort -rn | head -10 | cut -f2 | xargs -I{} ls -lh {}
```

bash 的问题：
- `du -a` 输出是文本（`大小\t路径`），必须用 `sort -rn` 按数值排序
- `cut -f2` 靠字段号取路径（脆弱）
- 最后还要 `xargs ls -lh` 回去取人类可读的大小

ash 的做法：
- `ls` 输出直接是结构化数据（含 `.size` 字段）
- `sort .size` 按语义字段名排序
- 整个 pipeline 一行，清晰可读

## ash 脚本

见 [bigfiles.ash](bigfiles.ash)

## 依赖

- ash v0.5+
