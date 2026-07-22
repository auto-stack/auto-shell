// examples/buildtest/buildtest.ash
// 构建然后测试:build 失败就中止,不浪费跑测试的时间。
// 展示: 函数封装 + system_status() 检查退出码 + 失败即停
//
// 用法: ash buildtest.ash [release]
// 例: ash buildtest.ash          # cargo build && cargo test
//     ash buildtest.ash release  # cargo build --release

fn build(mode) {
    var cmd = "cargo build"
    if mode == "release" { cmd = "cargo build --release" }
    print("== [1/2] build (" + cmd + ") ==")
    var out = system(cmd + " 2>&1")
    if out.trim().len() > 0 { print(out) }
    var code = system_status()
    return code
}

fn test_all() {
    print("== [2/2] test (cargo test) ==")
    var out = system("cargo test 2>&1")
    if out.trim().len() > 0 { print(out) }
    return system_status()
}

fn main() {
    var mode = system("echo $1").trim()

    print("=== buildtest: build → test ===")
    print("")

    // 第一步:build,失败立刻中止
    var build_code = build(mode)
    if build_code != 0 {
        print("")
        print("✗ build 失败 (exit " + build_code.str() + "),跳过测试")
        exit(build_code)
    }
    print("✓ build 通过")
    print("")

    // 第二步:test
    var test_code = test_all()
    print("")
    if test_code == 0 {
        print("✓ 全部通过:build + test")
    } else {
        print("✗ 测试失败 (exit " + test_code.str() + ")")
    }
    exit(test_code)
}

main()
