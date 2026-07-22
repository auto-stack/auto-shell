# svc-status —— 按端口检查服务状态

## 运行

```bash
# 检查内置的一组服务端口
ash examples/svc-status/svc-status.ash
```

## ash 版本亮点

- 用 `curl` 探活 `localhost:PORT`,取 HTTP 状态码
- HashMap 配置服务名 → 端口,改一处即可定制
- 在线/离线汇总,离线则非零退出(便于告警串联)

## bash 对照

```bash
# bash:for + curl,变量计数靠手动
for p in 8080 3000 5432; do curl -s localhost:$p; done
```

bash 的问题:无名字映射、无汇总计数、状态码解析要 `awk`。ash 用 HashMap + 函数封装。

## ash 脚本

见 [svc-status.ash](svc-status.ash)

## 依赖

- ash v0.5+(需要 curl)
