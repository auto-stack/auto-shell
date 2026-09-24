// examples/query-block/query-block.ash
// `> { A; B; C }` 多段命令块：语句位置逐段执行打印；捕获位置前段执行
// （如 cd 生效）、取末段的结构化记录。
//
// 用法: ash query-block.ash [目录]

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }

    // 语句位块：先 cd 到目标目录，再打印其子目录清单（两段都执行）
    > { cd $dir; ls | where type == dir | select name }

    // 捕获位块：前段 cd 已生效，末段 du 的 total 行记录被捕获为 t
    var t = > { du | where path == total }
    if t.len() == 1 {
        print("total bytes under " + dir + ": " + t[0].bytes)
        exit(0)
    }
    print("unexpected: " + t.len() + " total rows")
    exit(1)
}

main()
