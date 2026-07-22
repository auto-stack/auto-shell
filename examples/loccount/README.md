# loccount —— 按语言统计代码行数

## 运行

```bash
# 统计当前目录
ash examples/loccount/loccount.ash

# 统计指定项目
ash examples/loccount/loccount.ash /path/to/project
```

## ash 版本亮点

- 按 `.rs`/`.py`/`.js`/`.ts`/`.go`/`.java`/`.c` 等扩展名分组
- HashMap 聚合行数与文件数,输出汇总表
- 自动跳过 `node_modules` / `target` 等构建产物

## bash 对照

```bash
# bash:cloc 要装;cloc 之外需 find + awk + sort 拼接
find . -name "*.rs" | xargs wc -l | tail -1
```

bash 的问题:每语言一段、要手动汇总、无统一表格输出。ash 用 HashMap 一次聚合。

## ash 脚本

见 [loccount.ash](loccount.ash)

## 依赖

- ash v0.5+
