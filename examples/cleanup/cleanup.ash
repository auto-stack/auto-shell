// examples/cleanup/cleanup.ash
// 清理临时文件(*.tmp, *.bak, *.log)。列出后确认删除。
// 展示: system + for 循环 + 用户确认交互
//
// 用法: ash cleanup.ash [目录]

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }

    print("扫描 " + dir + " 下的临时文件...")

    // 收集多种临时文件扩展名。ash 的内置 find 不支持 GNU 的 -o(OR)/分组,
    // 所以这里循环每个扩展名单独 find,再合并(等价的 bash 是
    // `find . \( -name '*.tmp' -o -name '*.bak' -o -name '*.log' \)`)。
    var patterns = ["*.tmp", "*.bak", "*.log"]
    var found = ""
    for p in patterns {
        var r = system("find " + dir + " -maxdepth 1 -name " + p + " -type f 2>/dev/null || true")
        if r.trim().len() > 0 {
            if found.len() > 0 { found = found + "\n" }
            found = found + r.trim()
        }
    }

    if found.trim().len() == 0 {
        print("✓ 没有找到临时文件")
        return
    }

    var lines = found.trim().lines()
    print("找到 " + lines.len().str() + " 个临时文件:")
    for line in lines {
        print("  " + line)
    }
    print("")

    // 确认删除
    print("确认删除以上文件? (y/n)")
    var confirm = system("read -r response; echo $response")
    if confirm.trim() != "y" {
        print("已取消")
        return
    }

    var deleted = 0
    for line in lines {
        var result = system("rm \"" + line + "\" 2>/dev/null")
        if system_status() == 0 {
            deleted = deleted + 1
        }
    }
    print("✓ 已删除 " + deleted.str() + " 个文件")
}

main()
