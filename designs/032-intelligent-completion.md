# Plan 032: ASH 智能补全系统设计(AI 补全层 + 上下文 plumbing)

> **日期**: 2026-07-21
> **状态**: 设计中(待评审)
> **战略驱动**: 在 Plan 021/315 成熟的静态+动态补全引擎上,新增 AI 补全层(LLM 子命令/自然语言/上下文排序/历史 ghost-text),让 ash 的补全从"查表"进化为"理解意图"
> **范围**: CompletionContext 增强 + AI 补全引擎 + 缺失动态源 + reedline hinter 增强
> **预估**: M0-M3 共约 4-5 周(详见 §6)
> **跟 029 分工**: B 负责输入中补全(Tab/ghost-text),029 §7 负责命令后建议(💡提示)。无重叠。

---

## 愿景

> **ash 的补全从"查表+执行命令取候选"进化为"理解用户意图"**:LLM 补全子命令、自然语言翻译成 pipeline、历史驱动 ghost-text、上下文感知排序。静态(Plan 021)+ 动态(Command source)+ AI(本 Plan)三层叠加。

### 核心洞察:Plan 021/315 已经做了很多

探勘证实 Plan 021(代码里标 "Plan 315",同一份)的静态+动态层**非常成熟**:
- 静态:`CompletionSpec` 树 + 三层目录(user/generated/cache)+ help-probe + 5 内置 spec
- 动态:`CompletionSource::Command` 变体 + 5s TTL 缓存 + Line/Field 解析。git 分支/remotes/tags/改动文件、docker 容器/镜像已接上
- 路径:独立完整(tilde 展开 + prefix-subsequence fuzzy)
- ghost-text:reedline 的 `CwdAwareHinter`(历史前缀,Ctrl+F 接受)

**B 不是从零建补全,而是加 AI 层 + 补上下文 + 补缺动态源。**

### 四个核心决策(已在 brainstorming 阶段确认)

1. **B = 输入中补全,029 §7 = 命令后建议** —— 无重叠。B 负责 Tab/ghost-text(提升当前输入),029 负责 `💡 接下来可能想`(执行完后)。
2. **AI 补全包含四项**:LLM 子命令补全 + 自然语言补全 + 上下文排序 + 历史驱动 ghost-text。
3. **AI 补全走本地 Ollama**(经 029 §2.1 的 preferred_provider)—— 补全是高频低延迟场景,不能用云端(每次 Tab 等 500ms 不可接受)。
4. **不重写 Plan 021 的引擎** —— 在 `CompletionProvider::resolve()` 之后加 AI 层,作为候选的重排/补充,不替换。

### 范围内 / 范围外

| 范畴 | 本 Plan 包含 | 不包含 |
|---|---|---|
| **AI 补全** | LLM 子命令 + 自然语言 + 上下文排序 + 历史 ghost-text | 命令后建议(029 §7) |
| **上下文 plumbing** | CompletionContext 加 last_command/exit_code/history reader | 输出留存(029 决策 2 不做) |
| **缺失动态源** | ssh hosts / kubectl / env var names | 所有命令的动态源(只补高频) |
| **hinter 增强** | 历史 fuzzy ghost-text(替代 reedline 纯前缀) | AI 实时 ghost-text(延迟太高) |
| **Plan 021 引擎** | 不改(只在其后加 AI 层) | 重写 CompletionProvider |

---

## 第 1 节:子能力总览(给阶段 2 横向检查用)

| 子能力 | 主要消费者 | 依赖 | 跟其他方向的接触点 |
|---|---|---|---|
| **LLM 子命令补全** | 用户(Tab) | 029 §2.1 OllamaProvider + preferred_provider | 跟 Plan 021 静态 spec 协作(LLM 补静态 spec 没有的) |
| **自然语言补全** | 用户(输入自然语言) | 029 §2.2 ash→agent 桥 + F3 pipeline 翻译 | 跟 029 §5 F3 NL→pipeline **重叠**(阶段 2 决定合并) |
| **上下文排序** | 所有补全候选 | 029 §2.3 上下文 builder | 跟 029 §7 上下文感知共享 context |
| **历史 ghost-text** | 用户(打字时) | `~/.auto-shell-history` + fuzzy 匹配 | 跟 reedline CwdAwareHinter 协作(增强或替代) |
| **缺失动态源** | 用户(ssh/kubectl/env) | Plan 021 Command source | 无 |

**阶段 2 要检查的重叠点**:
1. **自然语言补全 vs 029 §5 F3 NL→pipeline** —— 两者都是"自然语言 → 命令"。B 是"边打边补",F3 是"完整输入后翻译"。可能合并成"统一的 NL→命令 能力,两种触发方式"。

---

## 第 2 节:现状(探勘确认)

### 2.1 Plan 021/315 已实现的(成熟)

**静态层**:
- `CompletionSpec { command, desc, subcommands, flags, args }`(spec.rs:24)
- `CompletionSource` 5 变体:Static/Command/Files/Directories/Variables(spec.rs:82)
- `CompletionProvider::resolve(parts, cursor_part, prefix, ctx) -> Vec<Completion>`(provider.rs:58)
- 三层目录:user > generated > cache(spec_tiers.rs)
- help-probe:`parse_help(cmd, help_text) -> CompletionSpec`(help_parser.rs:22)+ 运行时 `ensure_spec`(completions_reedline.rs:69)
- 5 内置 spec:git(17 子命令,8 处动态分支源)、docker(11 子命令,容器/镜像源)、cargo(13)、npm、ssh

**动态层**:`CompletionSource::Command { cmd, parse }` + 5s TTL 缓存 + Line/Field 解析。git 分支/remotes/tags/改动文件、docker 容器/镜像已接。

**路径层**:`file.rs` complete_file + tilde 展开 + prefix-subsequence fuzzy。

**ghost-text**:reedline `CwdAwareHinter`(历史前缀,Ctrl+F 接受,Ctrl+→ 接受词)。**只看历史前缀,不看 spec/AI**。

### 2.2 B 的缺口(greenfield)

**AI 层完全不存在**:
- `CompletionKind::AiSuggested` 变体存在(mod.rs:42)但**零生产者**
- 补全路径无任何 AI 调用
- `CompletionContext` 只有 `current_dir`,**无 last_command/exit_code/history**(provider.rs:15)

**上下文 plumbing 缺失**:
- `CompletionContext { current_dir, command_executor }` —— 只有这两字段
- `ShellCompleter` 的 `CompletionState` 也只有 `current_dir`(completions_reedline.rs:17)
- 历史文件 `~/.auto-shell-history` 存在,`read_history_file()` 存在(repl.rs:887),但**从不喂给补全**

**缺失动态源**:ssh hosts(无源)、kubectl 资源(无 spec)、env var names(硬编码 11 项,auto.rs:33)、cargo targets、npm scripts。

### 2.3 关键集成点

`ShellCompleter::complete(line, pos) -> Vec<Suggestion>`(completions_reedline.rs:192)是**所有补全的唯一汇聚点**。AI 层在这里插入,跟 `provider.resolve()` 和 `get_completions_with_context` 并列。

---

## 第 3 节:上下文 plumbing(M0 前置)

### 3.1 CompletionContext 增强

```rust
// ash-core/src/completions/provider.rs(改)
pub struct CompletionContext {
    pub current_dir: std::path::PathBuf,
    pub command_executor: Box<dyn Fn(&str, &Path) -> Result<String, String>>,
    // Plan 032 新增:
    pub last_command: Option<String>,       // 上一条命令
    pub last_exit_code: Option<i32>,        // 上一条 exit code
    pub history: Vec<String>,               // 最近 N 条历史(用于 AI/fuzzy)
    pub aliases: HashMap<String, String>,   // 用户别名
}
```

`ShellCompleter` 在构造 `CompletionContext` 时从 `Shell` 填充这些字段(复用 029 §2.3 的公开访问器)。

### 3.2 history reader

`read_history_file()` 已存在(repl.rs:887),但只读全量。新增带 limit 的版本:

```rust
// repl.rs 或独立模块
pub fn read_recent_history(path: &Path, n: usize) -> Vec<String> {
    // 读最后 N 行(避免全量读大文件)
    read_history_file(path).into_iter().rev().take(n).collect()
}
```

补全时注入最近 50 条历史(够 fuzzy/AI 用,不全量读)。

---

## 第 4 节:AI 补全层(核心)

### 4.1 LLM 子命令补全

**场景**:用户输入 `git ` 按 Tab,静态 spec 给出 checkout/commit/push 等。但如果用户装了个 git 插件(如 `git-town`),静态 spec 没有。LLM 补这层。

**设计**:在 `provider.resolve()` 返回候选后,如果**候选少于 3 个**或**用户前缀不是标准子命令**,异步调 Ollama 补充:

```rust
// ash/auto-shell/src/completions/ai_layer.rs(新增)
pub async fn ai_subcommand_suggest(
    cmd: &str,        // "git"
    prefix: &str,     // 用户输入的前缀
    ctx: &CompletionContext,
) -> Vec<Completion> {
    let prompt = format!(
        "用户在输入 shell 命令。命令: {}, 前缀: '{}'。\n\
         列出 5 个最可能的子命令或 flag(只返回子命令名,每行一个):\n\
         参考:当前目录{}, 上一条命令是 '{}'",
        cmd, prefix, ctx.current_dir.display(),
        ctx.last_command.as_deref().unwrap_or("(无)")
    );
    // 调 Ollama(tier:min, preferred_provider=ollama)
    let resp = ai_complete(&prompt, "tier:min", Some("ollama")).await;
    parse_lines(&resp).into_iter().filter(|s| s.starts_with(prefix))
        .map(|s| Completion { label: s, kind: CompletionKind::AiSuggested, .. })
        .collect()
}
```

**关键约束**:
- **异步 + 超时 500ms**:补全是高频交互,不能等。超时返回空(降级到静态)。
- **只在静态不足时触发**:静态 spec 有足够候选(≥3)时不调 AI(省本地算力)。
- **本地 Ollama**:不用云端(延迟 + 成本)。

### 4.2 自然语言补全

**场景**:用户输入 `列出最大文件` —— 这不是命令,但 LLM 能翻译成 `ls | sort .size | head`。

**设计**:在 `ShellCompleter::complete` 里,如果**第一个 token 不匹配任何已知命令/路径/alias**,触发 NL 翻译:

```rust
// completions/ai_layer.rs
pub async fn nl_to_pipeline_suggest(
    input: &str,      // "列出最大文件"
    ctx: &CompletionContext,
) -> Vec<Completion> {
    let prompt = format!(
        "把以下自然语言翻译成一条 ash 命令或 pipeline:\n{}\n\
         只返回命令本身,不要解释。ash 支持 ls|sort|head 等。",
        input
    );
    let resp = ai_complete(&prompt, "tier:min", Some("ollama")).await;
    vec![Completion {
        label: resp.trim().to_string(),
        kind: CompletionKind::AiSuggested,
        description: Some("(自然语言翻译)".into()),
        ..Default::default()
    }]
}
```

**跟 029 §5 F3 的重叠**:F3 是"用户完整输入后翻译+执行",这里是"边打边补全"。阶段 2 决定是否合并(共享 prompt + 翻译逻辑,两种触发)。

### 4.3 上下文排序

**场景**:用户刚 `cd` 进 git 仓库,输入 `gi` 按 Tab。静态给 `git/grep/gzip`,但 `git` 应该排第一(上下文相关)。

**设计**:对 `provider.resolve()` 返回的候选,按上下文重排:

```rust
pub fn context_rank(completions: &mut Vec<Completion>, ctx: &CompletionContext) {
    // 启发式排序(不调 AI,纯本地):
    // 1. 如果 cwd 是 git 仓库,git 相关命令排前
    // 2. 如果 last_command 是 cargo build,cargo test/relevant 排前
    // 3. 高频历史命令排前(从 history 统计)
    completions.sort_by(|a, b| {
        let sa = context_score(&a.label, ctx);
        let sb = context_score(&b.label, ctx);
        sb.partial_cmp(&sa).unwrap_or(Equal)
    });
}

fn context_score(cmd: &str, ctx: &CompletionContext) -> f64 {
    let mut score = 0.0;
    // 历史频率
    score += ctx.history.iter().filter(|h| h.starts_with(cmd)).count() as f64 * 0.5;
    // git 仓库上下文
    if is_git_repo(&ctx.current_dir) && cmd.starts_with("git") { score += 2.0; }
    // last_command 连贯性
    if let Some(last) = &ctx.last_command {
        if are_related(last, cmd) { score += 1.0; }
    }
    score
}
```

**不调 AI**(纯本地启发式,零延迟)。

### 4.4 历史驱动 ghost-text(增强 reedline hinter)

**现状**:reedline `CwdAwareHinter` 只做**前缀匹配**(用户输入 `git c`,history 里有 `git commit -m`,显示灰色 ghost)。但 `git commit` 如果用户上次写的是 `git commit --amend`,前缀匹配可能选错。

**增强**:用 **fuzzy 匹配**(复用 `file.rs` 的 prefix-subsequence)替代纯前缀:

```rust
// 新 Hinter,替代 CwdAwareHinter
pub struct AshHinter {
    history: Vec<String>,  // 注入最近历史
}

impl Hinter for AshHinter {
    fn hint(&self, line: &str, ..) -> Option<Span> {
        // 1. 先试前缀匹配(现有行为)
        // 2. 前缀无果 → 试 fuzzy(prefix-subsequence)
        // 3. 仍无果 → None
    }
}
```

**不做 AI 实时 ghost-text**(LLM 调用延迟 >200ms,打字时不可接受)。AI 只在 Tab 触发时用(§4.1/4.2)。

### 4.5 补全的合成顺序

`ShellCompleter::complete` 改造后的流程:

```
1. provider.resolve(静态 + 动态)  → 候选 A
2. 如果 A 不足(<3)且是子命令位 → ai_subcommand_suggest(AI 补充) → 候选 B
3. 如果第一个 token 不是已知命令  → nl_to_pipeline_suggest(NL 翻译) → 候选 C
4. context_rank(A + B + C 重排)    → 最终候选
5. 如果只有一个候选且是 ghost-text 风格 → 可选 inline 提示
```

**AI 调用是异步的**,主流程不阻塞:`provider.resolve` 立即返回,AI 结果到了再刷新菜单(用 reedline 的 `EditCommand::Complete` 重触发)。

---

## 第 5 节:缺失动态源(补 Plan 021 的洞)

### 5.1 ssh hosts

```rust
// definitions/ssh.rs 增强
// 从 ~/.ssh/config 和 ~/.ssh/known_hosts 解析 host
fn ssh_hosts() -> CompletionSource {
    CompletionSource::Command {
        cmd: "cat ~/.ssh/config ~/.ssh/known_hosts 2>/dev/null | grep -E '^Host|^host' | awk '{print $2}'".into(),
        parse: ParseMode::Line,
    }
}
```

或纯本地解析(不走 shell,直接读文件)。

### 5.2 env var names(替代硬编码 11 项)

```rust
// 替代 auto.rs:33 的硬编码
fn env_var_names() -> Vec<String> {
    std::env::vars().map(|(k, _)| k).collect()
}
```

`CompletionSource` 加一个 `Environment` 变体(或用 Static + 运行时填充)。

### 5.3 kubectl / cargo / npm(视需求)

kubectl resources / cargo metadata targets / npm scripts —— 每个都要写 spec + Command source。v1 视用户需求优先级,不全部做。

---

## 第 6 节:里程碑、风险、非目标

### 6.1 里程碑

#### M0:上下文 plumbing(1 周)
- `CompletionContext` 加 last_command/exit_code/history/aliases
- `ShellCompleter` 从 Shell 填充
- `read_recent_history` helper
- 测试:CompletionContext 正确携带上下文

#### M1:上下文排序 + 历史 ghost-text(1 周)
- `context_rank`(纯本地启发式)
- `AshHinter`(fuzzy 历史 ghost-text,替代 CwdAwareHinter)
- 测试:git 仓库下 git 命令排前;fuzzy ghost-text 工作

#### M2:LLM 子命令 + 自然语言补全(1.5 周)
- `ai_subcommand_suggest`(Ollama,500ms 超时)
- `nl_to_pipeline_suggest`
- `ShellCompleter::complete` 集成异步 AI(非阻塞)
- 测试:静态不足时 AI 补充;NL 输入翻译成 pipeline

#### M3:缺失动态源(0.5 周,可选)
- ssh hosts / env var names
- kubectl/cargo/npm 视需求

### 6.2 工作量

| 里程碑 | 代码行 | 估算 |
|---|---|---|
| M0 上下文 | ~300 | 1 周 |
| M1 排序+ghost-text | ~400 | 1 周 |
| M2 AI 补全 | ~600 | 1.5 周 |
| M3 动态源 | ~200 | 0.5 周 |
| **总计** | **~1500** | **4-5 周** |

### 6.3 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| **AI 补全延迟** (>500ms 破体验) | 高 | 高 | 严格超时;超时降级静态;Ollama 本地(低延迟) |
| **Ollama 未安装**(AI 层不可用) | 中 | 中 | 全部降级到静态+动态(Plan 021 已有);AI 是增强非必需 |
| **自然语言补全质量差**(LLM 翻译错) | 中 | 中 | 标记为 AiSuggested;用户可忽略;只补不自动执行 |
| **fuzzy ghost-text 选错项**(比前缀更激进) | 中 | 低 | 优先前缀匹配;fuzzy 仅在前缀无果时;用户 Ctrl+F 才接受 |
| **历史文件大**(读性能) | 低 | 低 | 只读最后 50 行(M0 的 read_recent_history) |
| **跟 029 F3 的 NL 重叠**(阶段 2 融合点) | 中 | 中 | 阶段 2 决定合并;v1 各自实现,prompt 共享 |

### 6.4 非目标

- ❌ **命令后建议**(💡 接下来)—— 029 §7 范围
- ❌ **AI 实时 ghost-text**(打字时调 LLM)—— 延迟不可接受;只 Tab 触发
- ❌ **重写 Plan 021 引擎** —— 在其后加层
- ❌ **所有命令的动态源** —— 只补高频(ssh/env)
- ❌ **学习型个性化模型** —— 远期
- ❌ **云端 LLM 补全** —— 延迟+成本,只用本地 Ollama

### 6.5 成功指标

1. **M0**:CompletionContext 携带 last_command/exit_code/history(测试验证)
2. **M1**:git 仓库下输入 `gi`,git 排第一;fuzzy ghost-text 工作(输入 `gcm` 匹配 `git commit -m`)
3. **M2**:静态 spec 没有的子命令,AI 补上(500ms 内);NL 输入"列出最大文件"补全成 pipeline
4. **M3**:ssh hosts 补全工作;env var 补全用真实环境(非硬编码)
5. **降级正确**:Ollama 未装时,补全完全回退到 Plan 021 行为(无 AI)

### 6.6 跟其他方向的关系

| 方向 | 关系 |
|---|---|
| **Plan 021/315**(已实现) | B 在其上加 AI 层 + 上下文 + 补动态源 |
| **Plan 029**(AI 能力) | 共享 §2.1 OllamaProvider + §2.3 上下文 builder;NL 补全跟 §5 F3 重叠(阶段 2) |
| **Plan 030**(ash-gui) | GUI 的补全面板(§4.1)消费 B 的候选 |
| **Plan 031**(数据处理) | 无直接关系 |

---

## 附录 A:实施前置勘探记录(2026-07-21)

### A.1 关键发现

1. **Plan 021/315 已成熟**:静态(CompletionSpec 树 + 三层目录 + help-probe + 5 内置 spec)+ 动态(Command source + 5s 缓存 + git/docker 接上)+ 路径(fuzzy)+ ghost-text(reedline CwdAwareHinter)。
2. **AI 层完全 greenfield**:`CompletionKind::AiSuggested` 存在但零生产者;补全路径无 AI 调用。
3. **上下文 plumbing 缺失**:CompletionContext 只有 current_dir;历史从不喂补全。
4. **文档不一致**:代码标 "Plan 315",文件名 021(重编号,同一份)。

### A.2 关键文件路径

- `ash-core/src/completions/spec.rs` —— CompletionSpec + CompletionSource(5 变体)
- `ash-core/src/completions/provider.rs:58` —— resolve() + CompletionContext(只有 current_dir)
- `ash-core/src/completions/help_parser.rs:22` —— parse_help(help-probe)
- `ash/auto-shell/src/completions/spec_tiers.rs` —— 三层目录
- `ash/auto-shell/src/completions/definitions/{git,docker,cargo,npm,ssh,env}.rs` —— 5+1 内置 spec
- `ash/auto-shell/src/frontend/completions_reedline.rs:192` —— ShellCompleter::complete(唯一汇聚点)
- `ash/auto-shell/src/frontend/repl.rs:243-255` —— CwdAwareHinter(ghost-text)
- `ash/auto-shell/src/frontend/repl.rs:887` —— read_history_file(历史读取,未喂补全)

---

## 参考

- `plans/021-ash-arbitrary-command-completion.md` —— Plan 021/315,B 在其上增强
- `designs/029-ai-capabilities.md` —— §2.1 OllamaProvider / §2.3 上下文 builder / §5 F3 NL(NL 补全重叠点)
- `designs/030-ash-gui.md` —— §4.1 补全面板消费 B 的候选
