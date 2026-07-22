// examples/loccount/loccount.ash
// 按语言(扩展名)统计代码行数。
// 展示: for 循环 + HashMap 分组聚合 + 字符串 split
//
// 用法: ash loccount.ash [目录]
// 默认: 当前目录,递归统计

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }

    print("统计 " + dir + " 的代码行数(按语言分组):")
    print("-------------------------------------------")

    // 各语言对应的扩展名映射
    var langs = HashMap.new()
    langs.insert_str("rs", "Rust")
    langs.insert_str("py", "Python")
    langs.insert_str("js", "JavaScript")
    langs.insert_str("ts", "TypeScript")
    langs.insert_str("go", "Go")
    langs.insert_str("java", "Java")
    langs.insert_str("c", "C")
    langs.insert_str("cpp", "C++")
    langs.insert_str("sh", "Shell")
    langs.insert_str("ash", "AutoLang")

    // 按扩展名聚合行数和文件数(两个 HashMap)
    var loc = HashMap.new()
    var fcount = HashMap.new()

    // 遍历所有源码文件
    var files = system("find " + dir + " -type f \\( -name \"*.rs\" -o -name \"*.py\" -o -name \"*.js\" -o -name \"*.ts\" -o -name \"*.go\" -o -name \"*.java\" -o -name \"*.c\" -o -name \"*.cpp\" -o -name \"*.sh\" -o -name \"*.ash\" \\) 2>/dev/null | grep -v node_modules | grep -v target || true")
    var lines = files.trim().lines()

    for fpath in lines {
        if fpath.trim().len() == 0 { continue }

        // 提取扩展名(最后一个 . 之后)
        var parts = fpath.split(".")
        if parts.len() < 2 { continue }
        var ext = parts[parts.len() - 1].lower()

        // 跳过不在映射里的扩展名
        if !langs.contains(ext) { continue }

        // wc -l 数行数
        var wc_out = system("wc -l < \"" + fpath + "\" 2>/dev/null || echo 0")
        var n = wc_out.trim()

        // 累加到该语言的 loc / 文件数
        var prev_loc = loc.get_str(ext)
        var prev_fc = fcount.get_str(ext)
        if prev_loc.len() == 0 {
            loc.insert_str(ext, n)
            fcount.insert_str(ext, "1")
        } else {
            loc.insert_str(ext, prev_loc.to_uint() + n.to_uint())
            fcount.insert_str(ext, prev_fc.to_uint() + 1)
        }
    }

    // 输出汇总表
    print("语言        | 文件数 | 代码行")
    print("------------|--------|-------")
    var grand_total = 0
    for (ext, lines_n) in loc {
        var lang = langs.get_str(ext)
        var fc = fcount.get_str(ext)
        print(lang + " | " + fc + "      | " + lines_n)
        grand_total = grand_total + lines_n.to_uint()
    }
    print("------------|--------|-------")
    print("总计        |        | " + grand_total.str() + " 行")
}

main()
