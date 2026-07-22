# git-batch —— 跨多仓库批量 git 操作

## 运行

```bash
# 遍历 ~/projects 下所有仓库执行 status
ash examples/git-batch/git-batch.ash ~/projects status

# 统一 pull
ash examples/git-batch/git-batch.ash ~/projects pull
```

## ash 版本亮点

- 自动发现同级目录下的所有 git 仓库
- `pull`/`status`/`fetch` 任意操作,函数封装 + 退出码检查
- 汇总成功/失败数,一眼看出哪个仓库有问题

## bash 对照

```bash
# bash:for 循环 + cd + git,错误处理靠 || true 吞掉
for d in ~/projects/*/; do (cd "$d" && git pull); done
```

bash 的问题:每个仓库要 `cd` 子 shell,失败计数要手动累加,无统一汇总。ash 用 `git -C` 免切换 + AutoLang 计数。

## ash 脚本

见 [git-batch.ash](git-batch.ash)

## 依赖

- ash v0.5+
