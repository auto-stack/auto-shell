// examples/disk-clean/disk-clean.ash
// 磁盘清理:找出大于阈值(默认 100MB)的文件,列清单,确认后删除。
// 展示: find + 大小过滤 + 用户确认 + 删除统计
//
// 用法: ash disk-clean.ash [目录] [大小阈值MB]
// 例: ash disk-clean.ash /tmp 100
//     ash disk-clean.ash ~/Downloads 50

fn main() {
    var dir = system("echo $1").trim()
    var size_str = system("echo $2").trim()

    if dir.len() == 0 { dir = "." }
    if size_str.len() == 0 { size_str = "100" }

    print("=== 扫描 " + dir + " 下 > " + size_str + "MB 的文件 ===")
    print("")

    // find 按大小过滤:直接用 -size +NM 单位(find 原生支持 M=MB),
    // 避开在脚本里做 `mb * 1024 * 1024` 的字节换算 —— ash 的 .to_uint() 目前
    // 在 auto-lang 里有返回类型 bug(见 Plan 034 附录 Bug 1),算术会出错,
    // 所以这里把阈值原样(字符串)传给 find,由 find 自己换算。
    var cmd = "find " + dir + " -type f -size +" + size_str + "M 2>/dev/null | head -50 || true"
    var files = system(cmd)
    var lines = files.trim().lines()

    if lines.len() == 0 {
        print("✓ 没有找到大于 " + size_str + "MB 的文件")
        return
    }

    print("找到 " + lines.len().str() + " 个大文件:")
    print("-------------------------------------------")
    // 逐个显示文件 + 大小
    for fpath in lines {
        if fpath.trim().len() == 0 { continue }
        var size = system("du -h \"" + fpath.trim() + "\" 2>/dev/null | cut -f1 || echo ?")
        print("  " + size.trim() + "  " + fpath.trim())
    }
    print("-------------------------------------------")
    print("")

    // 确认删除
    print("确认删除以上文件? 输入 y 删除,其他取消:")
    var confirm = system("read -r resp; echo $resp")
    if confirm.trim() != "y" {
        print("已取消")
        return
    }

    var deleted = 0
    var failed = 0
    for fpath in lines {
        if fpath.trim().len() == 0 { continue }
        var rm_out = system("rm -f \"" + fpath.trim() + "\" 2>/dev/null")
        if system_status() == 0 {
            deleted = deleted + 1
        } else {
            failed = failed + 1
        }
    }
    print("✓ 已删除 " + deleted.str() + " 个, 失败 " + failed.str() + " 个")
}

main()
