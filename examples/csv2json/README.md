# csv2json —— CSV 转 JSON

## 运行

```bash
# 转换并打印到终端
ash examples/csv2json/csv2json.ash data.csv

# 写入文件
ash examples/csv2json/csv2json.ash data.csv out.json
```

## ash 版本亮点

- 用 `cat file | from_csv | to_json` 原生 pipeline,一行完成格式转换
- 无需安装 csvkit 或写 Python 脚本
- 支持输出到终端或重定向到文件

## bash 对照

```bash
# bash:要装 csvkit,或写一段 Python
csvjson data.csv > out.json
```

bash 的问题:依赖外部工具、格式转换不是一等公民。ash 内置 from_csv/to_json pipeline。

## ash 脚本

见 [csv2json.ash](csv2json.ash)

## 依赖

- ash v0.5+(内置 from_csv/to_json 转换器)
