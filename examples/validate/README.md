# validate —— CSV 字段校验

## 运行

```bash
ash examples/validate/validate.ash users.csv
```

## ash 版本亮点

- 三类校验:必填列非空、email 含 @、数值列是数字
- 逐行逐列扫描,List 收集所有错误
- 合法则退出 0,有问题列出全部并退出 1(便于 CI 卡)

## bash 对照

```bash
# bash:awk 写校验逻辑,规则一多就难以维护
awk -F, 'NR>1 && $1=="" {print "row "NR" id empty"}' users.csv
```

bash 的问题:每条规则一段 awk、错误信息靠字符串拼接、多规则组合难。ash 用函数 + List 显式收集。

## ash 脚本

见 [validate.ash](validate.ash)

## 依赖

- ash v0.5+
