# deploy —— 端到端部署流水线(MS3 demo)

build → test → deploy 的多阶段流水线,综合展示 AutoLang 的核心能力:
`fn` + `while` + `try/catch` + `system()` + `export()` + `exit()` + 字符串拼接。

> 这是 ash 最早期的 MS3 端到端 demo。AI 增强版见 [deploy-ai/](../deploy-ai/),
> 后者在部署后额外用 AI 生成 release notes(Plan 029 占位实现)。

## 运行

```bash
ash examples/deploy/deploy.ash
```

## ash 版本亮点

- 多阶段流水线封装进 `fn`,每阶段用 `try/catch` 容错
- `export()` 设置环境变量,`system_status()` 判断阶段成败,失败即 `exit(1)`
- 全流程在一个脚本里,可读、可测、可改

## bash 对照

bash 版通常是一个带 `set -e` 的脚本,靠退出码串联:

```bash
#!/bin/bash
set -e
echo "[build] cargo build --release" && cargo build --release
echo "[test] cargo test" && cargo test
echo "[deploy] shipping" && ./ship.sh
```

bash 的问题:
- `set -e` 在子 shell / 管道里的行为微妙(`set -o pipefail` 又是另一坑)
- 想给每阶段加自定义失败处理要 `trap` 或显式 `if`,啰嗦
- 无结构化的"阶段"概念,全靠约定

ash 的做法:
- `fn stage(name, cmd)` 把"运行命令 + 报告成败"封装成可复用单元
- `try/catch` 显式处理每阶段错误,控制流清晰
- 失败时 `exit(code)` 直接带语义退出

## ash 脚本

见 [deploy.ash](deploy.ash)

## 依赖

- ash v0.5+
