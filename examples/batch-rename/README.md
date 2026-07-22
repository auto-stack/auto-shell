# batch-rename —— 批量重命名文件

把目录下所有 `.jpeg` 改成 `.jpg`（或任意扩展名映射）。支持 `--dry-run` 预览。展示 AutoLang 的 for 循环 + 字符串 replace + dry-run 模式。

## 运行
```bash
# 预览
ash examples/batch-rename/batch-rename.ash . jpeg jpg --dry-run

# 真正执行
ash examples/batch-rename/batch-rename.ash . jpeg jpg
```

## bash 对照
bash 需 `for f in *.jpeg; do mv "$f" "${f%.jpeg}.jpg"; done`——参数扩展语法晦涩。ash 版用 `.replace()` 清晰可读。
