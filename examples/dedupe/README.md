# dedupe —— 按键列去重

## 运行

```bash
# 按第 0 列(从 0 开始)去重
ash examples/dedupe/dedupe.ash users.csv 0

# 按第 2 列去重
ash examples/dedupe/dedupe.ash data.csv 2
```

## ash 版本亮点

- 按指定键列去重,HashMap 记录已见 key,O(n) 去重
- 保留表头,逐行判断是否重复
- 输出保留行 + 去重计数

## bash 对照

```bash
# bash:sort -u 全行去重,按指定列要 awk
awk -F, '!seen[$1]++' users.csv
```

bash 的问题:`awk '!seen[$1]++'` 是黑魔法、新人看不懂、列号硬编码。ash 用 HashMap + 循环,逻辑清晰可读。

## ash 脚本

见 [dedupe.ash](dedupe.ash)

## 依赖

- ash v0.5+
