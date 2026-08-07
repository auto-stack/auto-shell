# ash-gui VM 诊断续接指南

> 目的:让任何会话/任何人在**修完已发现的 VM bug 后**,能从当前断点无缝继续诊断,
> 不需要重走三轮二分。本文是「断点快照」;VM 兼容性修复计划在
> `designs/ash-gui-vm-fix-plan.md`(改 bug 的方案),auto-lang 侧的计划是
> `auto-lang/docs/plans/398-vm-expose-and-store-sibling-handler.md`(Plan 398)。

## 0. 一句话现状(最新,2026-08-07 Plan 398 深入诊断后)

> ⚠️ **本文 §1–§6 的"3 个 VM bug(BUG-A/B/C)"框架部分已被 Plan 398 §11 修正**。
> 深入诊断(加 eprintln)后发现:ash-gui vm 启动失败主要是 **.at parse 错误被
> 静默吞掉**,不是 VM 链接/作用域 bug。**以本节及 Plan 398 §11 为准;§1–§6 作历史。**

**真正的根因(Plan 398 §11,已验证)**:
1. `collect_module_imports`(`auto-lang/lib.rs:2290`)的 `Err(_) => return` 静默吞
   parse 错误 → 整模块符号消失 → 下游误导性 `Undefined symbol: api.X`。**已修
   (Plan 398 commit `25642f91`,改 log::warn)**。
2. **ash-gui 侧两个 .at parse 错误**(下一步修):
   - `back/api.at`:`[][]T`(`rows: [][]RenderedCell`)与 `[](tuple)`
     (`fields: [](str, RenderedCell)`)不被 Core parser 支持 → 改 `[]T` 即可 parse。
   - `front/shell_store.at:29`:`git_status: None` 与 `GitStatusInfo` 类型不匹配 →
     改空 struct 或 `?GitStatusInfo`。
3. **真正的 BUG-C**(`PromptBar_State.Exit`):修了上面两个 parse 错误后才暴露。
   §2 诊断已确认 synthesize 为 PromptBar 生成了全部 13 handler + `expose=["Exit"]`
   被 parser 正确填充 → BUG-C 根因在 **linker/vm_bridge 的 `<Child>_State.<Handler>`
   符号查找**,不在 synthesize。

**修正后的续接步骤(替换 §4)**:
1. (已做)Plan 398 lib.rs parse 错误改 log::warn。
2. (已做,ash-gui .at)shell_store.at 的 `git_status: None` → 全零 GitStatusInfo struct。
3. (**需 Core parser 修复,Plan 398 §12**)api.at 的 `[][]T`(`rows: [][]RenderedCell`
   等)与 `[](tuple)`(`fields: [](str, RenderedCell)`)不被 Core parser 支持。
   **注意:不能用 `[][]T` → `[]T` 的 .at workaround**——这破坏 Vue 表格语义(行数组
   变一维)。正确做法是给 Core parser 加 `[][]T` / `[](tuple)` 支持(§12)。这是 vm
   跑通的前提,绕不过去。
4. (Plan 398 §2 BUG-C)修 `<Child>_State.<Handler>` 符号查找(§12 完成后才会暴露)。
5. ash-gui vm 启动验证 → 回 ash-gui-native-plan M0.5。

---

## 0.历史 一句话现状(2026-08-07 三轮诊断后,已被上文 §0 修正)

ash-gui-auto 在 `auto run -r vm` 下**仍无法启动**,但已定位到 **3 个独立 VM bug**
(BUG-A/B/C)。其中 BUG-A/B 的 .at workaround 已应用(已提交),BUG-C + 残留
`api.complete` 是当前阻塞。**修完 VM bug 后,回到 §4 的"续接步骤"继续。**

## 1. 三个 VM bug(已定位,带证据)

### BUG-A:store 的 `use back.api` 导入不透传到 App 作用域 ✅ workaround 已应用

- **症状**:`Undefined symbol: api.command_list in module App`(逐个 back.api fn 报)。
- **根因**:VM 把 store handler body 链接到 App 模块作用域时,store 的
  `use back.api: ...` 导入**不透传**。015-notes 的 app.at **自己也 `use back.api`**
  (第 7 行)——所以它能跑。ash-gui app.at 原本没加,故崩。
- **性质**:文档/惯例缺口(不是 VM 代码 bug,是约定没写明)。
- **workaround(已应用,commit 43f79a8)**:`app.at` 加
  `use back.api: command_list, history, complete, run_command, run_smart, prompt_context, open_path`。
- **彻底方案**:补 auto-ui-creator skill 文档(U1 加一条),或让 VM 自动透传 store
  导入(改 `vm_bridge.rs`,但工作量较大,不推荐——惯例 workaround 已够)。

### BUG-B:store handler 调用另一个 store handler `.Sibling()` → `<Store>_State.X` 未定义 ✅ workaround 已应用

- **症状**:`Undefined symbol: ShellStore_State.RefreshGit in module App`。
- **触发**:`shell_store.at` 的 `.Init` 与 `.RunResult` 内部调 `.RefreshGit()`
  (store 自己的另一个 handler)。VM 把这种调用解析成 `ShellStore_State.RefreshGit`
  state-struct 符号,但该符号未生成 → link 失败。
- **根因**:`handler_codegen.rs:103-110` 的 rewrite 只覆盖 **`store.Method()`(store
  别名调用,如 `store.Init()`)**,不覆盖 **store handler 内部的 `.Sibling()`**
  (走 `__state.field` 那套 rewrite,见 `handler_codegen.rs:60-70`,但 `.Sibling()`
  是调用不是字段,落到未实现分支)。
- **性质**:真 VM bug(store handler 间互调未支持)。
- **workaround(已应用,commit 43f79a8)**:把 `.RefreshGit()` 调用**内联**为
  `.git_info = prompt_context()`(`.Init` 与 `.RunResult` 两处)。
- **彻底方案**:见 VM 兼容性修复计划 BUG-B 节。

### BUG-C:子组件 handler 仅被内部引用(非模板直接绑定)→ `<Child>_State.X` 未定义 ❌ M0 真阻塞

- **症状**:`Undefined symbol: PromptBar_State.Exit` → 修一个变
  `PromptBar_State.PickCompletion` → 逐个 PromptBar 内部 handler 都报。**系统性**,
  非单点。
- **触发**:子组件(如 PromptBar)的 handler 仅在 handler 逻辑里被调、模板未直接绑定
  (PromptBar 的 `expose { .Exit }` + 内部 `.Exit()` 调用;以及所有
  `PickCompletion`、`AcceptGhost` 等内部 handler)。VM 对子组件的 state-struct
  handler 查找要求该 handler 被**模板直接引用**,否则 `Child_State.Handler` 符号不生成。
- **根因(决定性证据)**:`AuraWidget.exposes: Vec<Name>` 被 parser 正确填充
  (`parser.rs:11326/11366`,调 `parse_expose_block_inner` `parser.rs:11630`),
  但 **VM 运行时从不读取它**——
  `grep -rn '\.exposes' crates/auto-lang/src/ui/{handler_codegen,vm_bridge,dynamic}.rs`
  → 0 匹配。`expose` 本该是"额外强制生成 handler 符号"的清单,但被解析后丢弃,
  VM 不会因 expose 多生成任何 `handler_<Child>_<X>` 符号。
  (注:之前误判的 `vm_bridge.rs:1084` 等 `exposes: vec![]` 是 `#[test]` 块里的
  测试 struct literal,不是生产代码——已复核更正。)
- **性质**:真 VM 功能缺口(`expose` 未实现)。
- **workaround(重,未应用)**:每个 PromptBar 内部 handler 都要"模板可见"——
  要么在 view 里加隐藏 dummy 引用,要么大改 PromptBar。**不推荐**(PromptBar 有
  ~10 个此类 handler)。
- **彻底方案**:见 VM 兼容性修复计划 BUG-C 节。

## 2. 已排除的假因(不要再试)

这些都被三轮二分证伪,续接时**不要重试**:

| 假因 | 证伪证据 |
|---|---|
| 19 个 pub type 致 link 失败 | 完整 19 type 在 stub app 上全 link(tests A-F);换 015 api.at 也 link |
| 复杂类型特性(`?T`/`[][]`/tuple 数组) | 单独测全 link OK |
| `~Stream<T>` 返回类型 | 注释掉仍失败 |
| 缺 `use shell`(后端模块导入) | 加了仍失败(真因是 BUG-A) |
| 后端模块名 `shell` vs `db` | rename 仍失败 |
| `use types:` vs `use api:` | 都试过,无关 |
| front/types.at 与 api.at 的 type 重名 | 禁用 front/types.at 仍失败 |
| struct-fix 模式(`Type{}`+字段赋值) | 单独测全 link,不触发 panic |
| 循环变量字段赋值(`b.status = st`) | 单独测 link OK |
| store 完整性(所有 handler body) | stub app + 完整 store 全 link(test G) |
| `pac.at` 的 `api`/`render` 字段 | 与 015 一致,无关 |
| api.at 本身 | 完整 api.at 在 stub app 上全 link |

## 3. harness 复现脚本(续接起点)

harness 是"干净可复现的基准"——stub app + 完整 store + mock 后端。从这里逐项加回,
定位每个 bug。**续接时先重建 harness 确认基准仍 link,再加回 real 内容。**

### 基准(应 link 通过,看到 `AutoUI MCP: first state sync`)

```bash
cd D:/autostack/auto-shell/ash-gui/ash-gui-auto

# stub app.at(无 store 调用、无子组件)
cat > src/front/app.at <<'EOF'
widget App {
    msg Msg { Init }
    model { var x int = 0 }
    view { col { text "ash" } }
    on { .Init -> { } }
}
EOF

# 完整 store(所有 handler,但 app 不调它)+ mock shell.at(commit 已含)
# → auto run -r vm 应看到 "AutoUI MCP: first state sync in view()"
(timeout 12 auto run -r vm > /tmp/probe.log 2>&1 &); sleep 10
grep -iE 'first state|undefined|panic' /tmp/probe.log | head -3
taskkill //F //IM auto.exe 2>/dev/null
```

### 加回顺序(每步 `auto run -r vm` 测一次,定位首个失败)

1. **加 `use shell_store: ShellStore` + `store.Init()` 到 app.at** → 命中 BUG-A
   (`api.command_list`)。
2. **app.at 加 `use back.api: ...`** → BUG-A 消失,前进。
3. **加回 real store 的 `.Init` body(调 `command_list()`/`history()`)** → 若仍
   `api.X`,是残留问题(见 §4 决策点);若前进到 `ShellStore_State.X`,是 BUG-B。
4. **去掉 store 内部 `.Sibling()` 调用** → BUG-B 消失,前进。
5. **加回子组件(ToolSidebar / BlockList / PromptBar)** → 命中 BUG-C
   (`Child_State.X`)。逐个加,PromptBar 是重灾区。

### 关键判别

- 错误形如 `api.<fn>` 或 `<fn> in module App` → back.api 透传问题(BUG-A 类)。
- 错误形如 `<Store>_State.<Handler>` → store handler 互调(BUG-B)。
- 错误形如 `<Child>_State.<Handler>`(Child 是子组件名)→ expose/内部 handler(BUG-C)。
- 错误形如 `Expected term, got RBrace` → .at 语法问题(如 `[]` 空数组字面量作参数
  不被某些路径接受),改用变量。
- panic `codegen.rs:6058: Assignment to complex LHS` → VM 不支持某种 LHS 赋值形式
  (本轮未复现到,但二轮见过;若出现,grep `Expr::Dot` 分支要求 obj 是 `Expr::Ident`)。

## 4. 当前断点 + 续接决策点

**当前状态(应用 BUG-A/B workaround 后)**:运行 `auto run -r vm` 报
`Undefined symbol: api.complete in module App`。

**未决问题**:这个 `api.complete` 是
- (a) BUG-C 的另一种表现(store 的 `.Complete` handler return 值 + PromptBar 链)?还是
- (b) 独立的第四个 bug(store handler 的 `return` 语句在 VM 不支持)?

**续接第一步(改完 VM bug 后)**:
1. 先确认 VM 修复是否解决了 `api.complete`(若是 BUG-C 相关,修 expose 后可能自然消失)。
2. 若仍存在,二分:把 store 的 `.Complete` handler body 清空(`.Complete(l,c) -> { }`),
   看 `api.complete` 是否消失 → 判定是否 `return` 语句问题。
3. 修通后,按 §3 加回子组件,逐个验证 BUG-C 修复。

## 5. 关键 file:line 索引

**ash-gui-auto(本仓库,workaround 已提交)**:
- `src/front/app.at`:BUG-A workaround(第 11-13 行 `use back.api`)
- `src/front/shell_store.at`:BUG-B workaround(`.Init`/`.RunResult` 内联 RefreshGit)
- `src/back/shell.at`:mock 后端(VM merged 模式 in-process)
- `src/back/api.at`:`use shell` + 注释 `stream()`(M1 恢复)

**auto-lang(修 VM 的仓库,当前在 `plan-musk-022/markdown-mermaid-tag` 分支,需另开分支)**:
- BUG-C:`crates/auto-lang/src/ui/{vm_bridge.rs:1084, dynamic.rs:1074/1207/1403/1477/1670,
  aura_view_builder.rs:2823}`(7 处 `exposes: vec![]` 硬编码)
- BUG-B:`crates/auto-lang/src/ui/handler_codegen.rs:103-110`(store.Method rewrite,
  不覆盖 store 内部 .Sibling())
- handler dispatch:`crates/auto-lang/src/ui/vm_bridge.rs:668,800,831,869`
  (`call_handler` / `call_fn_by_name`)
- codegen panic 点(二轮见过):`crates/auto-lang/src/vm/codegen.rs:6058`

**诊断证据归档**:`designs/ash-gui-native-plan.md` §9.1–§9.7(三轮二分全过程)。

## 6. auto-lang 仓库状态提醒

- auto-lang 当前分支:`plan-musk-022/markdown-mermaid-tag`,有未提交的无关改动
  (`examples/ui/017-chat/`)。
- **修 VM 前必须**:从干净 master 另开分支(如 `fix/vm-expose-and-store-sibling`),
  不要在当前分支动。
- auto-lang 有自己的 plan/commit 规范(见其 `docs/plans/`),修 VM 走它的流程。

## 7. 不要做的事

- 不要重试 §2 的假因(已证伪)。
- 不要逐个 workaround BUG-C 的 PromptBar handler(~10 个,不现实)——等 VM 修复。
- 不要在 auto-lang 的当前分支(`plan-musk-022/...`)直接改。
- 不要改 ash-gui-vue(参照基准,不动)。
- 不要为"快速通 UI"过度简化 PromptBar(会丢失 ghost/completion/highlight,背离
  "UI/UX 一致"目标)——除非 VM 修复实在排不上,再做降级决策。
