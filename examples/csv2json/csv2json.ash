// examples/csv2json/csv2json.ash
// CSV 转 JSON:用 ash 原生 from_csv | to_json pipeline。
// 展示: 结构化 pipeline(格式转换一行搞定,无需 Python/jq)
//
// 用法: ash csv2json.ash <文件.csv> [输出.json]
// 例: ash csv2json.ash data.csv
//     ash csv2json.ash data.csv out.json

fn main() {
    var input = system("echo $1").trim()
    var output = system("echo $2").trim()

    if input.len() == 0 {
        print("用法: ash csv2json.ash <文件.csv> [输出.json]")
        print("展示 ash 原生 from_csv | to_json pipeline")
        exit(1)
    }

    print("=== CSV → JSON ===")
    print("输入: " + input)
    if output.len() > 0 {
        print("输出: " + output)
    }
    print("")

    // 核心:bash 要装 csvkit 或写 Python,
    // ash 直接 from_csv | to_json 一行 pipeline
    // 注:pipeline 放在 system("...") 里执行(> 行内不支持 | )
    if output.len() == 0 {
        // 直接打印到终端
        print("--- 转换结果 ---")
        var json = system("cat \"" + input + "\" | from_csv | to_json")
        if json.trim().len() > 0 { print(json.trim()) }
    } else {
        // 写入文件(> 行用于重定向,转换逻辑仍在 system 里)
        print("--- 写入 " + output + " ---")
        var json = system("cat \"" + input + "\" | from_csv | to_json")
        var write_out = system("echo '" + json + "' > \"" + output + "\"")
        print("✓ 已转换并写入 " + output)
    }

    print("")
    print("bash 对照: python -c 'import csv,json;...' 或装 csvkit")
}

main()
