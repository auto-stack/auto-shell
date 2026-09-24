# query-block —— `> { }` 多段命令块

演示 `> { A; B; C }` 命令块的两种位置语义：

- **语句位置**：每段都执行并打印（先 `cd` 再查询的两步准备流程）
- **捕获位置**（`var t = > { ... }`）：前段照常执行（`cd` 生效），
  取**末段**的结构化记录

## 运行

```bash
ash examples/query-block/query-block.ash          # 当前目录
ash examples/query-block/query-block.ash ash/ash  # 指定目录
```

输出示例：

```
（子目录清单表格）
total bytes under ash/ash: 468745
```

## ash 版本亮点

- `> { cd $dir; ls | where type == dir | select name }` —— 语句位块逐段
  执行，`cd` 与查询在同一块内有序完成
- `var t = > { du | where path == total }` —— 捕获位块：前段 `cd` 已改变
  工作目录，末段 `du` 的 total 行记录进入 `t`（`t[0].bytes` 直接可读）
- 分号/换行均可分段；首期不支持块嵌套

## bash 对照

```bash
total=$(cd "$dir" && du -sb | tail -1 | cut -f1)
echo "total bytes under $dir: $total"
```

bash 的问题:

- `cd` 的作用域靠子 shell `( )` 手工隔离，忘了就污染当前目录
- `du` 输出要 `tail`/`cut` 两道文本工序才能取出 total 数字

## ash 脚本

见 [query-block.ash](query-block.ash)

## 依赖与已知限制

- 需 PLAN-082 及以后的 ash（`> {}` 块语法）
