# Plan 002: 修复 `parse_args` 吞掉 Windows 反斜杠路径

- **日期**: 2026-06-25
- **状态**: ✅ 已完成（2026-06-25）
- **目标**: 让用户在命令行输入的 Windows 反斜杠路径（如 `open C:\Users\zhaop\data.csv`）不再被词法切分器破坏。

## 1. 背景与根因

### 现象
在 ash 里执行 `open C:\Users\zhaop\data.csv`（或任何接受路径的命令），文件名到达命令时变成 `C:Userszpaopdata.csv`——**反斜杠全被吃掉**，导致"No such file or directory"。这个问题在 Plan 001（open 命令）的集成测试中首次复现，但根因是全局的。

### 根因
`ash-core/src/parser/quote.rs::parse_args`（全局唯一的词法切分器）把 `\` 当成转义符。关键代码（quote.rs:42-53）：

```rust
} else if c == '\\' {
    if in_single_quote {
        // 单引号内：反斜杠字面（除 \' 外）
    } else {
        escape_next = true;   // ← 裸文本 AND 双引号内都触发转义
    }
}
```

对裸文本里的 `C:\Users\zhaop`：
- `\U` → escape_next，下一字符 `U` 命中 `_ => c`（未知转义，只保留字符本身）→ 推入 `U`，**反斜杠丢失**。
- 所有 `\字母` 序列同理丢反斜杠。

### 影响面
`quote::parse_args` 是**全局唯一的词法切分器**，被以下路径调用：
- `shell.rs:475, 608, 756, 2107`（多条执行管线）
- `cmd/builtin.rs:26, 124`（legacy 内置分发）

因此**所有命令**的路径参数都受影响，不只是 open/ls/cd。

### 相关的既有修复
`expand_tilde`（shell.rs:1393）此前已规避此问题：它把 `~` 展开成**正斜杠**路径（`#[cfg(windows)] home.replace('\\', "/")`），所以 `cd ~` / `ls ~` 不受影响。但用户**直接输入**的 `C:\...` 路径仍会踩坑。

## 2. 设计原则

**确立一条 Auto/ash 的路径表示规范：内部一切路径统一用正斜杠 `/`（POSIX 风格）。反斜杠 `\` 只作为转义符存在，不再兼任路径分隔符。**

这条原则通过修改 `parse_args` 实现：**裸文本（未加引号）里的 `\` 当字面字符**，不再触发转义。跨平台行为一致（不用 `#[cfg]` 平台特判）。

### 契约
| 输入形式 | `\` 的含义 | 示例 |
|---|---|---|
| **裸文本** | 字面字符（原样保留） | `open C:\Users` → `C:\Users` ✓ |
| **双引号内** | 转义符（保留现有语义） | `echo "a\nb"` → `a<换行>b` |
| **单引号内** | 字面字符（现有行为不变） | `echo 'a\b'` → `a\b` |

**用户契约**：需要转义/特殊字符时，**加双引号**。需要字面反斜杠时，用裸文本或单引号。

### 为什么这样设计
1. **符合 Windows 用户直觉**：Windows 用户从不在裸文本里写 `C:\\Users`（那是 Unix 习惯），他们写 `C:\Users`，期望原样保留。
2. **不破坏既有转义能力**：双引号内的 `\"` `\\` `\n` `\t` 仍工作（现有测试 `test_escaped_*` 不受影响）。
3. **跨平台统一**：行为不依赖平台特判，ash-core 保持可移植。
4. **与 expand_tilde 一致**：都遵循"内部统一 `/`"的原则。

## 3. 改动点

### 核心改动：`ash-core/src/parser/quote.rs::parse_args`
修改 `c == '\\'` 分支的裸文本处理：

```rust
} else if c == '\\' {
    if in_single_quote {
        // 单引号内：反斜杠字面（除 \' 外）—— 不变
        if let Some(&'\'') = chars.peek() {
            escape_next = true;
        } else {
            current.push(c);
        }
    } else if in_double_quote {
        // 双引号内：保留转义语义 —— 不变
        escape_next = true;
    } else {
        // 裸文本：反斜杠当字面字符（修复点）
        current.push(c);
    }
}
```

**只动裸文本分支**（原来它走 `escape_next = true`，改成 `current.push(c)`）。双引号、单引号分支完全不变。

> 注：`parse_args_preserve_quotes`（quote.rs:116）也把裸文本 `\` 当转义（line 128-130）。为保持一致性，应做同样的修改（裸文本 `\` 字面保留，但保留在输出里）。需确认其调用方是否依赖转义行为。

### 不在本次范围（YAGNI）
- **不**做路径自动归一化层（把所有参数的 `\` → `/`）。Plan 001 的 `expand_tilde` 已经处理 `~`；裸文本字面化后 `C:\Users` 能原样到达命令，Rust `Path` 接受混合分隔符，无需再转。归一化可作为后续独立提案。
- **不**改 `parse_args_preserve_quotes` 之外的其他转义逻辑。
- **不**引入路径类型/校验。

## 4. 测试策略

### TDD 流程（红→绿）
改动在 ash-core，测试加在 `ash-core/src/parser/quote.rs` 的 tests 模块。

#### 新增测试（RED 先行）
| 测试 | 输入 | 期望 |
|---|---|---|
| `bare_backslash_is_literal` | `open C:\Users\zhaop` | `["open", "C:\\Users\\zhaop"]` |
| `bare_backslash_preserved_in_path` | `cd D:\a\b\c` | `["cd", "D:\\a\\b\\c"]` |
| `bare_single_backslash` | `echo a\b` | `["echo", "a\\b"]` |
| `double_quote_escape_still_works`（回归保护） | `echo "a\nb"` | `["echo", "a\nb"]`（转义仍生效） |
| `double_quote_backslash_escape`（回归） | `echo "x\\y"` | `["echo", "x\\y"]` |
| `single_quote_backslash_literal`（回归） | `echo 'a\b'` | `["echo", "a\\b"]` |

#### 现有测试核查
现有 `test_escaped_*`（双引号转义）必须**全部继续通过**——这是回归保护的核心。逐一确认：
- `test_escaped_backslash`：`"test\\\\path"` → `test\path` ✓（双引号内，不改）
- `test_escaped_double_quote`：`"test\\\"quote"` → `test"quote` ✓
- `test_escaped_newline`：`"line1\\nline2"` → `line1\nline2` ✓
- `test_quote_after_text`：`echo"test"` → 原样（裸文本，不涉及 `\`）

### 集成回归
- 全量 `cargo test`（ash-core + auto-shell），确认 523+ 测试无回归。
- 重点回归：Plan 001 open 的集成测试现在可以用**反斜杠路径**重跑（之前被迫用正斜杠绕过）。

## 5. 实施步骤

1. **TDD RED**：在 quote.rs tests 加裸文本反斜杠字面的测试 → 失败（当前转义行为）。
2. **核查 preserve_quotes**：确认 `parse_args_preserve_quotes` 是否需要同步改 + 是否有调用方依赖。
3. **TDD GREEN**：修改 `parse_args` 的裸文本分支（`current.push(c)`）→ 新测试通过。
4. **回归验证**：现有 `test_escaped_*` 全过；全量 cargo test 无回归。
5. **（可选）补强**：给 `parse_args_preserve_quotes` 加同样的裸文本测试并修复。
6. 提交 + push。

## 6. 验收标准

- [ ] `open C:\Users\zhaop\data.csv` 能正确读取文件（反斜杠保留）
- [ ] `cd D:\folder\sub` 能正确切换目录
- [ ] `echo "a\nb"` 仍输出带换行的 `a<换行>b`（双引号转义不破坏）
- [ ] `echo "x\"y"` 仍输出 `x"y`（双引号转义不破坏）
- [ ] 现有 `test_escaped_*` 测试全部通过
- [ ] ash-core + auto-shell 全量 `cargo test` 通过，无回归
- [ ] Plan 001 open 集成测试可用反斜杠路径重跑通过（可选验证）

## 7. 风险与备注

- **风险**：若某处既有代码依赖"裸文本里 `\` 是转义"（如 `echo a\ b` 想输出 `a b`），修改后会变成输出 `a\ b`。需在全量回归中捕获。但 Windows 用户极少这么用，且 ash 以 Windows 为主。
- **ash-core 可移植性**：本方案跨平台统一行为，不破坏 ash-core 在 Unix 上的可用性；只是 Unix 上裸文本反斜杠也从"转义"变"字面"——这是行为变更，但对 Unix 用户影响小（他们用引号转义）。
- **与 Plan 001 的关系**：Plan 001 的 open 集成测试当时被迫用正斜杠绕过此 bug。本 Plan 修复后，可选择性放宽那些测试用反斜杠路径验证（非必须）。
