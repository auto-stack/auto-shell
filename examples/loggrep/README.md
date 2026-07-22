# loggrep —— 日志提取（grep + 统计 + 格式化报告）

## 运行

```bash
# 搜索包含 ERROR 的行,统计数量
ash examples/loggrep/loggrep.ash /var/log/app.log ERROR

# 只看最近 N 条匹配
ash examples/loggrep/loggrep.ash app.log "timeout" 20
```

## ash 版本亮点

- AutoLang 函数 + shell bridge(system/grep)组合,不是纯管道
- 有变量、循环、条件——bash 要写一堆 awk/sed 才能做的事
- 错误处理(try/catch):日志文件不存在不崩溃
- 格式化报告输出(含时间戳、计数)

## bash 对照

```bash
# bash 版:grep + wc + echo 拼接,无错误处理
grep "$2" "$1" > /tmp/found
echo "Found $(wc -l < /tmp/found) matches:"
head -${3:-10} /tmp/found
```

bash 的问题:
- 临时文件 `/tmp/found`(ash 版直接用变量)
- `wc -l < file` 的输出有空格要 `trim`
- 文件不存在会报错但不优雅(ash 有 try/catch)
- 统计/格式化全靠文本拼接

## ash 脚本

见 [loggrep.ash](loggrep.ash)

## 依赖

- ash v0.5+
