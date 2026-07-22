// examples/svc-status/svc-status.ash
// 服务状态检查:按端口检查一组服务是否在响应。
// 展示: HashMap 配置表 + for 循环 + curl 探活 + 状态汇总
//
// 用法: ash svc-status.ash
// 内置检查一组常见服务端口(可改脚本里的配置)

fn check_port(name, port) {
    // 用 curl 探活:连接成功返回 0,超时/拒绝返回非 0
    var out = system("curl -s -o /dev/null -w \"%{http_code}\" --max-time 3 http://localhost:" + port + " 2>/dev/null || echo 000")
    var code = out.trim()
    if code == "000" {
        print("  ✗ " + name + " (:" + port + ")  未响应")
        return false
    }
    print("  ✓ " + name + " (:" + port + ")  HTTP " + code)
    return true
}

fn main() {
    // 服务配置:名字 → 端口(改这里即可定制)
    var services = HashMap.new()
    services.insert_str("Web", "8080")
    services.insert_str("API", "3000")
    services.insert_str("DB", "5432")
    services.insert_str("Cache", "6379")

    print("=== 服务状态检查 ===")
    print("")

    var up = 0
    var down = 0

    for (name, port) in services {
        if check_port(name, port) {
            up = up + 1
        } else {
            down = down + 1
        }
    }

    print("")
    print("=== 汇总: " + up.str() + " 在线, " + down.str() + " 离线 ===")
    if down > 0 {
        print("⚠ 有服务离线,请检查")
        exit(1)
    }
    print("✓ 全部在线")
}

main()
