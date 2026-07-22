// examples/smart-commit/smart-commit.ash
// 智能提交:汇总当前改动,生成结构化提交信息,确认后提交。
// (Plan 029 git.finish-worktree SmartCommand 的简化版;此处用规则生成 message,
//  后续接入 AI 后自动写 release notes 风格的描述。)
// 展示: git status/diff 解析 + 改动分类 + 消息模板 + 交互确认
//
// 用法: ash smart-commit.ash [--push]
// 例: ash smart-commit.ash          # 暂存全部 + 提交
//     ash smart-commit.ash --push   # 提交并推送

fn summarize_changes() {
    // 取改动文件列表,按状态分组
    var status = system("git status --porcelain 2>/dev/null || true")
    if status.trim().len() == 0 {
        return ""
    }

    var lines = status.trim().lines()
    var added = List.new()
    var modified = List.new()
    var deleted = List.new()

    for line in lines {
        if line.len() < 3 { continue }
        var flag = line.sub(0, 2).trim()
        var path = line.sub(3, line.len()).trim()
        if flag == "??" || flag == "A" {
            added.push(path)
        } else if flag == "D" {
            deleted.push(path)
        } else {
            modified.push(path)
        }
    }

    // 拼成 message 摘要
    var summary = ""
    if !added.is_empty() {
        summary = summary + "add " + added.len().str() + " files"
    }
    if !modified.is_empty() {
        if summary.len() > 0 { summary = summary + ", " }
        summary = summary + "update " + modified.len().str() + " files"
    }
    if !deleted.is_empty() {
        if summary.len() > 0 { summary = summary + ", " }
        summary = summary + "remove " + deleted.len().str() + " files"
    }
    return summary
}

fn main() {
    var flag = system("echo $1").trim()
    var do_push = flag.contains("push")

    print("=== smart-commit ===")
    print("")

    // 检查是否有改动
    var summary = summarize_changes()
    if summary.len() == 0 {
        print("✓ 没有待提交的改动")
        return
    }

    print("检测到改动: " + summary)
    print("")

    // 显示 diff 概要(行数级)
    var diffstat = system("git diff --stat HEAD 2>/dev/null || true")
    if diffstat.trim().len() > 0 {
        print("--- 改动概要 ---")
        print(diffstat.trim())
        print("")
    }

    // 生成提交信息(规则版;后续 Plan 029 接 AI 升级)
    var message = "chore: " + summary
    print("建议提交信息:")
    print("  \"" + message + "\"")
    print("")
    print("确认提交? y/n:")
    var confirm = system("read -r resp; echo $resp")
    if confirm.trim() != "y" {
        print("已取消")
        return
    }

    // 暂存全部 + 提交(用 system 执行;> 行不能出现在 fn 内)
    var add_out = system("git add -A 2>&1")
    var commit_out = system("git commit -m \"" + message + "\" 2>&1")
    print(commit_out.trim())

    if do_push {
        print("")
        print("--- push ---")
        var push_out = system("git push 2>&1")
        print(push_out.trim())
    }

    print("")
    print("✓ 完成 (后续 Plan 029 接 AI 自动写 release notes)")
}

main()
