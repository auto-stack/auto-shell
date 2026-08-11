# Plan 053: ash-gui-auto 对齐 vue 原版 — 输入体验与渲染打磨

> **日期**: 2026-08-11
> **状态**: 📋 草案（待实施）
> **来源**: 全面对比 `ash-gui-vue`（手写原版）与 `ash-gui-auto`（AutoLang 生成版）的功能差距
> **范围**: `ash-gui-auto/src/front/*.at`（AutoLang 源码）+ `auto-lang` 仓库的 Vue codegen（必要的基础设施修复）
> **核心目标**: 让 auto 版的**输入体验**（ghost text / 语法高亮 / 多行续行 / 补全 debounce）与**渲染打磨**对齐 vue 原版，使两版在 vue 模式下功能等价。
> **前置**: Plan 051（前后端拆分）、Plan 052（GAP-Table 透传）已完成——vue 模式 SSE 闭环 + 结构化 Table 渲染已通。

---

## 0. 背景

### 为什么要做这个对齐

Plan 043 把手写 `ash-gui-vue` 反生成为 `.at` 源码驱动的 `ash-gui-auto`，目标是「源码驱动的一等公民 + 可重生」。
反向生成时优先打通了**架构骨架**（widget/store/api 契约/SSE 桥），但 `PromptBar` 的一些**交互细节**留了 stub：

- `prompt_bar.at:218-224` 的 `AcceptGhost`/`AcceptGhostWord` 是空 handler
- `prompt_bar.at:26` 的 `in_continuation` 声明了却永不置 true
- 文件头注释（`prompt_bar.at:4-5`）声称「ghost text + 语法高亮」，但 view 没兑现

这些是 Plan 043 已知的「设计权衡式缺口」（DEBTS.md:488-492）。本计划把它们补齐。

### 当前状态（已扣除 Plan 052）

| 层面 | 状态 |
|---|---|
| 后端通信（vue 模式） | ✅ 已对齐（051 SSE 闭环 + 052 Table 透传） |
| 渲染器分发（Table/Record/Code/Text/Error） | ✅ 已对齐（block_body.at 分发 + 052 完整透传） |
| block 折叠 | ✅ auto 版**领先**（Plan 049，vue 原版反而没有） |
| **输入体验** | ❌ **核心差距**（本计划主战场） |
| 渲染/交互打磨 | ⚠️ 多处 codegen bug + 体验缺失（本计划顺带修） |

### 关键约束（实施前必须知道的 codegen 能力边界）

源码核查发现两个**系统性 codegen 限制**，直接决定本计划的技术路线：

1. **Vue codegen 字符串方法映射不全**（`auto-lang/.../ui_gen/vue.rs:5463-5476`）
   只映射了 5 个：`len→length`、`contains→includes`、`to_string→toString`、`to_int→parseInt`、`to_float→parseFloat`。
   `to_lower / to_upper / starts_with / ends_with / char_at / find / substr / slice / replace / trim_left …` **原样输出，生成无效 JS**。
   - 实证：`gen/.../HistorySearch.vue:31` 的 `query.value.trim().to_lower()` 就是坏 JS（`.to_lower` 不存在）。
   - **影响**：凡是要在 vue 模式跑的 `.at` 字符串操作，都被这个限制卡住——包括 ghost text（`starts_with`）、语法高亮（`char_at`）。

2. **.at 完全没有定时器**（`native_catalog.rs` 全表无 `setTimeout/setInterval/debounce`）
   - 最接近的 `auto.time.sleep_ms` 是**阻塞** sleep，会卡 UI 线程，不能做 debounce。
   - **影响**：补全 debounce（vue 原版用 `setTimeout` + 序列号）**无法在 .at 层实现**，必须 codegen 注入或产物手写。

3. **computed 多行 body**：Vue target 已安全（commit `db19b947`），但 aura VM target 团队仍标注「有风险」（`shell_store.at:14,83-84` 注释）。
   - **对策**：遵循团队既定模式——复杂逻辑写 **module fn + handler 写 model 字段**，computed 只 `=> .field` 单表达式返回。

> **结论**：本计划无法「纯 .at 改动」完成，必须配合少量 **auto-lang codegen 修复**（字符串方法映射 + debounce 注入 + textarea 默认 class）。这与 Plan 051/052 跨 auto-lang worktree 的模式一致。

---

## 1. 差距全景（auto 版 vs vue 原版）

### A. 输入体验（PromptBar）— 本计划核心

| # | 能力 | vue 原版 | auto 版 | 性质 | 目标里程碑 |
|---|---|---|---|---|---|
| A1 | ghost text 自动建议 | ✅ 最长历史前缀+补全回退（`PromptBar.vue:299-328`） | ❌ 空 stub（`prompt_bar.at:218-224`） | 功能缺失 | M2 |
| A2 | Ctrl+F 接受全部建议 | ✅ `:252-257,331-340` | ❌ 无绑定 | 功能缺失 | M2 |
| A3 | Ctrl+→ 接受一个词 | ✅ `:258-263,343-354` | ❌ 无绑定 | 功能缺失 | M2 |
| A4 | 输入框语法高亮 | ✅ 自研 tokenize overlay（`highlight.ts`+`:289,391-397`） | ❌ 裸 input 无高亮（`:100-116`） | 功能缺失 | M3 |
| A5 | 多行续行检测 | ✅ 括号/引号/`\`+`·`提示+autoGrow（`:104-129,170-190`） | ❌ `in_continuation` 永不置 true | 功能缺失 | M4 |
| A6 | 补全 debounce | ✅ 80ms+序列号（`:63-85`） | ❌ 每次 keyup 同步触发（`:130-137`） | 正确性 | M5 |

### B. 渲染 / block 交互打磨 — 本计划顺带修

| # | 能力 | vue 原版 | auto 版 | 性质 | 目标里程碑 |
|---|---|---|---|---|---|
| B1 | duration 标签 | ✅ | ⚠️ codegen bug：内层 if 无 return（`BlockItem.vue:12-14`） | bug | M6 |
| B2 | cwd 缩写 `~` | ✅ `abbrevPath`（`path.ts`） | ❌ 显示全路径 | 体验 | M6 |
| B3 | copy 错误兜底 | ✅ | ❌ `navigator.clipboard` 无 `?.`/`.catch` | 健壮性 | M6 |
| B4 | MemoryInfo 进度数值 | ✅ 解析 `usage_percent`（`RecordView.vue:17-34`） | ⚠️ `Number()` 无 `isFinite`/不剥 `%`（`"75%"→NaN`） | bug | M6 |
| B5 | HistorySearch 方法名 | ✅ | ⚠️ 产物 `to_lower()` 坏 JS（`HistorySearch.vue:31,36`）+ `%` 括号（`:63`） | codegen bug | M1 自动修复 |

### C. 类型安全 / D. VM 模式 — 本计划范围外（记录为后续工作面，见 §7）

---

## 2. 任务里程碑（M1–M7）

> 改动跨两个仓库：**auto-shell**（`ash-gui-auto/src/`）与 **auto-lang worktree**（`crates/auto-lang/src/ui_gen/vue.rs` 等）。
> 每个里程碑可独立交付、独立验证。**M1 是 M2/M3 的前置**（解锁字符串方法）。

### M1: codegen 字符串方法映射修复（基础设施）★

**目标**：修复 Vue codegen 的字符串方法映射不全，让 `.at` 的字符串操作在 vue 产物里生成有效 JS。一举解决 B5（HistorySearch `to_lower` bug）并为 M2/M3 铺路。

**改动**（auto-lang worktree）：
- 文件：`crates/auto-lang/src/ui_gen/vue.rs:5463-5476`（method map）
- 补全映射表：

| .at 方法 | TS 产物 |
|---|---|
| `to_lower` / `lower` | `.toLowerCase()` |
| `to_upper` / `upper` | `.toUpperCase()` |
| `starts_with` | `.startsWith(...)` |
| `ends_with` | `.endsWith(...)` |
| `char_at` | `.charAt(...)` |
| `find` | `.indexOf(...)` |
| `substr` / `sub` | `.substring(...)`（注意参数语义核对） |
| `slice` | `.slice(...)` |
| `replace` | `.replaceAll(...)`（.at replace 语义为全替换；若仅首个则 `.replace`） |
| `trim_left` | `.trimStart()` |
| `trim_right` | `.trimEnd()` |
| `repeat` | `.repeat(...)` |
| `reverse` | `String.fromCodePoint(...[...x].reverse())` 或保留原生扩展 |

- 同步检查 `ts_adapter.rs:193`（`method_map_decision`）是否需对齐。

**验证**：
1. `HistorySearch.vue` 产物里 `to_lower()` → `toLowerCase()`（B5 修复）
2. `vue-tsc --noEmit` 0 错误
3. 写一条 codegen 单测：断言 `.to_lower()` 产 `.toLowerCase()`

**风险**：`substr`/`sub` 的参数语义（start+count vs start+end）需核对 `libs/string.rs` 实现，勿错译。`replace` 全替换 vs 首个需核对。

---

### M2: ghost text 自动建议（A1 + A2 + A3）

**目标**：输入框 inline 显示灰色「最长历史前缀匹配」建议；Ctrl+F 接受全部、Ctrl+→ 接受一个词。

**改动**（auto-shell，`ash-gui-auto/src/front/`）：

1. **`prompt_bar.at` model 加字段**：
   - `var ghost_text str = ""`
   - `var complete_seq int = 0`（若 M5 debounce 需要；M2 暂可不用）

2. **module fn 计算 ghost text**（遵循团队模式：复杂逻辑放 module fn + handler 写字段）：
   - 在 `prompt_bar.at` 顶层加 `fn compute_ghost(input: str, history: []str, first_suggestion: str) -> str`：
     - 遍历 history（倒序），找**以 input 为前缀且比 input 长**的最长条目（用 `starts_with`）；
     - 无匹配则回退 `first_suggestion`；
     - 返回建议后缀（去掉已输入前缀）。
   - 逻辑照搬 `PromptBar.vue:299-328`。

3. **handler `.OnInput` 末尾**：调 `compute_ghost(...)` 写 `.ghost_text`。

4. **view 改造**（input 外包 overlay——`history_search.at:19` 已验证的 `relative`+`absolute` 手法）：
   ```
   stack {
       style: "relative flex-1"
       // ghost text 叠加层
       span .ghost_text {
           style: "pointer-events-none absolute inset-0 text-sm font-mono-ash text-muted-foreground/35 whitespace-pre"
       }
       input { ... 现有 input, style 末尾加 "text-transparent caret-foreground" ... }
   }
   ```
   > 注：M2 暂只叠 ghost text，不加高亮 span（那是 M3）。input 文字透明、光标保留——ghost text 透出来。

5. **绑定键**（input onkeydown）：
   - `ctrl.f: .AcceptGhost`
   - `ctrl.right: .AcceptGhostWord`

6. **填空 handler**（`prompt_bar.at:218-224`）：
   - `.AcceptGhost` → `.input = .input + .ghost_text; .ghost_text = ""`
   - `.AcceptGhostWord` → 用 `find(" ")` 取下一个空白，拼入首个 token；逻辑照搬 `PromptBar.vue:343-354`。

**依赖**：M1（`starts_with` / `find` 的 Vue 映射）。

**验收**：输入 `ls` 历史，再输 `l` 出现灰色 `s`；Ctrl+F 整条接受；Ctrl+→ 接受一个词。

---

### M3: 输入框语法高亮 overlay（A4）

**目标**：输入框命令名/字符串/flag/变量/注释异色（fish/zsh 风格），与输出区分开。

**改动**：

1. **新建 `ash-gui-auto/src/front/lib/highlight.at`**：
   ```
   pub type HighlightSpan { text: str, cls: str }

   pub fn tokenize(line: str) -> []HighlightSpan {
       // 状态机：逐字符 char_at + Char.is_whitespace/is_alphanum/is_ident
       // 识别：# 注释 / " ' 字符串 / $ 变量 / | && ; 运算符 / > < 重定向 / -flag / 命令位
       // 色板（Tailwind class）：
       //   cmd-builtin → "text-emerald-400 font-bold"
       //   cmd-external → "text-sky-400"
       //   string → "text-amber-400"
       //   flag → "text-purple-400"
       //   variable → "text-red-400"
       //   operator → "text-pink-400 font-bold"
       //   comment → "text-muted-foreground italic"
       //   plain → ""
       ...
   }
   ```
   - 逻辑照搬 `ash-gui-vue/src/lib/highlight.ts`（168 行状态机），正则全部改写为 `char_at` + 字符比较 + `Char.is_*`（VM 原生可用）。
   - BUILTINS Set 用 `[]str` + `contains` 查找（或硬编码 if 链）。

2. **`prompt_bar.at`**：
   - model 加 `var highlighted_spans []HighlightSpan = []`
   - `.OnInput` handler 里 `var spans = tokenize(.input); .highlighted_spans = spans`（同时调 `compute_ghost`）
   - view 的 overlay 层（M2 建的 `span` 容器）里：
     ```
     span {
         style: "pointer-events-none absolute inset-0 text-sm font-mono-ash whitespace-pre-wrap break-all overflow-hidden"
         for sp in .highlighted_spans {
             span .sp.text { class: .sp.cls }
         }
         // ghost text 接在后面
         span .ghost_text { style: "text-muted-foreground/35" }
     }
     ```
   > ⚠️ 动态 Tailwind class 必须用 `class: .sp.cls`（emit `:class`），**不能**用 `style: .sp.cls`（会被当 inline CSS emit `:style`，对 Tailwind class 无效——`vue.rs:5069-5089`）。

**依赖**：M1（`char_at` / `starts_with` 的 Vue 映射）。

**风险**：tokenize 状态机逻辑较长，移植需仔细对照 `highlight.ts`。建议先写最小版（注释/字符串/flag/命令名 4 类），再增量补全。

**验收**：输入 `ls $HOME # c` 时命令名（emerald）、变量（red）、注释（muted italic）颜色不同。

---

### M4: 多行续行检测（A5）

**目标**：未闭合 `{ } ( ) [ ] " '` 或尾随 `\` 时 Enter 换行而非执行，提示符 `❯→·`。

**改动**：

1. **module fn**：`prompt_bar.at` 顶层加 `fn needs_continuation(text: str) -> bool`：
   - 括号/方括号/大括号深度计数；
   - 引号状态机（识别 `\"` 转义）；
   - 尾随 `\` 检测。
   - 逻辑照搬 `PromptBar.vue:104-129`，正则改字符比较。

2. **`prompt_bar.at`**：
   - `.OnInput` 末尾：`.in_continuation = needs_continuation(.input + "\n")`
   - `onenter`：当前是 `onenter: .Run(.input)`。改为 `onenter: .OnEnter`，handler 里：
     - 若 `needs_continuation(.input + "\n")` → 放行默认换行（.at 里如何「不 prevent」？—— textarea 的 Enter 默认换行，handler 不 emit Run 即可）
     - 否则 → `.Run(.input)`
   - prompt 符号：`prompt_bar.at:89` 的 `❯` 改为 computed `prompt_symbol => if .in_continuation { "·" } else { "❯" }`（颜色已由 `:79` 的 if 控制）

3. **input → textarea**（支持多行）：
   - `prompt_bar.at:100` 的 `input { ... }` 改 `textarea { ... }`
   - **codegen 小改动**（auto-lang worktree）：`vue.rs:4909-4918` 的 `user_class_skip_elements` 列表加入 `"textarea"`（当前含 `input` 不含 `textarea`，否则会被强加 `border rounded px-2 py-1`，`vue.rs:4961`）。
   - textarea 加 autoGrow：vue 原版用 `scrollHeight` 自适应。.at 无 DOM API → 产物手写或 codegen 注入（见风险）。

**依赖**：M1（字符串方法）。textarea 的 codegen 改动独立。

**风险**：
- **autoGrow**：.at 无 `scrollHeight` 访问。降级方案：(a) 固定 `rows` + CSS `resize-none` + `max-h-[168px] overflow-y-auto`，靠浏览器原生 textarea 滚动（不做高度自适应）；(b) 产物手写 autoGrow watch（脆弱）。**推荐 (a)**——零 codegen 改动，体验略逊但可接受。
- `.at` 里「放行默认换行」的语义需验证：textarea 的 `onenter` handler 若不 emit Run、也不 prevent，是否产生换行？实施时在产物验证；若不行则 codegen 补 `preventDefault` 条件。

**验收**：输入 `for i in (1..3) {` 按 Enter 换行，提示符变 `·`；补 `}` 后 Enter 执行。

---

### M5: 补全 debounce（A6）

**目标**：补全请求 80ms debounce + 序列号丢弃过期结果，避免每次 keyup 打后端。

**约束**：.at 无定时器（§0 约束 2），**不能在 .at 层实现**。

**方案**（按推荐度）：

1. **codegen 注入（推荐）**：`auto-lang/.../ui_gen/vue.rs`——生成 input 元素的 oninput/onkeyup handler 时，若 handler body 含 `complete(` 调用，自动包一层 debounce wrapper：
   - 产物形如：`let __t; let __seq=0; const onInput = () => { clearTimeout(__t); __seq++; const mySeq=__seq; __t=setTimeout(()=>{ ...原 complete 调用，结果按 mySeq 过期判定... }, 80) }`
   - 逻辑照搬 `PromptBar.vue:60-84`。
   - 触发条件识别：handler 体 AST 含对 `complete` module fn 的调用。
   - 一劳永逸、单源不破。

2. **产物手写（降级）**：每次 regen 后在 `PromptBar.vue` 的 `OnInput` 处贴 `clearTimeout/setTimeout`。脆弱，需写进流程文档。

3. **去掉 debounce（零成本降级）**：保持现状。若后端 complete 够快（已有命令表缓存）可接受；否则快速连打时卡顿。

**改动**（方案 1）：auto-lang worktree `vue.rs`，input handler 生成逻辑加 debounce 包装。

**验收**：快速连打 `lslsls` 时，complete 只在停顿后请求一次（Network 面板观察），无过期结果闪烁。

---

### M6: 渲染打磨批（B1 + B2 + B3 + B4）

**目标**：修复 4 处渲染/交互瑕疵。均为小改、纯 .at（B5 已由 M1 自动修复）。

**改动**（auto-shell，`ash-gui-auto/src/front/`）：

1. **B1 duration_label bug**（`block_item.at:25` + 产物 `BlockItem.vue:12-14`）：
   - 根因：codegen 生成内层 `if` 无 `return`，导致 undefined。
   - 修法：核对 `block_item.at:25` 的 `duration_label` computed 写法，确保所有分支有返回值；若 codegen 丢 return，需在 auto-lang 修 computed→箭头函数的 return 注入（实施时定位）。

2. **B2 cwd 缩写**：
   - 新建 `ash-gui-auto/src/front/lib/path.at`：`pub fn abbreviate(p: str, home: str) -> str`（home 前缀→`~`，`\`→`/`），照搬 `ash-gui-vue/src/lib/path.ts`。
   - `app.at` 标题栏 cwd 显示 + `prompt_bar.at` cwd_display 改调 `abbreviate(.cwd, .home)`。
   - 依赖 M1（`starts_with` 映射）。

3. **B3 copy 兜底**（`block_item.at:118-120`）：
   - `.CopyCommand` handler 的 `navigator.clipboard.writeText(...)` 加可选链 + catch：
     `.at` 里写 `navigator.clipboard?.writeText(.command)`（若 .at 支持?.）；否则 module fn 包 try/catch。
   - 实施时按 .at 的 FFI 语法调整。

4. **B4 Progress 数值兜底**（`block_body.at` RecordView 的 MemoryInfo 分支，约 `:100-134`）：
   - `Number(field)` 前先剥尾部 `%` + `isFinite` 校验：
     - module fn `parse_percent(s: str) -> int`：`if s.ends_with("%") { s = s.substr(0, s.len()-1) }; var n = s.to_int(); if n 无效 → 0; return n`
   - 依赖 M1（`ends_with`/`substr` 映射）。

**验收**：
- B1：耗时正常显示 `123ms` / `1.5s`，不出现 undefined
- B2：cwd 显示 `~/src/foo` 而非全路径
- B3：非安全上下文（http）点复制不抛异常
- B4：MemoryInfo `usage_percent: "75%"` 正确渲染进度条而非 NaN

---

### M7: 验证

1. **codegen**：auto-lang worktree 重新生成 vue 产物（`auto build` 或对应命令）
2. **类型**：`vue-tsc --noEmit` 0 错误（重点查 M1 的方法映射、M3 的 HighlightSpan 类型）
3. **构建**：`vite build` 成功
4. **VM 回归**：`auto run -r vm` 启动正常，新增字段不影响 VM（computed 多行 body 避开 VM，用 module fn 模式）
5. **功能验收**：逐项跑 §M2–M6 的验收清单
6. **curl 链路**：确认 SSE 闭环未被破坏（`POST /api/run_command` + `GET /api/stream`）

---

## 3. 改动文件清单

| 文件 | 仓库 | 里程碑 | 改动 |
|---|---|---|---|
| `crates/auto-lang/src/ui_gen/vue.rs` | auto-lang worktree | M1 | method map 补全字符串方法映射 |
| `crates/auto-lang/src/ui_gen/ts_adapter.rs` | auto-lang worktree | M1 | `method_map_decision` 对齐 |
| `ash-gui-auto/src/front/prompt_bar.at` | auto-shell | M2/M3/M4/M6 | ghost text + 高亮 overlay + 续行 + cwd 缩写 |
| `ash-gui-auto/src/front/lib/highlight.at` | auto-shell | M3 | 新建：tokenize 状态机 |
| `ash-gui-auto/src/front/lib/path.at` | auto-shell | M6 | 新建：abbreviate |
| `ash-gui-auto/src/front/block_item.at` | auto-shell | M6 | duration_label / copy 兜底 |
| `ash-gui-auto/src/front/block_body.at` | auto-shell | M6 | parse_percent 兜底 |
| `ash-gui-auto/src/front/app.at` | auto-shell | M6 | cwd 缩写显示 |
| `crates/auto-lang/src/ui_gen/vue.rs` | auto-lang worktree | M4 | `user_class_skip_elements` 加 textarea |
| `crates/auto-lang/src/ui_gen/vue.rs` | auto-lang worktree | M5 | input handler debounce 注入（方案 1） |

> 新增 `lib/` 目录需确认 .at 的 use 导入路径约定（参照 `use back.api: complete`）。

---

## 4. 里程碑与验证总表

| 里程碑 | 内容 | 改动层 | 验证 | 依赖 |
|---|---|---|---|---|
| **M1** | codegen 字符串方法映射 | auto-lang | HistorySearch `to_lower→toLowerCase` + 单测 | 无（前置） |
| **M2** | ghost text + Ctrl+F/→ | .at | 输入出灰字、接受建议 | M1 |
| **M3** | 输入框语法高亮 | .at + 新 module | 命令/串/注释异色 | M1 |
| **M4** | 多行续行 | .at + 小 codegen | `·` 提示符、续行换行 | M1 |
| **M5** | 补全 debounce | auto-lang | 连打只请求一次 | 无 |
| **M6** | 渲染打磨批 | .at + 新 module | 见 §M6 验收 | M1（B2/B4） |
| **M7** | 验证 | — | 全链路回归 | M1–M6 |

**建议顺序**：M1（解锁）→ M6（顺手修 B5 + 低风险打磨）→ M2 → M3 → M4 → M5 → M7。
M6 放 M1 后是因为 B5 被 M1 自动修复，B2/B4 依赖 M1 的字符串映射。

---

## 5. 依赖与联动

- **M1 是 M2/M3/M6(B2,B4) 的前置**——字符串方法映射不修，这些功能的 vue 产物全是坏 JS。
- **M2/M3 共用 overlay 容器**（input 外的 `relative`+`absolute` span）——M2 先搭壳，M3 在壳里填高亮 span + ghost text。
- **M3/M4 都用 module fn 模式**（tokenize / needs_continuation / abbreviate）——遵循 `shell_store.at:15-55` 的既定模式，避开 VM 的 computed 多行 body 风险。
- **M5 独立**（codegen debounce 注入，不碰 .at 逻辑）。
- **与 Plan 052 的关系**：052 已让结构化 output 完整透传，本计划不动 SSE/store 传输层，只动视图与输入。
- **auto-lang worktree 协调**：M1/M4/M5 改 auto-lang，需注意 TODO.md:24（另一个 agent 在 master 实时改 auto-lang）——基于最新 master 分支开 worktree。

---

## 6. 后续工作面（本计划范围外，记录备查）

### C. 类型安全（store any → 强类型）
- **现状**：`gen/.../useShellStoreStore.ts` 的 10 个 ref + ~25 个 handler 参数全 `any`（codegen 有意简化，用户自定义类型在 TS 端擦除）。
- **根因**：codegen 的类型透传策略。`api.ts`（从 `back/api.at` 生成）已有完整 interface，但 store composable 没复用。
- **方向**：让 store codegen 复用 `api.ts` 的 interface 标注 ref/handler 参数。工作量较大，独立计划。

### D. VM 模式完善（auto 版独有工作面）
auto 版的 VM（merged）模式是 vue 原版没有的能力，但其自身有多处限制（Plan 044/049）：
- D1 命令执行绕过 ash-core（循环依赖阻断，renderer 自解析 stdout）
- D2 流式输出未做（merged_exec_loop 同步阻塞 UI）
- D3 命令取消缺失
- D4 复制按钮 no-op（`navigator.clipboard` 在 VM 软失败）
- D5 block 折叠全局（统一 root state，点一个全折叠——需 per-instance state）
- D6 `prompt_context` mock（git FFI 有 VM bug）
- **底层技术债**：`__sse_*` 预置字段 hack + codegen SSE dispatch 硬编码映射（Plan 050 §3.5 / 051 §4 登记）——待 VM renderer 支持多参 handler 后，方案 ⑤ 统一清除。

> D 类受 VM renderer 能力限制，属另一工作面，建议单独立计划。

### 共同缺口（两版都缺，非 auto 独有）
- 主题切换 UI（CSS 变量已备好但无 toggle）
- SSE onerror 重连
- 输出复制/导出、block 删除/固定
- 数字退出码显示

---

## 7. 参考文件

**vue 原版（移植源头）**：
- `ash-gui-vue/src/components/input/PromptBar.vue`（ghost text `:299-354`、高亮 `:289`、续行 `:104-190`、debounce `:60-85`、快捷键 `:229-284`）
- `ash-gui-vue/src/lib/highlight.ts`（tokenize 状态机，168 行）
- `ash-gui-vue/src/lib/path.ts`（abbreviate）
- `ash-gui-vue/src/components/block/renderers/RecordView.vue:17-34`（usage_percent 解析）

**auto 版（改动目标）**：
- `ash-gui-auto/src/front/prompt_bar.at`（主战场，A1–A6）
- `ash-gui-auto/src/front/history_search.at:19`（overlay 手法已验证范例）
- `ash-gui-auto/src/front/shell_store.at:15-55`（module fn + handler 写字段模式范例）
- `ash-gui-auto/src/front/block_item.at` / `block_body.at`（M6 打磨）

**auto-lang codegen（M1/M4/M5）**：
- `auto-lang/crates/auto-lang/src/ui_gen/vue.rs:5463-5476`（字符串方法映射，M1）
- `auto-lang/crates/auto-lang/src/ui_gen/vue.rs:4909-4918`（user_class_skip_elements，M4 textarea）
- `auto-lang/crates/auto-lang/src/ui_gen/vue.rs:5069-5130`（class vs style emit 规则，M3 动态 class）
- `auto-lang/crates/auto-lang/src/vm/native_catalog.rs:710-745,893-904`（VM 字符串方法全表）

**历史 plan**：
- 043（auto 版逆向生成）、049（VM UI 完善/折叠）、050（双模分析）、051（前后端拆分）、052（Table 透传）
