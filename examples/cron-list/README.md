# cron-list —— crontab 转人类可读

## 运行

```bash
# 解析当前用户 crontab
ash examples/cron-list/cron-list.ash

# 解析指定 crontab 文件
ash examples/cron-list/cron-list.ash /etc/crontab
```

## ash 版本亮点

- 把 `*/5 * * * * cmd` 翻译成"每 5 分钟, 每小时..."的中文说明
- 处理 `*`、`*/N`、具体值三种字段形式
- 跳过注释行,只列真正的定时任务

## bash 对照

```bash
# bash:直接 crontab -l,五个星号要自己脑补含义
crontab -l
```

bash 的问题:cron 表达式可读性极差,`*/5`、`1,3,5` 都要人肉翻译。ash 自动解释。

## ash 脚本

见 [cron-list.ash](cron-list.ash)

## 依赖

- ash v0.5+(Linux/macOS 的 `crontab`)
