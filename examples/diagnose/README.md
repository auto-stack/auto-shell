# diagnose —— 错误日志诊断(规则版)

## 运行

```bash
ash examples/diagnose/diagnose.ash /var/log/app.log
```

## ash 版本亮点

- grep 出 ERROR/FATAL,用关键词启发式归类(TIMEOUT/OOM/PERMISSION 等)
- HashMap 按类别统计次数 + 每类存一条样本
- 每个类别给出修复建议(规则版,后续 Plan 029 接 AI 精确分析)

## bash 对照

```bash
# bash:grep + 手动归类,建议全靠人脑
grep -iE "ERROR|FATAL" app.log | less
```

bash 的问题:错误一堆要人肉读、归类靠经验、无统计。ash 自动分类 + 计数 + 给建议。

## ash 脚本

见 [diagnose.ash](diagnose.ash)

## 依赖

- ash v0.5+(AI 精确诊断需 Plan 029)
