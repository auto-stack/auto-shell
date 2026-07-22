// examples/diagnose/diagnose.ash
// 错误日志诊断:grep 出错误,用简单启发式规则归类(后续接 Plan 029 真 AI)。
// 展示: grep 提取 + 关键词分类 + HashMap 统计 + 修复建议
//
// 用法: ash diagnose.ash <日志文件>
// 例: ash diagnose.ash /var/log/app.log

fn categorize(line) {
    // 启发式:按关键词判断错误类别(后续 Plan 029 用 AI 替代)
    if line.contains("timeout") || line.contains("Timeout") { return "TIMEOUT" }
    if line.contains("connection refused") || line.contains("Connection refused") { return "CONN_REFUSED" }
    if line.contains("out of memory") || line.contains("OOM") { return "OOM" }
    if line.contains("permission denied") { return "PERMISSION" }
    if line.contains("not found") || line.contains("No such file") { return "NOT_FOUND" }
    if line.contains("panic") { return "PANIC" }
    return "UNKNOWN"
}

fn suggest(category) {
    // 每类给一个修复建议(规则版)
    if category == "TIMEOUT" { return "检查下游服务响应、网络延迟、超时配置" }
    if category == "CONN_REFUSED" { return "检查目标端口是否监听、防火墙规则" }
    if category == "OOM" { return "增加内存、排查内存泄漏、限制并发" }
    if category == "PERMISSION" { return "检查文件/目录权限、运行用户" }
    if category == "NOT_FOUND" { return "检查路径、依赖是否安装" }
    if category == "PANIC" { return "查看堆栈、修代码 bug" }
    return "需人工排查(后续 AI 自动分析)"
}

fn main() {
    var file = system("echo $1").trim()

    if file.len() == 0 {
        print("用法: ash diagnose.ash <日志文件>")
        print("例: ash diagnose.ash /var/log/app.log")
        exit(1)
    }

    print("=== 错误诊断: " + file + " ===")
    print("")

    // grep 出所有 ERROR/FATAL
    var errors = system("grep -iE \"ERROR|FATAL|panic\" \"" + file + "\" 2>/dev/null || true")
    if errors.trim().len() == 0 {
        print("✓ 没有发现错误日志")
        return
    }

    var lines = errors.trim().lines()
    print("发现 " + lines.len().str() + " 条错误,按类别分析:")
    print("")

    // 按类别统计
    var counts = HashMap.new()
    var samples = HashMap.new()  // 每类存一条样本

    for line in lines {
        var cat = categorize(line)
        var prev = counts.get_str(cat)
        if prev.len() == 0 {
            counts.insert_str(cat, "1")
            samples.insert_str(cat, line.trim())
        } else {
            counts.insert_str(cat, prev.to_uint() + 1)
        }
    }

    // 输出诊断报告
    print("类别           | 次数 | 修复建议")
    print("---------------|------|-------------------")
    for (cat, cnt) in counts {
        var tip = suggest(cat)
        print(cat + " | " + cnt + "    | " + tip)
    }
    print("")

    // 显示每类一条样本
    print("=== 样本(每类一条)===")
    for (cat, sample) in samples {
        var display = sample
        if display.len() > 100 { display = display.sub(0, 100) + "..." }
        print("[" + cat + "] " + display)
    }
    print("")
    print("✓ 诊断完成(后续 Plan 029 接 AI 给精确修复建议)")
}

main()
