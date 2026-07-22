// examples/user-activity/user-activity.ash
// 用户活动:显示最近登录的用户(解析 who / last 输出)。
// 展示: shell 命令输出解析 + 字符串 split + 表格输出
//
// 用法: ash user-activity.ash [当前|历史]
// 例: ash user-activity.ash           # 默认看当前登录(who)
//     ash user-activity.ash 历史       # 看登录历史(last)

fn show_current() {
    print("=== 当前在线用户 (who) ===")
    print("")
    var out = system("who 2>/dev/null || true")
    if out.trim().len() == 0 {
        print("(无输出或命令不可用)")
        return
    }
    var lines = out.trim().lines()
    print("用户         终端       来源")
    print("------------|-----------|-----------")
    for line in lines {
        // who 输出:username tty date time (来源)
        var parts = line.trim().split(" ")
        if parts.len() >= 5 {
            var user = parts[0]
            var tty = parts[1]
            // 来源通常在最后
            var src = parts[parts.len() - 1]
            print(user + " | " + tty + " | " + src)
        } else {
            print(line.trim())
        }
    }
}

fn show_history() {
    print("=== 最近登录历史 (last, 前 20 条) ===")
    print("")
    var out = system("last -20 2>/dev/null || true")
    if out.trim().len() == 0 {
        print("(无输出或命令不可用)")
        return
    }
    var lines = out.trim().lines()
    var shown = 0
    for line in lines {
        if shown >= 20 { break }
        print("  " + line.trim())
        shown = shown + 1
    }
}

fn main() {
    var mode = system("echo $1").trim()

    if mode == "历史" || mode == "history" {
        show_history()
    } else {
        show_current()
    }
}

main()
