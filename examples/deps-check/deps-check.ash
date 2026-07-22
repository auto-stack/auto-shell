// examples/deps-check/deps-check.ash
// 依赖检查:解析 Cargo.toml,列出所有依赖及其版本。
// 展示: 文件读取 + 字符串 split/contains + 结构化报告
//
// 用法: ash deps-check.ash [Cargo.toml 路径]
// 默认: ./Cargo.toml

fn main() {
    var file = system("echo $1").trim()
    if file.len() == 0 { file = "Cargo.toml" }

    print("=== 依赖清单: " + file + " ===")
    print("")

    var content = system("cat \"" + file + "\" 2>/dev/null || true")
    if content.trim().len() == 0 {
        print("✗ 读不到 " + file + "(文件不存在?)")
        exit(1)
    }

    var lines = content.lines()
    var in_deps = false
    var deps = List.new()

    for line in lines {
        var t = line.trim()

        // 进入 [dependencies] 段
        if t == "[dependencies]" {
            in_deps = true
            continue
        }
        // 遇到下一个 [...] 段就退出
        if t.starts_with("[") && t.ends_with("]") {
            if in_deps { in_deps = false }
            continue
        }

        if in_deps && t.len() > 0 && !t.starts_with("#") {
            // 形如: serde = "1.0"  或  serde = { version = "1.0", features = [...] }
            var eq_pos = t.find("=")
            if eq_pos > 0 {
                var name = t.sub(0, eq_pos).trim()
                var rest = t.sub(eq_pos + 1, t.len()).trim()
                // 提取版本号(引号之间的部分)
                var version = rest
                var q1 = rest.find("\"")
                if q1 >= 0 {
                    var after = rest.sub(q1 + 1, rest.len())
                    var q2 = after.find("\"")
                    if q2 >= 0 { version = after.sub(0, q2) }
                }
                deps.push(name + "|" + version)
            }
        }
    }

    if deps.is_empty() {
        print("没有找到依赖")
        return
    }

    print("包名                版本")
    print("-------------------|---------")
    for d in deps {
        var parts = d.split("|")
        var name = parts[0]
        var ver = ""
        if parts.len() > 1 { ver = parts[1] }
        print(name + " | " + ver)
    }
    print("")
    print("✓ 共 " + deps.len().str() + " 个依赖")
    print("提示: 用 `cargo outdated` 检查哪些有新版本")
}

main()
