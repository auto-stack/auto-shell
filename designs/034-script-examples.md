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

---

## 附录 B:已知 VM Bug 记录(2026-07-23 调查)

在编写 30 个实例时发现 5 个运行时 bug。以下为根因分析和修复方案。

### Bug 1:`.to_uint()` 返回垃圾值且算术错误 —— **auto-lang 仓库**

**症状**:`"42".to_uint()` 返回 `0-2147483647` 之类的垃圾值。`"5".to_uint() + 3` 给出错误结果。但 `var x = 0; x = x + 1` 正常。

**根因**:`codegen.rs` 的 `contains_u64`(line 8772)和 `is_u64_expr`(line 8743)在处理 `Expr::Call` 时,只检查 `Expr::Ident`(函数名)形式的调用,**不处理 `Expr::Dot`(方法调用)**。所以 `"42".to_uint()` 的返回类型被误判为 I32(1 slot),而实际 native 返回 I64(2 slot),导致栈对齐错乱。

**影响**:所有返回 I64/U64 的实例方法(`.to_uint()`、`.len()` 等)在算术/打印中都会出错。影响 csvsum、filestats、loccount、dedupe、diagnose、disk-clean、biglog 等实例。

**修复**:
- `codegen.rs` `contains_u64` 的 `Expr::Call` 分支:增加对 `Expr::Dot` 方法调用的处理
- `is_u64_expr`:同样增加 Dot 分支
- 启动时从 native_catalog 填充 `fn_return_types`,让 I64 返回的 native 被正确识别
- 范围:~30 行,3 个函数

### Bug 2:位置参数 `$1`/`$@` 不传递给脚本 —— **auto-shell 仓库**

**症状**:`ash script.ash hello world` → 脚本内 `system("echo $1")` 返回空字符串。

**根因**:`main.rs:184` 收集了脚本路径但 **从未传递 `args[i+1..]`**。`execute_script_file` 和 `execute_script_content` 无 args 参数。Shell 无 `set_script_args` 机制。

**修复**:
- `shell.rs`:Shell 加 `script_args: Vec<String>` 字段 + `pub fn set_script_args(&mut self, args: Vec<String>)`
- `main.rs:184`:收集 `args[i+1..]` 并调 `shell.set_script_args()`
- `interpolate_auto_vars()`:扩展 `$1`/`$@`/`$#` 查找,优先查 script_args
- 范围:~30 行

### Bug 3:`> cmd` shell 行不能出现在 `fn` 体内 —— **auto-shell 仓库**

**症状**:
```
fn foo() {
    > ls -la      // ← 解析错误:"unexpected token"
}
```
但 `var x = system("ls -la")` 在 fn 内正常。`> ls -la` 在顶层正常。

**根因**:`execute_script_content`(shell.rs:2306)的预处理器**不跟踪大括号深度**。遇到 `>` 行时无条件 `flush_auto_block`,把不完整的 `fn foo() {` 发给 VM,导致解析失败。

**修复**:
- 在 `execute_script_content` 的行循环中跟踪大括号深度(count `{` / `}`)
- 当 `>` 行遇到且深度 > 0(在 fn/if/for 体内)时,重写为 `system("cmd")` 注入 auto_block,而非 flush+execute
- 范围:~25 行

### Bug 4:`var x = > cmd` 捕获语法解析错误 —— **auto-shell 仓库**

**症状**:`var result = > git rev-parse HEAD` 给出 "Expected term, got RBrace"。只有 `var result = system("git rev-parse HEAD")` 能用。

**根因**:`try_capture_assignment`(shell.rs:2513)要求精确的 `"= >"` 子串(`rest.find("= >")`)。空格变体(`=>`、`=  >`、`= >cmd`)不匹配,返回 None,原始 `>` 字符被当作 AutoLang token 送进 parser。

**修复**:
- 把 `rest.find("= >")` 改为更宽松的匹配:手动扫描 `=` 后跟(可选空格)`>`,允许任意空格
- 范围:~15 行

### Bug 5:`cat file | from_json` 管道在运行时失败 —— **auto-shell 仓库**

**症状**:`cat data.json | from_json` 给出 "unexpected end of input"。from_json 独立使用正常。

**根因**:`from_json.rs:37-46` 的 `run_atom` 经过 lossy 的 `atom_to_pipeline_data` 桥接。`ExternalStream` 的 `unwrap_or_default()`(pipeline_convert.rs:45)吞掉读取错误,返回空字符串 → `parse_json("")` → "unexpected end of input"。

**修复**:
- `from_json.rs` 的 `run_atom`:不经过 bridge,直接调 `input.into_text()` + `parse_json`
- 同样的模式也影响 from_csv/from_yaml/from_toml/from_xml(相同 run_atom 结构),v1 只修 from_json 验证模式
- 范围:~10 行

### Bug 影响矩阵

| Bug | 影响的实例 | 仓库 | 严重度 |
|-----|-----------|------|--------|
| 1 (.to_uint) | csvsum, filestats, loccount, dedupe, diagnose, disk-clean, biglog | auto-lang | 高(系统性) |
| 2 ($1 参数) | 几乎所有带参数的实例 | auto-shell | 高(功能缺失) |
| 3 (> in fn) | synctree, batch-rename, cleanup 等用 > 在 fn 内的 | auto-shell | 中(可避免) |
| 4 (var = >) | 少数用捕获语法的实例 | auto-shell | 低(有 system() 替代) |
| 5 (from_json pipe) | jq-like, csv2json | auto-shell | 中(核心功能) |
