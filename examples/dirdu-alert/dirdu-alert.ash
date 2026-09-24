// examples/dirdu-alert/dirdu-alert.ash
// 阈值告警：捕获 du 的记录数组，对超过阈值的目录逐条告警。
// 展示 PLAN-082 捕获形态：var rows = > 管道 得 Array<Obj>，
// rows.len() / rows[i].field / r.bytes.to_uint() 均可用——不再是 JSON 文本。
//
// 用法: ash dirdu-alert.ash [目录] [阈值MB，默认 100]

fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }
    var threshold = system("echo $2").trim().to_uint()
    if threshold == 0 { threshold = 100 }
    var limit = threshold * 1048576

    // 形态 2：捕获记录数组（du 记录 {path size bytes}）
    var rows = > du $dir | where path != total | where path != .
    if rows.len() == 0 {
        print("no subdirectories under " + dir)
        exit(1)
    }

    var alerts = 0
    for r in rows {
        if r.bytes.to_uint() > limit {
            print("OVER: " + r.path + " = " + r.size)
            alerts = alerts + 1
        }
    }
    print("checked " + rows.len() + " entries, " + alerts + " over " + threshold + "MB")
    exit(0)
}

main()
