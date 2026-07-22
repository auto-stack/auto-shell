// examples/jq-like/jq-like.ash
// JSON 查询:用 ash 原生 from_json/to_json pipeline 取代 jq。
// 展示: 结构化 pipeline(filter / select / sort)——无需安装 jq。
//
// 用法: ash jq-like.ash <文件.json> [字段名]
// 例: ash jq-like.ash data.json name
//     ash jq-like.ash data.json

fn main() {
    var file = system("echo $1").trim()
    var field = system("echo $2").trim()

    if file.len() == 0 {
        print("用法: ash jq-like.ash <文件.json> [字段名]")
        print("展示 ash 原生 JSON pipeline,无需 jq")
        exit(1)
    }

    if field.len() == 0 { field = "name" }

    print("=== JSON 查询 (ash 原生 pipeline, 无 jq) ===")
    print("文件: " + file)
    print("")

    // 核心思想:bash 要 `cat file | jq '.field'`,
    // ash 直接 `cat file | from_json | select .field`
    // from_json 把 JSON 转成结构化 Table,后续可用 filter/select/sort
    // 注:pipeline 放在 system("...") 里执行(> 行内不支持 | )
    print("--- 1. 完整 JSON ---")
    var full = system("cat \"" + file + "\" | from_json")
    if full.trim().len() > 0 { print(full.trim()) }

    print("")
    print("--- 2. 只取 " + field + " 字段 ---")
    var sel = system("cat \"" + file + "\" | from_json | select " + field)
    if sel.trim().len() > 0 { print(sel.trim()) }

    print("")
    print("--- 3. 过滤 + 重新输出为 JSON ---")
    var filt = system("cat \"" + file + "\" | from_json | filter ." + field + " != \"\" | to_json")
    if filt.trim().len() > 0 { print(filt.trim()) }

    print("")
    print("✓ 完成。bash 对照: cat file | jq '.[] | {field: .field}'")
}

main()
