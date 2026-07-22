// examples/loggrep/loggrep.ash
//
// 日志搜索工具:在日志文件里搜关键词,统计数量,格式化报告。
// 展示 AutoLang 的函数 + 变量 + 循环 + try/catch + shell bridge。
//
// 用法: ash examples/loggrep/loggrep.ash <文件> <关键词> [显示条数]
// 默认: 显示前 10 条匹配

// ── 格式化报告头 ──
fn print_header(file, keyword) {
    print("╔══════════════════════════════════════╗")
    print("║  日志搜索报告                         ║")
    print("╚══════════════════════════════════════╝")
    print("文件: " + file)
    print("关键词: " + keyword)
    print("")
}

// ── 搜索并统计 ──
fn search_log(file, keyword, limit) {
    // 用 shell bridge 调 grep,结果存入 AutoLang 变量
    var matches = system("grep " + keyword + " " + file + " 2>/dev/null || true")

    if matches.trim().len() == 0 {
        print("未找到匹配 '" + keyword + "' 的行")
        return 0
    }

    // 按行分割(AutoLang 字符串方法)
    var lines = matches.trim().lines()
    var total = lines.len()

    print("找到 " + total.str() + " 条匹配:")
    print("-------------------------------------------")

    // 显示前 limit 条(用 for 循环遍历)
    var shown = 0
    for line in lines {
        if shown >= limit {
            break
        }
        // 简单截断过长的行
        var display = line
        if display.len() > 120 {
            display = display.sub(0, 120) + "..."
        }
        print(display)
        shown = shown + 1
    }

    if total > limit {
        print("... 还有 " + (total - limit).str() + " 条未显示")
    }

    return total
}

// ── 主函数 ──
fn main() {
    // 解析参数
    var args = system("echo $@")
    var parts = args.trim().split(" ")

    var file = ""
    var keyword = "ERROR"
    var limit = 10

    if parts.len() > 0 && parts[0].len() > 0 {
        file = parts[0]
    }
    if parts.len() > 1 && parts[1].len() > 0 {
        keyword = parts[1]
    }
    if parts.len() > 2 && parts[2].len() > 0 {
        limit = parts[2].to_uint()
    }

    if file.len() == 0 {
        print("用法: ash loggrep.ash <文件> [关键词] [显示条数]")
        print("默认关键词: ERROR, 默认显示: 10 条")
        exit(1)
    }

    print_header(file, keyword)

    // try/catch: 文件不存在等错误不崩溃
    try {
        var count = search_log(file, keyword, limit)
        print("-------------------------------------------")
        if count > 0 {
            print("✓ 完成,共 " + count.str() + " 条匹配")
        }
    } catch(e) {
        print("✗ 搜索失败: " + e)
        exit(1)
    }
}

main()
