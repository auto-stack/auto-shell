# deploy-ai —— AI 部署助手(release notes 占位版)

## 运行

```bash
# 部署到 staging
ash examples/deploy-ai/deploy-ai.ash staging

# 部署到 production
ash examples/deploy-ai/deploy-ai.ash production
```

## ash 版本亮点

- 端到端 pipeline:build → test → deploy → 健康检查 → release notes
- 每阶段函数封装 + `system_status()` 判断,任一阶段失败即停
- `export("DEPLOY_ENV", env)` 让环境进入 shell
- release notes 用 `system("echo ...")` 模拟 AI 生成(后续 Plan 029 接真实模型)

## bash 对照

```bash
# bash:&& 链 + echo 占位,阶段间无结构、失败处理靠 ||
cargo build && cargo test && deploy && echo notes
```

bash 的问题:阶段无封装、无统一进度、健康检查循环难写。ash 用 `stage()` 函数 + while 循环。

## ash 脚本

见 [deploy-ai.ash](deploy-ai.ash)

## 依赖

- ash v0.5+(AI 生成需 Plan 029)
