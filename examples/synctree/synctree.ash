// examples/synctree/synctree.ash
// 增量同步目录:只复制源目录中比目标更新的文件。
// 展示: for 循环 + 字符串拼接 + system + 条件判断
//
// 用法: ash synctree.ash <源目录> <目标目录>

fn main() {
    var src = system("echo $1").trim()
    var dst = system("echo $2").trim()

    if src.len() == 0 || dst.len() == 0 {
        print("用法: ash synctree.ash <源目录> <目标目录>")
        exit(1)
    }

    print("同步 " + src + " → " + dst)

    // 确保目标目录存在
    > mkdir -p $dst

    // 列出源目录所有文件
    var files = system("find " + src + " -type f 2>/dev/null || true")
    var lines = files.trim().lines()
    var copied = 0
    var skipped = 0

    for fpath in lines {
        if fpath.trim().len() == 0 { continue }

        // 计算相对路径 + 目标路径
        var rel = fpath.sub(src.len(), fpath.len())
        var target = dst + rel

        // 检查目标是否存在且比源旧(用 system 判断)
        var target_exists = system("test -f \"" + target + "\" && echo yes || echo no")
        var src_mtime = system("stat -c %Y \"" + fpath + "\" 2>/dev/null || stat -f %m \"" + fpath + "\" 2>/dev/null || echo 0")
        var dst_mtime = system("stat -c %Y \"" + target + "\" 2>/dev/null || stat -f %m \"" + target + "\" 2>/dev/null || echo 0")

        if target_exists.trim() == "yes" && src_mtime.trim() <= dst_mtime.trim() {
            skipped = skipped + 1
        } else {
            // 确保目标子目录存在,然后复制
            var target_dir = system("dirname \"" + target + "\"")
            > mkdir -p $target_dir
            > cp "$fpath" "$target"
            copied = copied + 1
            print("  复制 " + rel)
        }
    }

    print("✓ 同步完成: " + copied.str() + " 个文件复制, " + skipped.str() + " 个跳过")
}

main()
