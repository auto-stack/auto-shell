# fmt-check —— 代码格式化检查

## 运行

```bash
# 检查 src/ 下所有 .rs
ash examples/fmt-check/fmt-check.ash

# 检查指定目录
ash examples/fmt-check/fmt-check.ash lib
```

## ash 版本亮点

- 逐文件跑 `rustfmt --check`,用 `system_status()` 判断是否已格式化
- List 收集未格式化文件,单独列出
- 给出修复命令提示

## bash 对照

```bash
# bash:cargo fmt --check 整体报错,不告诉你哪个文件
cargo fmt --check || echo "需要 fmt"
```

bash 的问题:整体失败但看不出具体哪些文件。ash 逐文件检查,精确报告。

## ash 脚本

见 [fmt-check.ash](fmt-check.ash)

## 依赖

- ash v0.5+(rustfmt 已随 Rust 安装)
