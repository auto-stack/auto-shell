# topn —— 分组取 Top N

## 运行

```bash
# 每个地区 amount 最高的 3 条
ash examples/topn/topn.ash sales.csv region amount 3

# 每个 class score 最高的 5 条
ash examples/topn/topn.ash scores.csv class score 5
```

## ash 版本亮点

- 按分组列聚合,每组按数值列排序取前 N
- HashMap 分组 + sort 管道组合
- 自动解析表头找列索引,无需硬编码列号

## bash 对照

```bash
# bash:sort + awk + head 多段拼接,分组逻辑极绕
sort -t, -k2,2 -k3,3nr sales.csv | awk -F, '!seen[$2]++{c[$2]++} c[$2]<=3'
```

bash 的问题:awk 计数黑魔法、列号脆弱、可读性极差。ash 用 HashMap 分组 + 显式循环。

## ash 脚本

见 [topn.ash](topn.ash)

## 依赖

- ash v0.5+
