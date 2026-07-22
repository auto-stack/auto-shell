# user-activity —— 查看用户登录活动

## 运行

```bash
# 当前在线用户(who)
ash examples/user-activity/user-activity.ash

# 登录历史(last)
ash examples/user-activity/user-activity.ash 历史
```

## ash 版本亮点

- 解析 `who`/`last` 输出,拆分成 用户/终端/来源 字段
- 两种模式:当前在线 vs 登录历史
- AutoLang 字符串 split 把杂乱文本整理成表格

## bash 对照

```bash
# bash:直接 who/last,字段对齐靠原始输出
who
last | head -20
```

bash 的问题:输出是原始文本,字段靠空格对齐,无法结构化。ash 拆分字段成表格。

## ash 脚本

见 [user-activity.ash](user-activity.ash)

## 依赖

- ash v0.5+(Linux/macOS 的 `who`/`last`)
