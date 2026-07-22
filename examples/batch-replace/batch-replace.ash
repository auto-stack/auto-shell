// examples/batch-replace/batch-replace.ash
// 跨多文件搜索替换:遍历匹配文件,逐个替换文本,统计改动数。
// 展示: find + for 循环 + 字符串 replace + dry-run 模式
//
// 用法: ash batch-replace.ash <目录> <旧文本> <新文本> [--dry-run]
// 例: ash batch-replace.ash src foo bar

fn main() {
    var dir = system("echo $1").trim()
    var old_text = system("echo $2").trim()
    var new_text = system("echo $3").trim()
    var flag = system("echo $4").trim()

    if dir.len() == 0 || old_text.len() == 0 || new_text.len() == 0 {
        print("用法: ash batch-replace.ash <目录> <旧文本> <新文本> [--dry-run]")
        print("例: ash batch-replace.ash src foo bar")
        exit(1)
    }

    var is_dry = flag.contains("dry")
    if is_dry {
        print("=== DRY RUN (不会真正修改文件) ===")
    }

    // 列出目录下所有文本文件(排除常见二进制/构建产物)
    var files = system("grep -rl \"" + old_text + "\" " + dir + " 2>/dev/null || true")
    var lines = files.trim().lines()

    if lines.len() == 0 {
        print("没有文件包含 \"" + old_text + "\"")
        return
    }

    print("在 " + lines.len().str() + " 个文件中替换: \"" + old_text + "\" → \"" + new_text + "\"")
    print("-------------------------------------------")

    var changed = 0
    for fpath in lines {
        if fpath.trim().len() == 0 { continue }

        // 读文件内容,在 AutoLang 内存里替换(比 sed 跨平台、无引号转义问题)
        var content = system("cat \"" + fpath + "\" 2>/dev/null || true")
        if content.len() == 0 { continue }

        // 数命中次数(按 old_text 分割后段数 - 1)
        var parts = content.split(old_text)
        var hits = parts.len() - 1
        print("  " + fpath + "  (" + hits.str() + " 处)")

        if !is_dry && hits > 0 {
            // AutoLang 原生 replace,再写回文件
            var new_content = content.replace(old_text, new_text)
            var write_cmd = "cat > \"" + fpath + "\" <<'ASH_EOF'\n" + new_content + "\nASH_EOF"
            var w = system(write_cmd)
        }
        changed = changed + 1
    }

    if is_dry {
        print("-------------------------------------------")
        print("=== 将修改 " + changed.str() + " 个文件(去掉 --dry-run 执行) ===")
    } else {
        print("-------------------------------------------")
        print("✓ 已修改 " + changed.str() + " 个文件")
    }
}

main()
