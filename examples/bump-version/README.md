# bump-version —— 跨文件同步更新版本号

## 运行

```bash
# 升到 1.2.0
ash examples/bump-version/bump-version.ash 1.2.0

# 先预览
ash examples/bump-version/bump-version.ash 0.3.1 --dry-run
```

## ash 版本亮点

- 一次更新 `Cargo.toml` / `package.json` / `pyproject.toml` 的版本号
- 自动检测每个文件当前版本,显示 `旧 → 新`
- dry-run 预览,跨语言项目版本统一

## bash 对照

```bash
# bash:sed 各写一段,正则易错、版本号格式各异
sed -i 's/version = .*/version = "1.2.0"/' Cargo.toml
sed -i 's/"version": .*/"version": "1.2.0",/' package.json
```

bash 的问题:每文件一条不同正则,无预览、无统一汇总。ash 用函数 `bump_file` 复用逻辑。

## ash 脚本

见 [bump-version.ash](bump-version.ash)

## 依赖

- ash v0.5+
