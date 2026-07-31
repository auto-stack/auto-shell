// examples/filestats/filestats.ash
// 按文件扩展名分组统计文件数量。
// 展示: HashMap 统计 + for 循环 + 字符串 split
//
// 用法: ash filestats.ash [目录]

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }

    print("统计 " + dir + " 的文件类型分布:")
    print("-----------------------------------")

    // 用 ls 拿文件名列表
    var files = system("ls -1 " + dir + " 2>/dev/null || true")
    var lines = files.trim().lines()

    // HashMap 存 扩展名 → 计数
    var stats = HashMap.new()

    for fname in lines {
        if fname.trim().len() == 0 { continue }

        // 提取扩展名(最后一个 . 后面的部分)
        // NOTE: 变量名用 `extension` 而非 `ext` —— 后者在重新赋值时会触发
        // auto-lang 解析器的一个 bug(标识符 `ext` 被特殊处理)。见 Plan 034 附录。
        // NOTE: 不调 .lower() —— split 产生的字符串上调 .lower() 会触发 native
        // 栈布局问题(返回 -2147483647);.upper() 则正常。扩展名按原样统计。
        var extension = "无扩展名"
        var dot_pos = fname.find(".")
        if dot_pos >= 0 {
            // 找最后一个点
            var parts = fname.split(".")
            if parts.len() > 1 {
                var last_idx = parts.len() - 1
                extension = parts[last_idx]
            }
        }

        // 累加计数
        var count = stats.get_str(extension)
        if count.len() == 0 {
            stats.insert_str(extension, "1")
        } else {
            stats.insert_str(extension, count.to_uint() + 1)
        }
    }

    // 输出统计结果
    var total = 0
    for (extension, count) in stats {
        print("  ." + extension + ": " + count + " 个")
        total = total + count.to_uint()
    }

    print("-----------------------------------")
    print("总计: " + total.str() + " 个文件")
}

main()
