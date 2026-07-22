# Plan 034: ASH AutoLang 脚本实例库设计

> **日期**: 2026-07-21
> **状态**: 设计中(待评审)
> **战略驱动**: 用实例证明 ash 的 AutoLang 脚本远比 bash 强。同时服务采用(给 README 提供素材)、验证 AutoLang(发现痛点)、为 SmartCommand 积累 body.ash 模板
> **范围**: `examples/` 扩充到 30+ 脚本,每个对照 bash,bash→ash 速查表
> **预估**: 1-2 周(纯写作 + 测试,无新代码)

---

## 愿景

> **30+ 真实脚本实例,每个对照 bash 版本,展示 ash 的结构化优势**。这不是代码开发,是"证明 AutoLang 值得用"的营销 + 验证素材。实例同时是 SmartCommand body.ash 的模板库(029 可复用)。

### 现状

- `examples/deploy.ash` 是唯一实例(MS3 demo,含 fn + while + try/catch + system/export/exit)
- `default_ashrc.txt` 有 4 个小函数示例(greet/countdown/banner/host_info)
- 无 bash→ash 对照,无速查表

### 范围内 / 范围外

| 范畴 | 包含 | 不包含 |
|---|---|---|
| **实例** | 30+ 脚本,分 6 类 | 完整应用级脚本 |
| **对照** | 每个有 bash 对照(或说明 ash 独有) | 性能基准 |
| **速查表** | `docs/bash-to-ash.md`(bash→ash 命令/语法映射) | 完整迁移指南 |
| **测试** | 每个实例可执行(cargo test 或 .at 测试) | CI 集成(留后续) |

---

## 第 1 节:实例分类与清单(30 个)

### 1.1 文件操作(6 个)
1. **批量重命名** —— `rename.ash`(给目录下所有 `.jpeg` 改 `.jpg`)
2. **查找大文件** —— `bigfiles.ash`(`ls | sort .size | head` 的脚本版)
3. **清理临时文件** —— `cleanup.ash`(找 + 删 `*.tmp`,带确认)
4. **同步目录** —— `synctree.ash`(源 → 目标,只复制更新的)
5. **文件类型统计** —— `filestats.ash`(按扩展名分组统计)
6. **目录大小排行** —— `du-top.ash`(`du | sort | head`)

### 1.2 文本处理(5 个)
7. **日志提取** —— `loggrep.ash`(grep ERROR + 上下文 + 时间过滤)
8. **CSV 汇总** —— `csvsum.ash`(读 CSV → 按列分组求和 → 输出汇总)
9. **JSON 查询** —— `jq-like.ash`(`from_json | filter | select | to_json`)
10. **文本替换** —— `batch-replace.ash`(跨多文件搜索替换)
11. **行数统计** —— `loccount.ash`(按语言统计代码行)

### 1.3 开发工具(6 个)
12. **构建+测试** —— `buildtest.ash`(build → 失败中止 → test → 报告)
13. **Git 批量操作** —— `git-batch.ash`(跨多仓库 git pull/status)
14. **依赖检查** —— `deps-check.ash`(检查 Cargo.toml/npm 依赖更新)
15. **代码格式化** —— `fmt-check.ash`(跑 rustfmt/prettier,报告未格式化文件)
16. **环境切换** —— `switch-env.ash`(切 .env 文件 + 验证)
17. **版本号更新** —— `bump-version.ash`(跨文件更新版本号)

### 1.4 系统管理(5 个)
18. **进程监控** —— `watch-proc.ash`(监控某进程,CPU>80% 报警)
19. **磁盘清理** —— `disk-clean.ash`(找 >100M 文件,列清单,确认删)
20. **服务状态** —— `svc-status.ash`(检查一组服务端口)
21. **用户活动** —— `user-activity.ash`(谁登录了,做了什么)
22. **定时任务清单** —— `cron-list.ash`(解析 crontab,人类可读输出)

### 1.5 数据处理(5 个,展示 Plan 031 lazy)
23. **大日志分析** —— `biglog.ash`(流式处理 GB 级日志,lazy pipeline)
24. **CSV → JSON** —— `csv2json.ash`(格式转换,pipeline)
25. **数据去重** —— `dedupe.ash`(按多字段去重)
26. **Top N** —— `topn.ash`(分组后每组取 Top N)
27. **数据校验** —— `validate.ash`(校验 CSV 字段完整性)

### 1.6 AI 增强(3 个,展示 Plan 029 SmartCommand)
28. **智能提交** —— `git.finish-worktree`(029 的首个 SmartCommand)
29. **部署助手** —— `deploy.ash` 升级版(AI 生成 release notes)
30. **日志诊断** —— `diagnose.ash`(AI 分析错误日志,给修复建议)

---

## 第 2 节:每个实例的结构

每个实例是一个**目录**,含:

```
examples/
└── bigfiles/
    ├── README.md       # 说明 + bash 对照 + 运行方式
    ├── bigfiles.ash    # ash 脚本
    └── bigfiles.bash   # bash 对照(可选,展示 ash 更简洁)
```

### README.md 模板

```markdown
# bigfiles —— 查找目录下最大的 N 个文件

## 运行
    ash bigfiles/bigfiles.ash /path/to/dir 10

## ash 版本亮点
- 用 `ls | sort .size | head` 结构化 pipeline,不需 awk/sort 管道拼接
- 输出是结构化 Table(可进一步 `| to_json`)

## bash 对照
bash 版需 `du -a | sort -rn | head -n 10 | cut -f2`——四段管道 + 文本解析。
ash 版一行 pipeline,输出结构化。

## ash 脚本
(贴 bigfiles.ash 内容)

## 依赖
- ash v0.5+
- Plan 031 lazy pipeline(大目录时)
```

---

## 第 3 节:bash→ash 速查表

`docs/bash-to-ash.md` 结构:

### 3.1 命令映射

| bash | ash | 说明 |
|---|---|---|
| `ls -la` | `ls -la` | 相同 |
| `find . -name "*.rs"` | `find . -name "*.rs"` 或 `glob "**/*.rs"` | ash 有 glob |
| `grep -r "TODO" .` | `grep -r "TODO" .` | 相同 |
| `du -a \| sort -rn \| head` | `ls \| sort .size \| head` | ash 结构化 |
| `cat file \| jq '.field'` | `cat file \| from_json \| select .field` | ash 原生 |
| `wc -l file` | `wc -l file` 或 `wc file \| select .lines` | ash 结构化输出 |

### 3.2 语法映射

| bash | ash (AutoLang) | 说明 |
|---|---|---|
| `if [ ... ]; then ...; fi` | `if cond { ... }` | AutoLang 语法 |
| `for f in *.txt; do ...; done` | `for f in glob("*.txt") { ... }` | AutoLang |
| `var=$(command)` | `var = system("command")` | shell bridge |
| `export VAR=val` | `export("VAR", "val")` | shell bridge |
| `$1 $2 $@` | `args[0] args[1] args` | AutoLang 参数 |
| `function name() { ... }` | `fn name() { ... }` | AutoLang 函数 |
| `command && command2` | `if system("command") == 0 { system("command2") }` | 显式 |
| `command \| command2` | `command \| command2` | 相同(管道) |

### 3.3 ash 独有特性(bash 没有)

- 结构化 pipeline(`ls | filter .size > 10.mb | sort .name`)
- Atom 类型系统(18 种语义标签)
- try/catch 错误处理
- AutoLang 完整编程能力(闭包、递归、数据结构)
- SmartCommand(AI 增强命令)
- 内置 from_json/to_json/csv/yaml/xml/toml 转换
- 安全沙箱(--sandbox/--read-only/--no-network)

---

## 第 4 节:里程碑

### M0:基础设施 + 速查表(2-3 天)
- `examples/` 目录结构 + README 模板
- `docs/bash-to-ash.md` 速查表(v1)
- 1-2 个示范实例(bigfiles + loggrep)

### M1:核心实例(5-7 天)
- 文件操作 6 个 + 文本处理 5 个 + 开发工具 6 个
- 每个可执行 + 有 README

### M2:高级实例(3-5 天)
- 系统管理 5 个 + 数据处理 5 个(部分依赖 Plan 031 lazy)
- AI 增强实例 3 个(依赖 Plan 029)

### M3:集成(1-2 天)
- 主 README 链接到实例库
- CI 跑实例测试(每个 .ash 能执行)

**总计**:约 2 周(含测试)。

---

## 第 5 节:跟其他方向的关系

| 方向 | 关系 |
|---|---|
| **Plan 029**(AI) | AI 实例(#28-30)展示 SmartCommand;实例是 body.ash 模板源 |
| **Plan 031**(数据处理) | 数据处理实例(#23-27)展示 lazy pipeline |
| **方向 A**(文档+分发) | 实例是 README/quickstart 的素材;速查表是上手桥 |
| **Plan 033**(插件) | 实例可打包成插件分发 |
| **Plan 032**(补全) | 实例脚本里的函数可被补全系统发现 |

---

## 附录:现有资产

- `examples/deploy.ash` —— MS3 demo(fn + while + try/catch + system/export/exit),作为 #29 部署助手的基础
- `ash/auto-shell/src/default_ashrc.txt` —— 4 个小函数示例(greet/countdown/banner/host_info),作为函数贡献的范例
