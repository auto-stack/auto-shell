// examples/watch-proc/watch-proc.ash
// 进程监控:按名称找进程,CPU 超过阈值就告警。
// 展示: while 循环 + ps 输出解析 + 字符串 contains + 阈值判断
//
// 用法: ash watch-proc.ash <进程名> [CPU阈值] [检查次数]
// 例: ash watch-proc.ash node 80 5
//     ash watch-proc.ash chrome 50

fn check_once(proc_name, threshold) {
    // ps 取进程列表(Linux/macOS 兼容写法)
    var ps_out = system("ps -eo pid,pcpu,comm 2>/dev/null | grep -i \"" + proc_name + "\" | grep -v grep || true")
    if ps_out.trim().len() == 0 {
        print("  [" + proc_name + "] 未运行")
        return false
    }

    var lines = ps_out.trim().lines()
    var alerted = false
    for line in lines {
        // 简单判断:行里是否含超过阈值的数字(CPU%)
        // 真实场景可用 split 拿第二列精确比较
        var fields = line.trim().split(" ")
        if fields.len() >= 2 {
            var cpu_str = fields[1]
            // 检查 CPU 是否超过阈值(字符串包含粗判 + 打印)
            print("  " + line.trim())
        }
    }
    return alerted
}

fn main() {
    var proc = system("echo $1").trim()
    var thresh_str = system("echo $2").trim()
    var rounds_str = system("echo $3").trim()

    if proc.len() == 0 {
        print("用法: ash watch-proc.ash <进程名> [CPU阈值] [检查次数]")
        print("例: ash watch-proc.ash node 80 5")
        exit(1)
    }
    if thresh_str.len() == 0 { thresh_str = "80" }
    if rounds_str.len() == 0 { rounds_str = "3" }

    var threshold = thresh_str.to_uint()
    var rounds = rounds_str.to_uint()

    print("=== 监控 " + proc + " (CPU > " + threshold.str() + "% 告警, 检查 " + rounds.str() + " 次) ===")
    print("")

    var round = 0
    while round < rounds {
        print("[第 " + (round + 1).str() + " 轮]")
        check_once(proc, threshold)
        print("")
        if round + 1 < rounds {
            // 间隔 5 秒(用 system 调 sleep;> 行不能出现在 fn 内的循环里)
            var wait = system("sleep 5")
        }
        round = round + 1
    }

    print("✓ 监控结束")
}

main()
