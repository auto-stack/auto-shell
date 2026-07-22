// examples/dedupe/dedupe.ash
// 数据去重:按指定的键列去除 CSV/数据里的重复行。
// 展示: HashMap 记录已见 key + 行过滤 + 计数
//
// 用法: ash dedupe.ash <文件.csv> <键列序号(从0)>
// 例: ash dedupe.ash users.csv 0    # 按第 0 列去重
//     ash dedupe.ash data.csv 2      # 按第 2 列去重

fn main() {
    var file = system("echo $1").trim()
    var key_str = system("echo $2").trim()

    if file.len() == 0 || key_str.len() == 0 {
        print("用法: ash dedupe.ash <文件.csv> <键列序号(从0)>")
        print("例: ash dedupe.ash users.csv 0")
        exit(1)
    }

    var key_col = key_str.to_uint()

    var content = system("cat \"" + file + "\"")
    var lines = content.trim().lines()

    if lines.len() == 0 {
        print("文件为空")
        exit(1)
    }

    print("=== 去重: " + file + " (按第 " + key_col.str() + " 列) ===")
    print("")

    // 表头保留
    var header = lines[0]
    print(header)

    // 用 HashMap 记录已出现过的 key
    var seen = HashMap.new()
    var kept = 0
    var dropped = 0

    var i = 1
    while i < lines.len() {
        var line = lines[i]
        if line.trim().len() == 0 {
            i = i + 1
            continue
        }

        var fields = line.split(",")
        if fields.len() > key_col {
            var key = fields[key_col].trim()
            if seen.contains(key) {
                dropped = dropped + 1
            } else {
                seen.insert_str(key, "1")
                print(line)
                kept = kept + 1
            }
        } else {
            // 列数不够,保留原行
            print(line)
            kept = kept + 1
        }
        i = i + 1
    }

    print("")
    print("✓ 保留 " + kept.str() + " 行, 去重 " + dropped.str() + " 行")
}

main()
