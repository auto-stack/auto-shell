// examples/cron-list/cron-list.ash
// crontab 解析:把 cron 表达式翻译成人类可读的中文说明。
// 展示: 字符串 split + 字段映射 + HashMap + 格式化输出
//
// 用法: ash cron-list.ash           # 解析当前用户 crontab
//       ash cron-list.ash <文件>    # 解析指定 crontab 文件

fn explain_field(value, unit) {
    if value == "*" {
        return "每" + unit
    }
    if value.starts_with("*/") {
        var step = value.sub(2, value.len())
        return "每 " + step + " " + unit
    }
    return unit + " " + value
}

fn explain_cron(line) {
    // 标准 cron: 分 时 日 月 周 命令
    var parts = line.split(" ")
    // 过滤掉空字段
    var fields = List.new()
    for p in parts {
        if p.trim().len() > 0 { fields.push(p.trim()) }
    }

    if fields.len() < 6 { return "" }

    var minute = fields.get(0)
    var hour = fields.get(1)
    var day = fields.get(2)
    var month = fields.get(3)
    var weekday = fields.get(4)

    // 拼命令(第 6 个字段之后全是命令)
    var cmd = ""
    var i = 5
    while i < fields.len() {
        cmd = cmd + fields.get(i) + " "
        i = i + 1
    }

    var sched = explain_field(minute, "分钟") + ", "
    sched = sched + explain_field(hour, "小时") + ", "
    sched = sched + explain_field(day, "日") + ", "
    sched = sched + explain_field(month, "月") + ", "
    sched = sched + explain_field(weekday, "周")

    return sched + "  →  " + cmd.trim()
}

fn main() {
    var file = system("echo $1").trim()
    var content = ""

    if file.len() == 0 {
        // 读当前用户 crontab
        content = system("crontab -l 2>/dev/null || true")
        print("=== 当前用户 crontab(人类可读)===")
    } else {
        content = system("cat \"" + file + "\" 2>/dev/null || true")
        print("=== " + file + " (人类可读)===")
    }
    print("")

    if content.trim().len() == 0 {
        print("(无 crontab 或文件为空)")
        return
    }

    var lines = content.lines()
    var count = 0
    for line in lines {
        var t = line.trim()
        // 跳过空行和注释
        if t.len() == 0 || t.starts_with("#") { continue }

        var readable = explain_cron(t)
        if readable.len() > 0 {
            count = count + 1
            print("[" + count.str() + "] " + readable)
        }
    }

    print("")
    print("✓ 共 " + count.str() + " 条定时任务")
}

main()
