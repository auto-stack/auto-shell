// examples/bump-version/bump-version.ash
// 跨多文件更新版本号:在 Cargo.toml / package.json 等里同步版本。
// 展示: 多文件遍历 + 字符串匹配 + 修改计数 + dry-run
//
// 用法: ash bump-version.ash <新版本> [--dry-run]
// 例: ash bump-version.ash 1.2.0
//     ash bump-version.ash 0.3.1 --dry-run

fn bump_file(path, pattern, new_version, is_dry) {
    // pattern 形如 "version = \"" —— 匹配 version = "x.y.z" 或 "version": "x.y.z"
    var content = system("cat \"" + path + "\" 2>/dev/null || true")
    if content.trim().len() == 0 {
        return false  // 文件不存在或空
    }

    // 找当前版本号(引号包住的 x.y.z)
    var ppos = content.find(pattern)
    if ppos < 0 { return false }

    var after = content.sub(ppos + pattern.len(), content.len())
    var q1 = after.find("\"")
    if q1 < 0 { return false }
    var rest = after.sub(q1 + 1, after.len())
    var q2 = rest.find("\"")
    if q2 < 0 { return false }
    var old_version = rest.sub(0, q2)

    print("  " + path + ": " + old_version + " → " + new_version)

    if !is_dry {
        // 在 AutoLang 内存里替换(old_version -> new_version),再写回文件
        // 比 sed 跨平台、无引号转义地狱
        var new_content = content.replace(old_version, new_version)
        var write_cmd = "cat > \"" + path + "\" <<'ASH_EOF'\n" + new_content + "\nASH_EOF"
        var w = system(write_cmd)
    }
    return true
}

fn main() {
    var new_ver = system("echo $1").trim()
    var flag = system("echo $2").trim()

    if new_ver.len() == 0 {
        print("用法: ash bump-version.ash <新版本> [--dry-run]")
        print("例: ash bump-version.ash 1.2.0")
        exit(1)
    }

    var is_dry = flag.contains("dry")
    if is_dry { print("=== DRY RUN (不修改文件) ===") }

    print("=== 升级版本号到 " + new_ver + " ===")
    print("")

    // 各文件的 version 字段匹配模式
    var updated = 0

    if bump_file("Cargo.toml", "version = ", new_ver, is_dry) { updated = updated + 1 }
    if bump_file("package.json", "\"version\": ", new_ver, is_dry) { updated = updated + 1 }
    if bump_file("pyproject.toml", "version = ", new_ver, is_dry) { updated = updated + 1 }

    print("")
    if updated == 0 {
        print("⚠ 没有找到可更新的版本字段")
    } else {
        print("✓ 更新了 " + updated.str() + " 个文件到 " + new_ver)
        if !is_dry {
            print("提示: 别忘了 git diff 复查 + 提交")
        }
    }
}

main()
