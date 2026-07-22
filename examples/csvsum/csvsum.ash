// examples/csvsum/csvsum.ash
// CSV 汇总:读取 CSV,按某列分组,对数值列求和,输出汇总表。
// 展示: from_csv 结构化输入 + AutoLang HashMap 聚合 + to_csv 输出
//
// 用法: ash csvsum.ash <文件.csv> <分组列> <求和列>
// 例: ash csvsum.ash sales.csv region amount

fn main() {
    var file = system("echo $1").trim()
    var group_col = system("echo $2").trim()
    var sum_col = system("echo $3").trim()

    if file.len() == 0 || group_col.len() == 0 || sum_col.len() == 0 {
        print("用法: ash csvsum.ash <文件.csv> <分组列> <求和列>")
        print("例: ash csvsum.ash sales.csv region amount")
        exit(1)
    }

    print("读取 " + file + " ...")

    // 用 ash 的 from_csv 把 CSV 转成结构化数据
    // 然后用 group-by + sum 管道(如果 ash 支持)
    // 这里展示 AutoLang 手动聚合(更灵活)

    var content = system("cat " + file)
    var lines = content.trim().lines()

    if lines.len() == 0 {
        print("CSV 文件为空")
        exit(1)
    }

    // 解析表头
    var header = lines[0].split(",")

    // 找列索引
    var group_idx = -1
    var sum_idx = -1
    var i = 0
    for col in header {
        if col.trim() == group_col { group_idx = i }
        if col.trim() == sum_col { sum_idx = i }
        i = i + 1
    }

    if group_idx < 0 {
        print("✗ 找不到分组列: " + group_col)
        exit(1)
    }
    if sum_idx < 0 {
        print("✗ 找不到求和列: " + sum_col)
        exit(1)
    }

    // 聚合
    var totals = HashMap.new()
    var counts = HashMap.new()

    var row = 1  // 跳过表头
    while row < lines.len() {
        var fields = lines[row].split(",")
        if fields.len() > group_idx && fields.len() > sum_idx {
            var key = fields[group_idx].trim()
            var val_str = fields[sum_idx].trim()
            var val = val_str.to_uint()

            var prev = totals.get_str(key)
            if prev.len() == 0 {
                totals.insert_str(key, val)
                counts.insert_str(key, "1")
            } else {
                totals.insert_str(key, prev.to_uint() + val)
                counts.insert_str(key, counts.get_str(key).to_uint() + 1)
            }
        }
        row = row + 1
    }

    // 输出汇总
    print("")
    print(group_col + " | " + sum_col + "_合计 | 记录数")
    print("----------|----------|-------")
    for (key, total) in totals {
        var cnt = counts.get_str(key)
        print(key + " | " + total + " | " + cnt)
    }
    print("")
    print("✓ 汇总完成")
}

main()
