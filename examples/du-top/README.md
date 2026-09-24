# du-top —— 目录大小排行

显示当前目录下各子目录的磁盘占用，最大在前。展示结构化 pipeline。

## 运行

```bash
# 当前目录
ash examples/du-top/du-top.ash

# 指定目录
ash examples/du-top/du-top.ash ash
```

## ash 版本亮点

- `du` 是 ash 内置命令，直接输出结构化记录 `{path size bytes}`（内部已按 bytes 降序）
- 过滤/选列在管道里完成（`where path != total`、`select path size`），不怕路径含空格
- AutoLang 只负责参数处理和退出码，不做文本切片

## bash 对照

```bash
# bash 需 du + sort + head 三段管道 + 文本解析
du -sh /home/*/ 2>/dev/null | sort -rh | head -15
```

bash 的问题:

- `du -sh` 输出是文本(`大小\t路径`)，必须 `sort -rh` 按数值排序
- 路径里有空格会破坏 `du -s /home/*` 的分词
- 想换输出格式(如只看大小、或转 JSON)要重写管道

## ash 脚本

见 [du-top.ash](du-top.ash)

## 依赖与已知限制

- ash v0.1.0 实测(2026-09-24)
- du 短标志 `-h` 被帮助占用，人类可读大小需用长标志 `--human-readable`
- `system()` 拿到的是 JSON 文本；结构化变换要在管道里做，脚本侧 AutoLang 尚无
  from_json 内建(见 skills/ash-scripting)
