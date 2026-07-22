// examples/git-batch/git-batch.ash
// 跨多仓库批量 git 操作:遍历同级目录里的 git 仓库,统一 pull / status。
// 展示: find/ls 遍历目录 + for 循环 + system + 结果汇总
//
// 用法: ash git-batch.ash [父目录] [操作]
// 例: ash git-batch.ash ~/projects pull
//     ash git-batch.ash ~/code status

fn repo_action(repo, action) {
    print("── " + repo + " ──")
    var cmd = "git -C " + repo
    if action == "pull" {
        cmd = cmd + " pull --ff-only 2>&1"
    } else if action == "status" {
        cmd = cmd + " status -sb 2>&1"
    } else if action == "fetch" {
        cmd = cmd + " fetch --all 2>&1"
    } else {
        cmd = cmd + " " + action + " 2>&1"
    }
    var out = system(cmd)
    if out.trim().len() > 0 { print(out.trim()) }
    return system_status()
}

fn main() {
    var parent = system("echo $1").trim()
    var action = system("echo $2").trim()

    if parent.len() == 0 { parent = "." }
    if action.len() == 0 { action = "status" }

    print("=== git-batch: 在 " + parent + " 下批量 " + action + " ===")
    print("")

    // 找出 parent 下所有含 .git 的子目录(即 git 仓库)
    var repos = system("find " + parent + " -maxdepth 2 -name \".git\" -type d 2>/dev/null | sed 's#/.git##' | sort || true")
    var lines = repos.trim().lines()

    if lines.len() == 0 {
        print("没有找到 git 仓库(在 " + parent + " 下)")
        exit(1)
    }

    var ok = 0
    var fail = 0
    for repo in lines {
        if repo.trim().len() == 0 { continue }
        var code = repo_action(repo.trim(), action)
        if code == 0 {
            ok = ok + 1
        } else {
            fail = fail + 1
            print("  ⚠ 退出码 " + code.str())
        }
        print("")
    }

    print("=== 汇总: " + ok.str() + " 成功, " + fail.str() + " 失败 ===")
}

main()
