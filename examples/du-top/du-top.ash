// examples/du-top/du-top.ash
// 目录大小排行：当前目录（或 $1 指定目录）下各子目录的磁盘占用，最大在前。
// 展示 PLAN-082 结构化互操作：for-in 消费 `>` 命令记录 + $rec.field 插值互调。
//
// 用法: ash du-top.ash [目录]
// 实测: PLAN-082 起 `>` 五形态可用；du -h 短标志已修复（不再被 help 占用）

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }

    // 形态 1：for-in 逐记录迭代 `>` 命令的结构化结果（ls 记录 {name type size modified}）
    for d in > ls $dir | where type == dir | select name {
        // 形态 3+4：循环体内以记录字段为参数再调另一个 `>` 命令；
        // 未消费的 `>` 命令直接打印表格（体内与顶层一致）。
        // $dir/$d.name 是字面量+$var+字段路径的混合插值；du 内部已按
        // bytes 降序，where 掉 total 与根行即得排行。
        > du -h $dir/$d.name | where path != total | where path != . | select path size
    }
    exit(0)
}

main()
