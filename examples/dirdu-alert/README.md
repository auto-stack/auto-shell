# dirdu-alert —— 目录大小阈值告警

捕获 `du` 的记录数组，对超过阈值的目录逐条告警。展示 PLAN-082 捕获形态：
`var rows = > 管道` 得到 `Array<Obj>`，`rows.len()` / `r.bytes.to_uint()`
等字段与方法直接可用——不再是 JSON 文本。

## 运行

```bash
# 默认当前目录、阈值 100MB
ash examples/dirdu-alert/dirdu-alert.ash

# 指定目录与阈值（MB）
ash examples/dirdu-alert/dirdu-alert.ash ash/auto-shell 1
```

输出示例：

```
OVER: src = 1212846
checked 2 entries, 1 over 1MB
```

## ash 版本亮点

- `var rows = > du $dir | where path != total | where path != .` —— 捕获的
  是结构化记录数组；v0.1.0 时代同样写法拿到的是 JSON 字符串、
  `rows.name` 静默返回 0
- `r.bytes.to_uint() > limit` —— 记录字段上直接做数值比较（csvsum 同款
  方法面），阈值 `1048576` 换算也在脚本内完成
- 结构化过滤（where）在管道里做，AutoLang 只做告警逻辑

## bash 对照

```bash
du -sb /var/*/ | awk -v lim=$((100*1024*1024)) '$1 > lim {print "OVER:", $2, $1}'
```

bash 的问题:

- `du -sb` 依赖 GNU 扩展；macOS/BSD 没有 `-b`
- 文本两列靠 `$1`/`$2` 位置约定，路径含空格时 `$2` 会截断

## ash 脚本

见 [dirdu-alert.ash](dirdu-alert.ash)

## 依赖与已知限制

- 需 PLAN-082 及以后的 ash（`var x = > cmd` 结构化捕获）
