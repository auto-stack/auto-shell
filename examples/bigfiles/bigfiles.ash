// examples/bigfiles/bigfiles.ash
//
// 找出指定目录下最大的 N 个文件。
// 展示 ash 的结构化 pipeline + AutoLang 函数封装。
//
// 用法: ash examples/bigfiles/bigfiles.ash [目录] [数量]
// 默认: 当前目录, 前 10 个

fn main() {
    // 解析参数
    var dir = "."
    var count = "10"

    // 从命令行参数读取(通过 shell bridge)。
    // NOTE: `system("echo $@")` 无参数时返回字面量 "$@"(auto-shell 已知
    // bug,见 Plan 034 附录 Bug 2),这里把它当空处理。
    var args = system("echo $@")
    if args.trim() == "$@" { args = "" }
    if args.len() > 0 {
        // 简单参数处理:第一个非空 token 是目录,第二个是数量
        // NOTE(v0.1.0): `&&` 不短路求值——`parts.len() > 1 && parts[1]...` 在
        // 只有一个参数时仍会求值 parts[1] 并抛 IndexError,所以用嵌套 if 做守卫。
        var parts = args.trim().split(" ")
        if parts.len() > 0 {
            if parts[0].len() > 0 {
                dir = parts[0]
            }
        }
        if parts.len() > 1 {
            if parts[1].len() > 0 {
                count = parts[1]
            }
        }
    }

    print("查找 " + dir + " 下最大的 " + count + " 个文件:")
    print("-----------------------------------")

    // 核心:一行结构化 pipeline
    //   ls 输出结构化记录 → where 按裸字段过滤 → sort .size 按语义字段排序
    //   → select 只留 name/size 两列
    // 注意(v0.1.0 实测):
    //   - `filter` 不是内建命令,写了会透传给系统 shell 报错;过滤用 `where`。
    //   - `head -n` 只吃文本行,不吃结构化记录——所以截断放到下面的 AutoLang 循环里。
    var table = system("ls " + dir + " | where type == file | sort .size descending | select name size")
    if table.trim().len() == 0 {
        print("(没有找到文件: " + dir + ")")
        exit(1)
    }

    var rows = table.trim().lines()
    // rows[0] 是表头,数据行从第二行开始;连表头一起打印 count+1 行 = 前 count 个文件
    var limit = count.to_int() + 1
    var shown = 0
    for row in rows {
        if shown < limit {
            print(row)
            shown = shown + 1
        }
    }

    print("-----------------------------------")
    print("提示: 只列文件名可改成 ... | each name | head -n " + count)
    print("提示: 加 | to_json 输出为 JSON")
    exit(0)
}

main()
