# biglog —— 大日志流式分析

## 运行

```bash
ash examples/biglog/biglog.ash /var/log/app.log
ash examples/biglog/biglog.ash huge.log
```

## ash 版本亮点

- 按 FATAL/ERROR/WARN/INFO/DEBUG 级别统计行数
- 用 `grep -c` 流式计数,不把整个大文件读进内存,适合 GB 级日志
- 抽样显示最近几条最严重的错误

## bash 对照

```bash
# bash:grep + wc 拼接,每级别一段,无抽样
grep -c ERROR app.log
grep -c WARN app.log
```

bash 的问题:每个级别手写一段、无汇总、无抽样。ash 用 for + HashMap 一次聚合 + 自动抽样。

## ash 脚本

见 [biglog.ash](biglog.ash)

## 依赖

- ash v0.5+
