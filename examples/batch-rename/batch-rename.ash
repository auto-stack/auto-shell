// examples/batch-rename/batch-rename.ash
// 批量重命名:把目录下所有 .jpeg 改成 .jpg(或自定义扩展名映射)。
// 展示: for 循环 + 字符串 replace + system mv + dry-run 模式
//
// 用法: ash batch-rename.ash <目录> <旧扩展> <新扩展> [--dry-run]

fn main() {
    var dir = system("echo $1").trim()
    var old_ext = system("echo $2").trim()
    var new_ext = system("echo $3").trim()
    var dry_run = system("echo $4").trim()

    if dir.len() == 0 || old_ext.len() == 0 || new_ext.len() == 0 {
        print("用法: ash batch-rename.ash <目录> <旧扩展> <新扩展> [--dry-run]")
        print("例: ash batch-rename.ash . jpeg jpg")
        exit(1)
    }

    var is_dry = dry_run.contains("dry")

    if is_dry {
        print("=== DRY RUN (不会真正重命名) ===")
    }

    // 找所有匹配旧扩展名的文件
    var pattern = "*." + old_ext
    var files = system("ls -1 " + dir + "/" + pattern + " 2>/dev/null || true")
    var lines = files.trim().lines()

    if lines.len() == 0 {
        print("没有找到 ." + old_ext + " 文件")
        return
    }

    var renamed = 0
    for fname in lines {
        if fname.trim().len() == 0 { continue }

        // 构造新文件名:替换扩展名
        var new_name = fname.replace("." + old_ext, "." + new_ext)
        if new_name == fname {
            continue  // 没变化,跳过
        }

        print("  " + fname + " → " + new_name)

        if !is_dry {
            > mv "$dir/$fname" "$dir/$new_name"
        }
        renamed = renamed + 1
    }

    if is_dry {
        print("=== 将重命名 " + renamed.str() + " 个文件(去掉 --dry-run 执行) ===")
    } else {
        print("✓ 已重命名 " + renamed.str() + " 个文件")
    }
}

main()
