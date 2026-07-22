# jq-like —— 用 ash 原生 pipeline 取代 jq

## 运行

```bash
# 取某个字段
ash examples/jq-like/jq-like.ash data.json name

# 完整查询
ash examples/jq-like/jq-like.ash data.json
```

## ash 版本亮点

- 用 `cat file | from_json | select .field` 原生 pipeline,无需安装 jq
- `from_json` 把 JSON 转结构化 Table,后续可 `filter`/`select`/`sort`/`to_json`
- AutoLang 包装:参数处理 + 多步查询组合

## bash 对照

```bash
# bash:必须装 jq,语法 .[] | {f: .f} 易错
cat data.json | jq '.[] | {name: .name}'
```

ash 版直接语义化 pipeline,JSON 是一等公民。

## ash 脚本

见 [jq-like.ash](jq-like.ash)

## 依赖

- ash v0.5+(内置 from_json/to_json 转换器)
