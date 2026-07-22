// examples/biglog/biglog.ash
// 大日志流式分析:逐行读日志,按错误类型(ERROR/WARN/...)统计次数。
// 展示: 行流式处理 + HashMap 聚合 + 按级别分类
//
// 用法: ash biglog.ash <日志文件>
// 例: ash biglog.ash /var/log/app.log
//     ash biglog.ash huge.log

fn classify(line) {
    // 根据日志级别关键词归类
    if line.contains("FATAL") || line.contains("CRITICAL") { return "FATAL" }
    if line.contains("ERROR") { return "ERROR" }
    if line.contains("WARN") { return "WARN" }
    if line.contains("INFO") { return "INFO" }
    if line.contains("DEBUG") { return "DEBUG" }
    return "OTHER"
}

fn main() {
    var file = system("echo $1").trim()

    if file.len() == 0 {
        print("用法: ash biglog.ash <日志文件>")
        print("例: ash biglog.ash /var/log/app.log")
        exit(1)
    }

    print("=== 大日志分析: " + file + " ===")
    print("(流式逐行处理,适合 GB 级文件)")
    print("")

    // 用 grep 流式提取各级别(grep 不把整个文件读进内存)
    // 真正的大文件:这里用 grep 计数,避免 AutoLang 一次读全文件
    var levels = ["FATAL", "ERROR", "WARN", "INFO", "DEBUG"]
    var counts = HashMap.new()

    // 各级别用 grep -c 统计(流式,内存友好)
    for level in levels {
        var cnt = system("grep -c \"" + level + "\" \"" + file + "\" 2>/dev/null || echo 0")
        counts.insert_str(level, cnt.trim())
    }

    // 输出分类统计
    print("级别        | 行数")
    print("------------|-----------")
    var total = 0
    for (level, cnt) in counts {
        print(level + "   | " + cnt)
        total = total + cnt.to_uint()
    }
    print("------------|-----------")
    print("总计匹配    | " + total.str())

    // 抽样显示最严重的几条 FATAL/ERROR
    print("")
    print("=== 最近 5 条 ERROR/FATAL (抽样) ===")
    var severe = system("grep -E \"ERROR|FATAL\" \"" + file + "\" 2>/dev/null | tail -5 || true")
    if severe.trim().len() == 0 {
        print("(无)")
    } else {
        var slines = severe.trim().lines()
        for s in slines {
            var display = s
            if display.len() > 120 { display = display.sub(0, 120) + "..." }
            print("  " + display)
        }
    }
    print("")
    print("✓ 分析完成")
}

main()
