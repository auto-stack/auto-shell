# smart-commit —— 智能提交(规则版)

## 运行

```bash
# 暂存全部改动 + 提交
ash examples/smart-commit/smart-commit.ash

# 提交并推送
ash examples/smart-commit/smart-commit.ash --push
```

## ash 版本亮点

- 解析 `git status --porcelain`,按 add/update/remove 分类统计改动
- 自动生成提交信息(`chore: add 3 files, update 2 files`)
- 交互确认,显示 diff 概要后再提交
- 这是 Plan 029 `git.finish-worktree` SmartCommand 的简化版;后续接 AI 自动写 release notes

## bash 对照

```bash
# bash:手动敲 add/commit -m,push 要再敲一遍
git add -A && git commit -m "..." && git push
```

bash 的问题:提交信息要人肉想、改动统计要 `git status` 人眼看。ash 自动汇总 + 生成 message。

## ash 脚本

见 [smart-commit.ash](smart-commit.ash)

## 依赖

- ash v0.5+(AI 增强需 Plan 029)
