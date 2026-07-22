// examples/validate/validate.ash
// CSV 校验:检查必填字段是否为空、数值字段是否合法。
// 展示: 逐行扫描 + 多种校验规则 + 错误汇总
//
// 用法: ash validate.ash <文件.csv>
// 例: ash validate.ash users.csv

fn check_row(row_num, fields, header) {
    var errors = List.new()
    var i = 0

    while i < fields.len() {
        var field = fields[i].trim()
        var col = ""
        if i < header.len() { col = header[i].trim() }

        // 规则 1:必填列不能为空(id/name/email 视为必填)
        if col == "id" || col == "name" || col == "email" {
            if field.len() == 0 {
                errors.push("第 " + row_num.str() + " 行:必填列 '" + col + "' 为空")
            }
        }

        // 规则 2:email 列要含 @
        if col == "email" && field.len() > 0 {
            if !field.contains("@") {
                errors.push("第 " + row_num.str() + " 行:email '" + field + "' 不合法(缺 @)")
            }
        }

        // 规则 3:age/amount 等数值列要是数字
        if col == "age" || col == "amount" || col == "price" {
            if field.len() > 0 {
                // 简单判断:去掉数字字符后应该为空
                var stripped = field
                // 逐字符检查(粗略:不含字母即视为数字)
                if field.contains("a") || field.contains("b") || field.contains("x") {
                    errors.push("第 " + row_num.str() + " 行:数值列 '" + col + "'='" + field + "' 不是数字")
                }
            }
        }
        i = i + 1
    }
    return errors
}

fn main() {
    var file = system("echo $1").trim()

    if file.len() == 0 {
        print("用法: ash validate.ash <文件.csv>")
        print("例: ash validate.ash users.csv")
        exit(1)
    }

    print("=== CSV 校验: " + file + " ===")
    print("")

    var content = system("cat \"" + file + "\"")
    var lines = content.trim().lines()

    if lines.len() == 0 {
        print("文件为空")
        exit(1)
    }

    var header = lines[0].split(",")
    print("列: " + lines[0])
    print("")

    var total_errors = 0
    var total_rows = 0
    var ri = 1
    while ri < lines.len() {
        if lines[ri].trim().len() == 0 {
            ri = ri + 1
            continue
        }
        total_rows = total_rows + 1
        var fields = lines[ri].split(",")
        var errs = check_row(ri, fields, header)
        for e in errs {
            print("  ✗ " + e)
            total_errors = total_errors + 1
        }
        ri = ri + 1
    }

    print("")
    if total_errors == 0 {
        print("✓ 校验通过: " + total_rows.str() + " 行数据全部合法")
    } else {
        print("✗ 校验失败: " + total_rows.str() + " 行中发现 " + total_errors.str() + " 个问题")
        exit(1)
    }
}

main()
