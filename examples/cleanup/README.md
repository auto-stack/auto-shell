# cleanup —— 清理临时文件

找指定目录下的 `*.tmp`/`*.bak`/`*.log` 文件，列出并确认后删除。

## 运行
```bash
ash examples/cleanup/cleanup.ash /tmp
```

## bash 对照
bash 需 `find /tmp -name "*.tmp" -exec rm -i {} \;`——三段管道 + exec。ash 版用 system + 确认循环。
