# ASH 脚本实例库

30+ 个 AutoLang 脚本实例，展示 ash 的结构化 pipeline、AutoLang 编程能力、以及对照 bash 的优势。

> ⚠️ **注意**:部分实例依赖 `system()` 调用 GNU 工具(find/grep/du 等),而 ash 的这些命令是
> 内置重实现,参数语法与 GNU 不同(如 `find -n` 而非 `-name`)。详见
> [bash→ash 速查表](../docs/bash-to-ash.md)的注意事项,以及
> [design 034 附录 C](../designs/034-script-examples.md)的已知限制说明。

## 运行方式

```bash
# 在 ash REPL 里
ash> source examples/bigfiles/bigfiles.ash

# 或从命令行
ash examples/bigfiles/bigfiles.ash
```

## 实例分类

### 文件操作
| 实例 | 说明 |
|------|------|
| [bigfiles](bigfiles/) | 找出目录下最大的 N 个文件 |
| [batch-rename](batch-rename/) | 批量重命名(改扩展名) |
| [cleanup](cleanup/) | 清理临时文件(*.tmp/*.bak/*.log) |
| [synctree](synctree/) | 增量同步目录(只复制更新的) |
| [filestats](filestats/) | 按扩展名分组统计文件 |
| [du-top](du-top/) | 目录大小排行 |

### 文本处理
| 实例 | 说明 |
|------|------|
| [loggrep](loggrep/) | 日志提取（grep + 上下文 + 时间过滤） |
| [csvsum](csvsum/) | CSV 汇总（按列分组求和） |
| [jq-like](jq-like/) | JSON 查询(原生 from_json/to_json,无需 jq) |
| [batch-replace](batch-replace/) | 跨多文件搜索替换 |
| [loccount](loccount/) | 按语言统计代码行数 |

### 开发工具
| 实例 | 说明 |
|------|------|
| [buildtest](buildtest/) | 构建后测试,失败即停 |
| [deploy](deploy/) | 端到端部署流水线(MS3 demo,综合 fn/try-catch) |
| [git-batch](git-batch/) | 跨多仓库批量 git pull/status |
| [deps-check](deps-check/) | 解析 Cargo.toml 列出依赖 |
| [fmt-check](fmt-check/) | rustfmt 格式化检查 |
| [switch-env](switch-env/) | 切换 .env 文件并校验 |
| [bump-version](bump-version/) | 跨文件同步更新版本号 |

### 系统管理
| 实例 | 说明 |
|------|------|
| [watch-proc](watch-proc/) | 进程 CPU 监控告警 |
| [disk-clean](disk-clean/) | 找大文件并清理 |
| [svc-status](svc-status/) | 按端口检查服务状态 |
| [user-activity](user-activity/) | 查看用户登录活动 |
| [cron-list](cron-list/) | crontab 转人类可读 |

### 数据处理
| 实例 | 说明 |
|------|------|
| [biglog](biglog/) | 大日志流式分析(按级别统计) |
| [csv2json](csv2json/) | CSV 转 JSON(原生 pipeline) |
| [dedupe](dedupe/) | 按键列去重 |
| [topn](topn/) | 分组取 Top N |
| [validate](validate/) | CSV 字段校验 |

### AI 增强
| 实例 | 说明 |
|------|------|
| [smart-commit](smart-commit/) | 智能提交(规则版,后续接 Plan 029) |
| [deploy-ai](deploy-ai/) | AI 部署助手(release notes 占位版) |
| [diagnose](diagnose/) | 错误日志诊断(规则版) |

完整清单见 [designs/034-script-examples.md](../designs/034-script-examples.md)。

## 每个实例的结构

```
example-name/
├── README.md     # 说明 + bash 对照 + 运行方式
└── name.ash      # ash 脚本（可直接运行）
```

## ash vs bash 的核心差异

ash 脚本的优势在于**结构化数据**——命令输出不是文本流，是带类型的对象：

```bash
# bash：四段文本管道 + 字段号
du -a | sort -rn | head -10 | cut -f2

# ash：一行语义化 pipeline
ls | sort .size | head -n 10
```

更多对照见 [docs/bash-to-ash.md](../docs/bash-to-ash.md)。
