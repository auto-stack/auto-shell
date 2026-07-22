# batch-replace —— 跨多文件搜索替换

## 运行

```bash
# 实际替换
ash examples/batch-replace/batch-replace.ash src foo bar

# 先预览(dry-run,不修改文件)
ash examples/batch-replace/batch-replace.ash src oldname newname --dry-run
```

## ash 版本亮点

- `grep -rl` 找出含目标文本的文件,再逐个 `sed` 替换
- dry-run 模式先预览影响范围
- AutoLang 包装:统计改动文件数 + 每文件命中次数

## bash 对照

```bash
# bash:一行但无统计、无 dry-run、报错静默
grep -rl "foo" src | xargs sed -i 's/foo/bar/g'
```

bash 的问题:无预览、无计数、文件名带空格会炸、跨 macOS/Linux 的 `sed -i` 语法不同。

## ash 脚本

见 [batch-replace.ash](batch-replace.ash)

## 依赖

- ash v0.5+
