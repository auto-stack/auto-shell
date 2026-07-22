# synctree —— 增量同步目录

只复制源目录中比目标更新的文件。展示 AutoLang 的 for 循环 + 字符串处理 + 条件逻辑。

## 运行
```bash
ash examples/synctree/synctree.ash /source /backup
```

## bash 对照
bash 需 `rsync` 或 `find -newer` + 复杂的 shell 循环。ash 版用 AutoLang 循环 + system 调用,逻辑清晰。
