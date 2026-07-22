// examples/du-top/du-top.ash
// 目录大小排行:显示占用空间最大的 N 个子目录。
// 展示: 结构化 pipeline + AutoLang 参数处理
//
// 用法: ash du-top.ash [目录] [显示数]

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }
    var count = system("echo $2").trim()
    if count.len() == 0 { count = "10" }

    print("=== " + dir + " 下最大的 " + count + " 个子目录 ===")
    print("")

    // ash 结构化: du 输出 → sort → head
    // 对比 bash: du | sort -rn | head | 数值对齐很麻烦
    var cmd = "du -s " + dir + "/*/ 2>/dev/null | sort -rn | head -n " + count
    > $cmd

    print("")
    print("提示: ash 原生版可用 du | from_csv | sort .size | head")
}

main()
