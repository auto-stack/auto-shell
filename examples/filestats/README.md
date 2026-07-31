# filestats —— 文件类型统计

按扩展名分组统计文件数量。展示 HashMap + for 循环 + 字符串分割。

## 运行

```bash
# 默认统计当前目录
ash examples/filestats/filestats.ash

# 指定目录
ash examples/filestats/filestats.ash /project
```

## ash 版本亮点

- 用 HashMap 做扩展名 → 计数的聚合,逻辑显式
- 扩展名提取(最后一个 `.` 之后)用字符串 `split`,清晰可控
- 输出带总计行,易扩展为占比、按数量排序

## bash 对照

```bash
# bash 需 ls + sed/awk 提取扩展名 + sort | uniq -c 统计
ls -1 /project | sed 's/.*\.//' | sort | uniq -c | sort -rn
```

bash 的问题:
- `sed 's/.*\.//'` 靠正则提取扩展名(无扩展名的文件会得到完整文件名,需额外处理)
- `uniq -c` 输出格式固定(`  3 txt`),解析要再切列
- 想加"无扩展名"分类、总计行要拼更多管道
- 排序靠文本(`sort -rn`),与统计耦合

ash 的做法:
- HashMap 聚合,扩展名是 map 的 key,计数是 value
- "无扩展名"作为默认 key 显式处理
- 总计在主流程里累加,一行输出
- 想按计数排序,加一行对 map 排序即可

## ash 脚本

见 [filestats.ash](filestats.ash)

## 依赖

- ash v0.5+
