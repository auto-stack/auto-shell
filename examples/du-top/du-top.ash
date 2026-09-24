// examples/du-top/du-top.ash
// 目录大小排行：显示当前目录下各子目录的磁盘占用，最大在前。
// 展示: 结构化 pipeline(du 输出记录) + AutoLang 参数处理
//
// 用法: ash du-top.ash [目录]
// 实测: ash v0.1.0 (2026-09-24)，du 为内置命令，输出 {path size bytes} 记录

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }

    // du 记录含根目录 "." 和 "total" 行，先 where 掉再取需要的字段。
    // 排序 du 内部已按 bytes 降序完成，无需再 sort。
    // 注意: 短标志 -h 被 du 的 help 占用(v0.1.0 实测)，人类可读要用长标志。
    var out = system("du --human-readable " + dir + " | where path != total | where path != . | select path size | to_json")
    if out.trim().len() == 0 {
        print("no subdirectories under " + dir)
        exit(1)
    }

    // system() 返回 JSON 文本——结构化变换在管道里做，AutoLang 只做呈现。
    // (AutoLang 尚无 from_json 内建，脚本侧拿到的是字符串。)
    print(out)
    exit(0)
}

main()
