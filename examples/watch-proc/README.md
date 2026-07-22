# watch-proc —— 进程 CPU 监控告警

## 运行

```bash
# 监控 node,CPU>80% 告警,检查 5 次
ash examples/watch-proc/watch-proc.ash node 80 5

# 监控 chrome,CPU>50%
ash examples/watch-proc/watch-proc.ash chrome 50
```

## ash 版本亮点

- 按进程名过滤 `ps` 输出,多轮检查 + 间隔
- CPU 超阈值告警,可配置阈值和轮次
- AutoLang while 循环控制检查节奏

## bash 对照

```bash
# bash:top + grep 循环,变量计数和 sleep 拼接
while true; do ps aux | grep node | grep -v grep; sleep 5; done
```

bash 的问题:无限循环无退出条件、计数靠外部、无阈值判断。ash 用 while + 计数器可控。

## ash 脚本

见 [watch-proc.ash](watch-proc.ash)

## 依赖

- ash v0.5+
