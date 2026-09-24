# du-top —— 目录大小排行

显示当前目录（或指定目录）下各子目录的磁盘占用，最大在前。展示
PLAN-082 结构化互操作：`for-in` 消费 `>` 命令记录 + `$rec.field` 插值互调。

## 运行

```bash
# 当前目录
ash examples/du-top/du-top.ash

# 指定目录
ash examples/du-top/du-top.ash ash/auto-shell
```

## ash 版本亮点

- `for d in > ls $dir | where type == dir | select name` —— 直接迭代 `>`
  命令的**结构化记录**，`d.name` 是字段访问，不是文本切片
- 循环体内 `> du -h $dir/$d.name | where path != total | where path != . |
  select path size` —— 字面量 + `$var` + 字段路径 `$d.name` 混合插值，
  处理结果作为参数再调下一个 `>` 命令
- 未消费的 `>` 命令在函数体内**直接打印表格**（与顶层一致）
- `du -h` 短标志可用（PLAN-082 修复：不再被自动 help 占用）

## bash 对照

```bash
# bash 需 du + sort + head 三段管道 + 文本解析
du -sh /home/*/ 2>/dev/null | sort -rh | head -15
```

bash 的问题:

- `du -sh` 输出是文本(`大小	路径`)，必须 `sort -rh` 按数值排序
- 路径里有空格会破坏 `du -s /home/*` 的分词
- 想换输出格式(如只看大小、或转 JSON)要重写管道

## ash 脚本

见 [du-top.ash](du-top.ash)

## 依赖与已知限制

- 需 PLAN-082 及以后的 ash（`for in >` / `$rec.field` / 体内 `>` 打印）
