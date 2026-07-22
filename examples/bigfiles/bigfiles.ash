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

    // 从命令行参数读取(通过 shell bridge)
    var args = system("echo $@")
    if args.len() > 0 {
        // 简单参数处理:第一个非空 token 是目录,第二个是数量
        var parts = args.trim().split(" ")
        if parts.len() > 0 && parts[0].len() > 0 {
            dir = parts[0]
        }
        if parts.len() > 1 && parts[1].len() > 0 {
            count = parts[1]
        }
    }

    print("查找 " + dir + " 下最大的 " + count + " 个文件:")
    print("-----------------------------------")

    // 核心:一行结构化 pipeline
    // ls 输出结构化数据 → sort .size 按大小排序 → head 取前 N
    // ash 的 sort .size 直接按语义字段排序,不需要 bash 的 sort -rn + cut
    var cmd = "ls -la " + dir + " | filter .type == \"file\" | sort .size descending | head -n " + count
    > $cmd

    print("-----------------------------------")
    print("提示: 加 | select name size 只看文件名和大小")
    print("提示: 加 | to_json 输出为 JSON")
}

main()
