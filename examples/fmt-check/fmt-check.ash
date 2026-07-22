// examples/fmt-check/fmt-check.ash
// 代码格式化检查:跑 rustfmt --check,报告未格式化的文件。
// 展示: for 循环 + system_status 逐文件判断 + 汇总
//
// 用法: ash fmt-check.ash [目录]
// 默认: src/ 下的所有 .rs 文件

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "src" }

    print("=== 格式化检查: " + dir + " 下的 .rs 文件 ===")
    print("")

    var files = system("find " + dir + " -name \"*.rs\" -type f 2>/dev/null | sort || true")
    var lines = files.trim().lines()

    if lines.len() == 0 {
        print("没有找到 .rs 文件")
        return
    }

    var unformatted = List.new()
    var clean = 0

    for fpath in lines {
        if fpath.trim().len() == 0 { continue }

        // rustfmt --check:文件已格式化返回 0,需要格式化返回 1
        var out = system("rustfmt --check \"" + fpath + "\" 2>/dev/null")
        var code = system_status()

        if code == 0 {
            clean = clean + 1
        } else {
            unformatted.push(fpath.trim())
        }
    }

    if unformatted.is_empty() {
        print("✓ 全部 " + clean.str() + " 个文件已格式化")
        return
    }

    print("⚠ " + unformatted.len().str() + " 个文件未格式化:")
    for f in unformatted {
        print("  " + f)
    }
    print("")
    print("已格式化: " + clean.str() + " / " + (clean + unformatted.len()).str())
    print("")
    print("修复: cargo fmt  (或 rustfmt " + dir + "/**/*.rs)")
}

main()
