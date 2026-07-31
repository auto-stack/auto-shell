# csvsum —— CSV 分组汇总

读取 CSV,按某列分组,对数值列求和,输出汇总表。展示 AutoLang HashMap 聚合 + 手动 CSV 解析(灵活可控)。

## 运行

```bash
# 按 region 列分组,对 amount 列求和
ash examples/csvsum/csvsum.ash sales.csv region amount

# 示例输入 sales.csv:
#   region,product,amount
#   east,widget,100
#   west,gadget,250
#   east,widget,180
```

## ash 版本亮点

- 用 HashMap 做 group-by + sum 聚合,逻辑显式、可调试
- 列索引自动解析(按表头名找列,不靠位置)
- 输出结构化汇总表,可改 `to_csv` / `to_json` 落盘
- 有参数校验 + 友好报错(缺列、空文件)

## bash 对照

```bash
# bash 需要 awk 脚本做分组求和(列索引硬编码、类型转换手动)
awk -F, -v g=$2 -v s=$3 '
NR==1 { for(i=1;i<=NF;i++){ if($i==g)gi=i; if($i==s)si=i } next }
{ totals[$gi]+=$si; counts[$gi]++ }
END { for(k in totals) print k, totals[k], counts[k] }
' sales.csv | column -t
```

bash 的问题:
- awk 脚本里列索引靠 `NR==1` 扫表头,逻辑晦涩
- 数值聚合、类型转换全塞进 awk 的 BEGIN/END 块
- 输出排序不稳定(awk 的 hash 遍历顺序不定),要额外 `| sort`
- 报错能力弱(列名拼错不提示)

ash 的做法:
- HashMap 聚合 + `for (key, val) in map` 遍历,逻辑平铺易读
- 列索引查找、空值、缺列都有显式校验
- 聚合逻辑在主流程里,加一行 `sort` 即可稳定排序

## ash 脚本

见 [csvsum.ash](csvsum.ash)

## 依赖

- ash v0.5+
