// examples/topn/topn.ash
// 分组取 Top N:按某列分组,每组取数值列最大的 N 行。
// 展示: HashMap 分组聚合 + 嵌套结构 + 排序选取
//
// 用法: ash topn.ash <文件.csv> <分组列> <数值列> [N]
// 例: ash topn.ash sales.csv region amount 3
//     ash topn.ash scores.csv class score 5

fn main() {
    var file = system("echo $1").trim()
    var group_col = system("echo $2").trim()
    var val_col = system("echo $3").trim()
    var n_str = system("echo $4").trim()

    if file.len() == 0 || group_col.len() == 0 || val_col.len() == 0 {
        print("用法: ash topn.ash <文件.csv> <分组列> <数值列> [N]")
        print("例: ash topn.ash sales.csv region amount 3")
        exit(1)
    }
    if n_str.len() == 0 { n_str = "3" }
    var n = n_str.to_uint()

    var content = system("cat \"" + file + "\"")
    var lines = content.trim().lines()

    if lines.len() == 0 {
        print("文件为空")
        exit(1)
    }

    // 解析表头找列索引
    var header = lines[0].split(",")
    var g_idx = -1
    var v_idx = -1
    var ci = 0
    for col in header {
        if col.trim() == group_col { g_idx = ci }
        if col.trim() == val_col { v_idx = ci }
        ci = ci + 1
    }
    if g_idx < 0 || v_idx < 0 {
        print("✗ 找不到指定列")
        exit(1)
    }

    // 分组:每组存 "key|value|row" 用 List,再排序取前 N
    // 这里用 HashMap 把每组的数据行收集起来(以字符串拼接)
    var groups = HashMap.new()

    var ri = 1
    while ri < lines.len() {
        var fields = lines[ri].split(",")
        if fields.len() > g_idx && fields.len() > v_idx {
            var key = fields[g_idx].trim()
            var val = fields[v_idx].trim()
            var row = lines[ri]
            // 把 (val, row) 追加到该组(用换行分隔的缓冲)
            var prev = groups.get_str(key)
            var entry = val + "\t" + row
            if prev.len() == 0 {
                groups.insert_str(key, entry)
            } else {
                groups.insert_str(key, prev + "\n" + entry)
            }
        }
        ri = ri + 1
    }

    // 每组排序取前 N(用 sort -rn 按数值)
    print("=== Top " + n.str() + " (按 " + group_col + " 分组, " + val_col + " 排序) ===")
    print("")
    for (key, buf) in groups {
        print("【" + key + "】")
        // 把缓冲写临时文件排序取前 N
        // 这里简化:直接用 sort 管道
        var sorted = system("echo '" + buf + "' | sort -t$'\\t' -k1 -rn | head -n " + n_str + " | cut -f2-")
        var slines = sorted.trim().lines()
        for s in slines {
            print("  " + s)
        }
        print("")
    }
}

main()
