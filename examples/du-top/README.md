# du-top —— 目录大小排行

显示占用空间最大的 N 个子目录。展示 shell bridge + 结构化思路。

## 运行

```bash
# 默认当前目录,取前 10 个
ash examples/du-top/du-top.ash

# 指定目录和数量
ash examples/du-top/du-top.ash /home 15
```

## ash 版本亮点

- 用 `du` 取数据 + AutoLang 排序,不依赖外部 `sort -rn`
- 输出可结构化(改 `to_json` / `select` 即可变换输出形态)
- 阈值、数量参数化,封装成可复用函数

## bash 对照

```bash
# bash 需 du + sort + head 三段管道 + 文本解析
du -s /home/* 2>/dev/null | sort -rn | head -15
```

bash 的问题:
- `du -s` 输出是文本(`大小\t路径`),必须 `sort -rn` 按数值排序
- 路径里有空格会破坏 `du -s /home/*` 的分词
- 想换输出格式(如只看大小、或转 JSON)要重写管道

ash 的做法:
- `du` 输出经 AutoLang 解析成结构化记录,按字段排序
- 路径作为整体字段,不怕空格
- 输出形态可一行切换(`select` / `to_json`)

## ash 脚本

见 [du-top.ash](du-top.ash)

## 依赖

- ash v0.5+
