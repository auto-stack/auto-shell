# disk-clean —— 找大文件并清理

## 运行

```bash
# 扫描当前目录 >100MB 的文件
ash examples/disk-clean/disk-clean.ash

# 扫描 /tmp,阈值 50MB
ash examples/disk-clean/disk-clean.ash /tmp 50
```

## ash 版本亮点

- `find -size +Nc` 按字节阈值过滤大文件
- 列清单 + 逐个显示大小,确认后才删,避免误删
- 删除/失败分别计数

## bash 对照

```bash
# bash:find + xargs rm,无确认、误删无救
find . -size +100M -delete
```

bash 的问题:`-delete` 直接删无确认,看不清删了什么。ash 列清单 + 交互确认。

## ash 脚本

见 [disk-clean.ash](disk-clean.ash)

## 依赖

- ash v0.5+
