# VM 兼容性修复计划(ash-gui-native M0 阻塞)

> 目的:修复阻塞 ash-gui 在 vm 模式启动的 3 个 VM bug,让 `auto run -r vm` 能开窗 +
> MCP 可连,从而解锁 M0(MCP 连通 + 测试骨架)及后续 M1–M4。
>
> **配套文档**:
> - 诊断全过程的证据:`designs/ash-gui-native-plan.md` §9.1–§9.7
> - 改完 bug 后的续接指南:`designs/ash-gui-vm-diagnosis-resumption.md`
> - 本文档:每个 bug 的**修复方案**(改哪个文件/函数/为什么)

## 0. 三个 bug 一览

| Bug | 一句话 | 性质 | 方案 |
|---|---|---|---|
| **BUG-A** | App 调 store.X() 时,store 的 `use back.api` 不透传到 App 作用域 | 惯例缺口 | .at workaround 已应用 + 补 skill 文档(不修 VM) |
| **BUG-B** | store handler 内部调 `.Sibling()` 另一个 store handler → `<Store>_State.X` 未定义 | 真 VM bug | 修 `handler_codegen.rs`(补 rewrite 规则) |
| **BUG-C** | 子组件 handler 仅被内部引用(非模板)→ `<Child>_State.X` 未定义;`expose{}` 被解析但 VM 运行时从不消费 | 真 VM 功能缺口 | 修 VM 运行时(让 `exposes` 生效) |

**关键修正(写计划时复核源码发现)**:之前误判"expose 硬编码 vec![]"——那些是
`#[test]` 块里的测试 struct literal,不是生产代码。真相是:`AuraWidget.exposes`
**被 parser 正确填充**(`parser.rs:11326/11366/11630`),但 **VM 运行时**
(`handler_codegen.rs` / `vm_bridge.rs` / `dynamic.rs`)**从不读取它**——
`grep '\.exposes' crates/auto-lang/src/ui/{handler_codegen,vm_bridge,dynamic}.rs`
三处全空。所以 expose 被解析后丢弃,VM 不会因 expose 而多生成任何 handler 符号。

## 1. 执行前提

- **auto-lang 仓库另开分支**:当前在 `plan-musk-022/markdown-mermaid-tag`(有未提交的
  无关改动)。从干净 master 开分支:`fix/vm-expose-and-store-sibling`。
- **遵循 auto-lang 的 plan/commit 规范**(见 `docs/plans/`):每个 bug 一个 plan +
  对应 commit,带回归测试。
- **不动 ash-gui-vue**(参照基准)。
- ash-gui-auto 的 BUG-A/B workaround(commit 43f79a8)**保留**——它们是合法的 .at 写法,
  即使修了 VM 也不冲突(BUG-A 本就是惯例;BUG-B 内联是简化)。

## 2. BUG-C 修复(优先级最高,M0 真阻塞)

### 现象
- 子组件 handler 仅被内部引用(非模板直接绑定)→ link 报
  `<Child>_State.<Handler> in module App` 未定义。
- PromptBar 系统性重灾区:`~10 个`内部 handler(`PickCompletion`、`AcceptGhost`、
  `Exit` 等)逐个报错。

### 根因(已验证)
- `AuraWidget.exposes: Vec<Name>` 字段存在(`aura/types.rs` AuraWidget struct),
  parser 正确填充(`parser.rs:11326/11366`,调用 `parse_expose_block_inner`
  `parser.rs:11630`)。
- **VM 运行时从不读取 `widget.exposes`**:
  `grep -rn '\.exposes' crates/auto-lang/src/ui/{handler_codegen,vm_bridge,dynamic}.rs`
  → 0 匹配。
- 后果:`handler_codegen::synthesize_widget_module`(`handler_codegen.rs:172`,
  生成每个 widget 的 `handler_<Widget>_<Event>` 函数集合)只对**模板直接引用的**
  handler 生成符号。`exposes` 本该是"额外强制生成"的清单,但被忽略 → 仅内部引用的
  handler 没有对应 `handler_<Child>_<X>` 符号 → App link 时 `<Child>_State.X` 未定义。

### 修复方案
在 `handler_codegen.rs` 的 widget module 合成阶段,**把 `widget.exposes` 里的每个名
字当作"必须生成的 handler"加入合成集合**(即使模板未引用)。

**定位**(待实现时确认精确行):
- `crates/auto-lang/src/ui/handler_codegen.rs:172` `synthesize_widget_module`(主入口,
  接收 `widget: &AuraWidget`)。
- 该函数内部某处枚举"要生成哪些 `handler_<W>_<E>`"——很可能基于 `widget.handlers`
  的 keys(所有声明的 handler)而非模板引用。**需先确认**:如果它已经生成所有声明
  的 handler,那 BUG-C 的根因在别处(`VmBridge::call_handler` 的符号查找,或
  `<Child>_State` 的命名),而非 synthesize 阶段。

**实现步骤**:
1. **先做精确诊断(30 分钟)**:在 `handler_codegen.rs:172` `synthesize_widget_module`
   加 `eprintln!("handlers for {}: {:?}", widget.name, widget.handlers.keys().collect::<Vec<_>>())`,
   跑 ash-gui vm,确认 PromptBar 的 handler 集合是否含 `Exit`/`PickCompletion`。
   - 若**含**(synthesize 已生成)→ BUG-C 根因在 `vm_bridge.rs` 的符号查找/命名
     (`<Child>_State.X` 的生成),转去查 `call_handler`/`call_handler_for`(`vm_bridge.rs:668,800,831`)。
   - 若**不含** → synthesize 阶段确实漏了,补:`for name in &widget.exposes { ... 强制加入合成集合 }`。
2. **根据诊断结果改对应文件**(synthesize 或 vm_bridge)。
3. **回归测试**:在 `auto-lang/crates/auto-lang/tests/` 或 ui 模块 `#[cfg(test)]` 加
   一个最小用例:子组件带 `expose { .HiddenHandler }` + 内部调用,父组件引用子组件,
   断言 vm 能 link + 调用。
4. **验证**:在 ash-gui-auto `auto run -r vm`,确认 `PromptBar_State.X` 类错误消失
   (逐个 handler 前进,直到 `<Child>_State` 类错误清零)。

### 风险
- BUG-C 的根因可能在 `vm_bridge` 的符号命名约定(`<Child>_State.<Handler>` 这种
  `State.X` 形式暗示有个 per-child state struct,其字段 = handler 名)。修复时要理解
  这个命名约定是怎么生成的(可能在 `synthesize_widget_module` 或 `dynamic.rs`)。
- 若修复涉及 Child state struct 的字段生成逻辑,影响面较大——需充分回归(015-notes /
  013-todo 必须仍通过)。

## 3. BUG-B 修复(优先级中,workaround 已让 ash-gui 不触发,但应修)

### 现象
- store handler 内部调 `.Sibling()`(同 store 的另一个 handler)→ link 报
  `<Store>_State.<Sibling>` 未定义。

### 根因(已验证)
- `handler_codegen.rs:103-130` 的 `rewrite_expr` 处理 **`store.Method()`(store 别名
  调用)**:当 obj 是 store 别名(在 `STORE_WIDGET_NAMES` 注册),rewrite 成
  `handler_<StoreName>_<Method>(__state, args)`。
- 但 **store handler 内部的 `.Sibling()`**(obj 是隐式 self,不是 store 别名)走的是
  `rewrite_state_refs_stmts`(`handler_codegen.rs:60-70`)那条路径——它把 `.field`
  rewrite 成 `__state.field`(**字段访问**),但 `.Sibling()` 是**调用**(`Expr::Call`
  包 `Expr::Dot`),被当成字段访问处理 → 生成 `__state.Sibling`(无意义)或落到未覆盖
  分支 → 符号未定义。

### 修复方案
在 `handler_codegen.rs` 的 `rewrite_expr`(或 `rewrite_state_refs_stmts`)中,补一条
**store handler 内部 `.Sibling()` 调用**的 rewrite 规则:
- 识别:`Expr::Call { name: Expr::Dot(obj, method), ... }` 且 obj 是隐式 self
  (`Expr::Self_` 或类似,即 store handler 体内的 `.X()`),且 `method` 是当前 store
  的某个 msg variant(即 sibling handler)。
- rewrite 成:`handler_<CurrentStoreName>_<Method>(__state, args)`(同 `store.Method()`
  的 rewrite 逻辑,复用 `STORE_WIDGET_NAMES` + 当前 widget 名)。

**定位**:`handler_codegen.rs:103-130`(现 `store.Method()` rewrite 处),在其前后补
sibling 分支。需获取"当前正在 rewrite 的 widget 名"(当前 rewrite 是 stmt 级,可能要
thread 一个 `current_widget_name` 参数进来,或用 thread-local 像 `STORE_WIDGET_NAMES`
那样)。

**实现步骤**:
1. 在 `rewrite_expr` 增加 sibling-handler-call 识别 + rewrite(复用 store.Method 的
   handler_fn 构造)。
2. 回归测试:最小 store 用例,`.A` handler 内部调 `.B`(`.B` handler 存在),vm 能 link +
   调用 `.A` 时 `.B` 也执行。
3. ash-gui 验证:`shell_store.at` 恢复 `.RefreshGit()` 调用(撤销 BUG-B workaround),
   确认不再报 `ShellStore_State.RefreshGit`。

### 风险
- 低:rewrite 规则局部,不影响其他路径。015/013 回归验证即可。

## 4. BUG-A 处置(不修 VM,补文档)

### 现象
- App 调 `store.X()` 时,store handler body 被链接到 App 作用域,store 的
  `use back.api: ...` 导入不透传 → `api.<fn>` 未定义。

### 处置
- **不修 VM**(让 VM 自动透传 store 导入涉及作用域模型大改,不值当)。
- **workaround 已应用**(commit 43f79a8):app.at 自己 `use back.api: ...`。
- **补文档**:更新 `skills/auto-ui-creator/SKILL.md` 的 U1(store 访问规则),加一条:
  > 当 App(或任意 widget)调 `store.X()`,而该 store 的 handler 用到了 `back.api`
  > 函数时,**调用方 widget 必须自己也 `use back.api: <用到的 fn>`**——VM 不透传
  > store 的导入。Vue/a2r 后端无此要求(它们的 codegen 自动处理)。
- 同时更新 `tests/probes/gotcha-probe.at` + `verify.sh` 加一条断言。

## 5. 残留 `api.complete` 的二分(BUG-C 修复前/后都要做)

### 现象
应用 BUG-A/B workaround 后,仍报 `Undefined symbol: api.complete in module App`。
未判定它是 BUG-C 的另一种表现,还是独立的第四个 bug(store handler 的 `return` 语句)。

### 二分步骤(改完 VM bug 后第一步)
1. 把 `shell_store.at` 的 `.Complete` handler body 清空:`.Complete(l,c) -> { }`。
2. `auto run -r vm`:
   - 若 `api.complete` 消失 → 是 `.Complete` body 的问题(很可能是 `return items`
     语句,或 `var items []CompletionItem = complete(...)` 的类型注解)→ 第四个 bug,
     单独修。
   - 若 `api.complete` 仍在 → 与 `.Complete` 无关,是别处引用 `complete`(继续二分
     store 各 handler)。
3. 记录结果到 `designs/ash-gui-vm-diagnosis-resumption.md` §4。

## 6. 执行顺序

```
[auto-lang 开分支 fix/vm-expose-and-store-sibling]
   │
   ├─ Step 1:BUG-C 精确诊断(synthesize vs vm_bridge)+ 修复 + 回归
   │          (解锁 ash-gui 子组件 link;最高优先)
   │
   ├─ Step 2:BUG-B 修复(handler_codegen 补 sibling rewrite)+ 回归
   │          (让 store handler 互调可用;中优先)
   │
   ├─ Step 3:回 ash-gui-auto,撤销 BUG-B workaround(恢复 .RefreshGit()),
   │          验证 BUG-B/C 修复生效;二分残留 api.complete(§5)
   │
   ├─ Step 4:BUG-A 补文档(auto-ui-creator skill U1 + probe)
   │
   └─ Step 5:ash-gui vm 完整启动验证 → 回 ash-gui-native-plan M0.5
              (MCP 连通 + 测试骨架)
```

## 7. 验收

- **BUG-C**:ash-gui-auto `auto run -r vm` 不再报任何 `<Child>_State.X`;PromptBar
  完整(含 expose + 内部 handler)能 link。
- **BUG-B**:`shell_store.at` 恢复 `.RefreshGit()` 后不再报
  `ShellStore_State.RefreshGit`。
- **回归**:015-notes、013-todo 的 `auto run -r vm` 仍正常开窗;它们的 MCP 测试
  (`examples/ui/015-notes/tests/desktop_mcp.py`)仍通过。
- **ash-gui**:vm 完整启动,`AutoUI MCP: listening on :9247`,
  `autoui_snapshot` 返回 App 树。

## 8. 非目标

- 不改 Vue/a2r 后端(它们没这些 bug)。
- 不重构 VM 作用域模型(BUG-A 用 workaround 足够)。
- 不做 SSE/in-process 后端(那是 M1,本计划只解 vm 启动 link 阻塞)。
- 不简化 PromptBar(BUG-C 修好后不需要;若 BUG-C 修复排不上,再单独决策降级)。

## 9. 风险与降级

- 若 BUG-C 修复影响面过大(动到 Child state struct 命名约定),**降级方案**:
  vm 模式先跑简化 PromptBar(去 ghost/completion/highlight,只留 input+run+history),
  把"完整 PromptBar"留给 a2r/HTTP 模式。这是最后手段,会偏离"UI/UX 一致"目标。
- 若 auto-lang 团队对修 VM 有自己的排期,ash-gui-native 的 M0 可先在
  "简化 PromptBar + vm"上跑通测试骨架,不等 BUG-C 完整修复。

## 10. 进度跟踪

- [ ] auto-lang 开分支 `fix/vm-expose-and-store-sibling`
- [ ] BUG-C 精确诊断(synthesize vs vm_bridge)
- [ ] BUG-C 修复 + 回归测试
- [ ] BUG-B 修复 + 回归测试
- [ ] 回 ash-gui 验证 + 撤销 BUG-B workaround + 二分残留 api.complete
- [ ] BUG-A 补 skill 文档
- [ ] ash-gui vm 完整启动验证
- [ ] 回 ash-gui-native-plan M0.5(MCP 连通 + 测试骨架)
