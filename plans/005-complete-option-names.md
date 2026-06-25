# Plan 005: 补全带值 option 名（第一期）

- **日期**: 2026-06-25
- **状态**: 待实施
- **目标**: 让内置命令的**带值 option**（`option`/`option_with_short` 定义，如 sort 的 `-w`/`-k`/`-t`、open 的 `--as`）也能被 Tab 补全，和 flag 一样出现在 `cmd -<Tab>` / `cmd --<Tab>` 的候选里。

## 1. 背景与根因

### 现象
`sort -<Tab>` 只列出 `-r`/`-n`/`-u`/`-f`（4 个 flag），**完全列不出 `-w`/`-k`/`-t`**（option）。用户输入 `sort -w` 时没有任何提示，得靠记忆。

### 根因（探查确认，三层）
补全系统对内置命令走 **Signature 静态路径**，该路径有两处把 option 信息丢弃/跳过：

1. **`ash-core/src/completions/types.rs` 的 `CompletionArgument`**（约 line 17-23）只有 5 个字段，**没有 `is_option`**——补全侧无从知道某参数是带值 option。
2. **`ash/auto-shell/src/cmd.rs:54` 的 `From<Argument>`** 在把 `Argument` 转成 `CompletionArgument` 时，**丢弃了 `is_option` 和 `default`**（`Argument` 本身有这些字段，转换时没带上）。
3. **`ash-core/src/completions/flag.rs:30` 的 `complete_flags`** 有 `if !arg.is_flag { continue; }`，**跳过所有非 flag 参数**（option 的 `is_flag` 为 false，被跳过）。

三层叠加：option 信息既没保留（types），又被丢弃（From），又被过滤（flag.rs），所以 option 名永远补不出来。

### 架构背景（决定范围）
补全有两条路径：
- **Provider 路径**（外部命令 git/cargo/...）：有动态能力（能跑外部命令拿候选），sort 因 `is_legacy_builtin` 被锁在外面。
- **Signature 路径**（内置命令）：纯静态，无运行时 hook。

本计划只动 Signature 路径，**不碰 Provider 路径、不碰 Shell 注入**。动态列名补全（`ls | sort -w <Tab>` 列出列名）需要 Shell 运行时支持，属于**第二期**（独立计划），本期不做。

## 2. 设计

### 行为契约
| # | 规则 |
|---|---|
| 1 | `cmd -<Tab>` 同时列出 flag 和 option 的短/长形式（如 sort 列出 `-r -n -u -f -w -k -t` 及对应长形式） |
| 2 | option 的呈现与 flag 完全一致：选中插入 `-w`，**不带尾随空格**（用户自己打空格再输值） |
| 3 | option 的**值不补全**（第二期范畴） |
| 4 | 已设置的 option（`already_set`）与 flag 一样去重，不重复列出 |
| 5 | 纯 positional 参数（如 `<file>`）不受影响，仍走文件补全 |

### 不在本期范围（YAGNI / 第二期）
- option 值的动态补全（列名等）
- 把 Shell 注入补全过程
- option 选中后自动加尾随空格 / 显示 `<VALUE>` 占位符

## 3. 实现改动（三处）

### 改动 A — `ash-core/src/completions/types.rs`：`CompletionArgument` 加字段
```rust
pub struct CompletionArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub is_flag: bool,
    pub short: Option<char>,
    pub is_option: bool,   // ← 新增
}
```

### 改动 B — `ash/auto-shell/src/cmd.rs:54`：`From<Argument>` 补回 `is_option`
```rust
impl From<Argument> for CompletionArgument {
    fn from(arg: Argument) -> Self {
        Self {
            name: arg.name,
            description: arg.description,
            required: arg.required,
            is_flag: arg.is_flag,
            short: arg.short,
            is_option: arg.is_option,   // ← 现在被丢弃，补回来
        }
    }
}
```
> 注：`Argument`（cmd.rs 约 line 32）已有 `is_option` 字段（Signature builder `option`/`option_with_short` 设为 true），只是转换时没带。

### 改动 C — `ash-core/src/completions/flag.rs:30`：放宽过滤
```rust
for arg in &sig.arguments {
    if !arg.is_flag && !arg.is_option {   // ← 原: if !arg.is_flag { continue; }
        continue;
    }
    // ... 其余补全逻辑完全不变（flag 和 option 同等对待）
}
```

### 连带改动
- `CompletionArgument` 加字段后，所有**字面量构造**它的地方都要补 `is_option`：
  - `flag.rs` 测试里的 `ls_sig()`（约 line 91-126）——给现有 4 个 argument 补 `is_option: false`
  - 全仓搜索 `CompletionArgument {` 字面量，逐一补字段（机械改动）

## 4. 测试策略

| 层级 | 测试 | 方式 |
|---|---|---|
| 单元 | `complete_flags` 能补出 option 的短/长形式 | flag.rs tests 加一个带 option 的 signature |
| 单元 | option 与 flag 同时出现在 `cmd -<Tab>` | 断言结果含 `-w` 和 `-r` |
| 单元 | 已设置的 option 去重 | `already_set` 含 `--with`，断言不再出现 |
| 单元 | pure positional 仍被跳过 | `is_flag: false, is_option: false` 的参数不出现在 flag 补全里 |
| 回归 | 现有 flag 补全测试全过 | 不改 behavior |

### TDD 流程
1. **RED**：在 flag.rs tests 加带 option 的 signature + 断言 `-w` 能补全 → 失败（当前跳过 option）
2. **GREEN**：改 A（types）+ B（From）+ C（flag.rs 过滤）→ 通过
3. **回归**：现有 flag 测试 + 全量 cargo test

## 5. 实施步骤

1. **TDD RED**：flag.rs tests 加 option 补全断言。
2. **改动 A**：types.rs `CompletionArgument` 加 `is_option`。
3. **改动 B**：cmd.rs `From<Argument>` 补 `is_option`。
4. **连带**：补全所有 `CompletionArgument {` 字面量构造（加 `is_option` 字段）。
5. **改动 C**：flag.rs 放宽过滤。
6. **GREEN**：新测试通过。
7. **全量回归**：ash-core + auto-shell cargo test。
8. 提交 + push。

## 6. 验收标准

- [ ] `sort -<Tab>` 列出 `-r -n -u -f -w -k -t`
- [ ] `sort --<Tab>` 列出 `--reverse --numeric ... --with --key --field-separator`
- [ ] 选中 `-w` 后插入 `-w`（无尾随空格）
- [ ] 已设置的 option 不重复出现
- [ ] `open --<Tab>` 列出 `--as`（open 也有 option）
- [ ] 现有 flag 补全行为不变
- [ ] 全量 cargo test 通过，无回归

## 7. 风险与备注

- **风险极低**：纯加字段 + 放宽过滤，不改分发逻辑、不碰 Shell、不碰 Provider 路径。
- **机械改动量**：主要在补全所有 `CompletionArgument {` 字面量的 `is_option` 字段（编译器会报错指路，逐个补即可）。
- **与第二期的关系**：本期打通了"option 名能补全"这一基础。第二期（动态列名补全）将在此之上加 option 值的动态生成，需要单独的架构设计（Shell 注入 + 动态值源）。
