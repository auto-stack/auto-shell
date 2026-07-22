// examples/switch-env/switch-env.ash
// 切换 .env 文件:把 .env.<环境> 复制成 .env,并校验必填变量。
// 展示: 文件复制 + 内容校验 + try/catch 容错
//
// 用法: ash switch-env.ash <环境>
// 例: ash switch-env.ash production
//     ash switch-env.ash staging

fn validate_env(content) {
    // 必填的 key 列表(按项目调整)
    var required = ["DATABASE_URL", "API_KEY", "SECRET_KEY"]
    var missing = List.new()

    for key in required {
        // 检查是否以 key= 开头(忽略注释行)
        if !content.contains(key + "=") {
            missing.push(key)
        }
    }
    return missing
}

fn main() {
    var env = system("echo $1").trim()

    if env.len() == 0 {
        print("用法: ash switch-env.ash <环境>")
        print("例: ash switch-env.ash production")
        print("")
        print("可用环境:")
        var envs = system("ls -1 .env.* 2>/dev/null | sed 's/.env.//' || echo (none)")
        if envs.trim().len() > 0 { print(envs.trim()) }
        exit(1)
    }

    var src = ".env." + env
    print("=== 切换环境 → " + env + " ===")

    try {
        // 校验源文件存在
        var check = system("test -f \"" + src + "\" && echo ok || echo no")
        if check.trim() == "no" {
            print("✗ 找不到 " + src)
            exit(1)
        }

        // 读内容并校验必填项
        var content = system("cat \"" + src + "\"")
        var missing = validate_env(content)

        if !missing.is_empty() {
            print("✗ " + src + " 缺少必填变量:")
            for k in missing {
                print("    - " + k)
            }
            print("请补全后再切换")
            exit(1)
        }

        // 备份当前 .env(system 里执行,避免 > 行的 && 转义问题)
        var backup = system("test -f .env && cp .env .env.backup || true")

        // 复制并生效
        var copy = system("cp \"" + src + "\" .env")
        print("✓ 已切换: " + src + " → .env")
        print("  (旧 .env 备份到 .env.backup)")

        // 把当前环境名导出到 shell
        export("APP_ENV", env)
        print("✓ export APP_ENV=" + env)
    } catch(e) {
        print("✗ 切换失败: " + e)
        exit(1)
    }
}

main()
