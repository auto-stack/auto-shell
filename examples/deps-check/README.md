# deps-check —— 解析 Cargo.toml 列出依赖

## 运行

```bash
# 默认读 ./Cargo.toml
ash examples/deps-check/deps-check.ash

# 指定文件
ash examples/deps-check/deps-check.ash path/to/Cargo.toml
```

## ash 版本亮点

- 用 AutoLang 解析 TOML 的 `[dependencies]` 段,识别 `name = "version"` 与 `name = { ... }` 两种形式
- List 收集 + 结构化表格输出
- 无需额外工具(对照:`cargo outdated` 要单独装)

## bash 对照

```bash
# bash:awk 提取,版本号正则易错
awk -F'=' '/^[a-z]/ {print $1}' Cargo.toml
```

bash 的问题:awk 切分靠字段号,内联 table/feature 写法解析不全。ash 用字符串方法 + List 显式构造。

## ash 脚本

见 [deps-check.ash](deps-check.ash)

## 依赖

- ash v0.5+
