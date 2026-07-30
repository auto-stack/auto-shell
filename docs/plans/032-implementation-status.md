# Plan 032 实施状态记录

> **日期**: 2026-07-30
> **分支**: feat/032-intelligent-completion
> **设计**: `designs/032-intelligent-completion.md`
> **状态**: ✅ M0-M3 全部完成

## 总览

| 里程碑 | 内容 | 状态 | 测试 |
|---|---|---|---|
| **M0** | 上下文 plumbing(CompletionContext + history + CompletionState) | ✅ | 10 |
| **M1** | 上下文排序 + 历史 fuzzy ghost-text | ✅ | 11 |
| **M2** | AI 补全层(LLM 子命令 + NL→pipeline) | ✅ | 12 |
| **M3** | 缺失动态源(ssh hosts + 真实 env vars) | ✅ | 8 |

**测试**:ash-core 395(+9)、auto-shell 737(+17),零回归。

## M0:上下文 plumbing ✅

- **M0.1** `CompletionContext`(ash-core/provider.rs)加 `last_command` / `last_exit_code` / `history` / `aliases` 字段 + `::new()` 便捷构造器(默认空)。
- **M0.2** `read_recent_history(path, n)`(frontend/repl.rs)——有界历史读取(默认 50),供补全上下文用。
- **M0.3** `CompletionState`(frontend/completions_reedline.rs)镜像新字段;`sync_completion_state` 从 Shell 访问器(029 §2.3 已落地)+ 最近历史填充;`ShellCompleter::complete` 一次性快照整个 state 构造 ctx。

## M1:上下文排序 + 历史 ghost-text ✅

- **M1.1** `context_rank::rank`(ash-core/context_rank.rs)——纯本地启发式,零 AI 零延迟。按历史频率(+0.5/次)、git 仓库上下文(+2.0)、命令连贯性(+1.0)重排。集成进 `ShellCompleter` 的 fallback 路径,**仅在命令名补全位**排序(不打乱子命令/flag/文件顺序)。
- **M1.2** `AshHinter`(frontend/term/hinter.rs)替代 reedline `CwdAwareHinter`。保留完全一致的前缀匹配行为,新增 **prefix-subsequence fuzzy 回退**(`gcm` → `git commit -m`)。不调 AI(实时 ghost-text 必须本地,LLM 延迟会拖慢光标)。

## M2:AI 补全层 ✅

- **M2.1/M2.2** `ai_layer` 模块(completions/ai_layer.rs)——后台线程 + 静态缓存模式(镜像 suggest.rs):
  - `trigger_ai_subcommand`(tier:min/Ollama,**500ms 超时**,prefix 过滤)
  - `trigger_nl_to_pipeline`(复用 ask_ai 的 system prompt + context block)
  - `take_ai_pending(line)` 按触发时的 line 快照 key 取结果(take 语义,line 不匹配则丢弃,避免跨按键泄漏)
- **M2.3** 集成进 `ShellCompleter::complete`:AI 候选以**滞后一次按键**合并(reedline `Completer::complete` 是同步的,无 async hook)。静态 spec 不足(<3 候选)时 fire 子命令补全;首 token 未知命令时 fire NL 翻译。默认开(`ai.completion` config,默认 true),因为降级完善:**无 daemon → 后台线程写空 → 纯 Plan 021 静态/动态行为**。

### 关键工程决策:AI 异步刷新策略

reedline `Completer::complete` 纯同步无 async 入口(已勘探确认 reedline 0.44 无 async 依赖)。采用"滞后一次按键"策略:本次按键 `complete()` 先返回静态结果 + fire 后台 AI 线程,**下一次按键**的 `complete()` 读缓存返回 AI 结果。零额外依赖,最低风险。

## M3:缺失动态源 ✅

- **M3.1** ssh/scp destination 参数现在补全 hosts(definitions/ssh.rs):纯 Rust 解析 `~/.ssh/config`(Host 别名)+ `~/.ssh/known_hosts`(首列),跨平台无 shell-out。过滤通配符(`*`/`?`/`!`)和 hashed 条目。顺手修复了一个既有 bug flag(把 `-o StrictHostKeyChecking=no` 编码成了 long flag 名)。
- **M3.2** `complete_auto`(ash-core/auto.rs)从硬编码 11 项改为真实环境变量(`std::env::vars`)+ fallback 列表,去重。

## 成功指标对照(design §6.5)

1. ✅ **M0**:CompletionContext 携带 last_command/exit_code/history(测试验证)
2. ✅ **M1**:git 仓库下 git 命令排前;fuzzy ghost-text 工作(`gcm` 匹配 `git commit -m`)
3. 🟡 **M2**:静态 spec 没有的子命令 AI 补(代码就绪 + 降级路径测试通过;端到端需运行中的 Ollama daemon);NL 输入翻译成 pipeline(同上)
4. ✅ **M3**:ssh hosts 补全工作;env var 用真实环境
5. ✅ **降级正确**:无 daemon 时 `complete()` 不 panic、返回静态结果(`complete_does_not_panic_without_daemon` 测试覆盖)

## 影响文件

- 新增:`ash-core/src/completions/context_rank.rs`
- 新增:`ash/auto-shell/src/completions/ai_layer.rs`
- 新增:`ash/auto-shell/src/frontend/term/hinter.rs`
- 改:`ash-core/src/completions/{provider.rs, auto.rs, mod.rs}`
- 改:`ash/auto-shell/src/frontend/{completions_reedline.rs, repl.rs}`
- 改:`ash/auto-shell/src/frontend/term/mod.rs`
- 改:`ash/auto-shell/src/completions.rs`、`completions/definitions/ssh.rs`

## 非目标(沿用 design §6.4)

- ❌ 命令后建议(💡)—— 029 §7.3 suggest.rs 已实现
- ❌ AI 实时 ghost-text(打字时调 LLM)—— 延迟不可接受
- ❌ 重写 Plan 021 引擎 —— 只在其后加层
- ❌ 云端 LLM 补全 —— 只用本地 Ollama
