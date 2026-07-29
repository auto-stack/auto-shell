# P1 实施: 结构化命令 bash 兼容输出模式

**目标**: 新增 `--bash-compat` flag,让 ash 的结构化命令(ls/grep/wc/ps)输出 bash 风格纯文本而非 ratatui 表格,使真实 shell 命令的 stdout 能与 bash parity。

**架构**: 照抄现有 `--json` 三件套(global flag → Shell 字段 → `format_output` 分支),新增 per-AtomType 经典格式器。数据流不变(命令仍产出同样的 Obj 数组,只改渲染)。

## 已确认的设计决策
- **ls**: 每行一个 name(ls -1 风格)
- **wc**: 纯数字(对应管道形式 `cat f | wc -l` → `3`,不带文件名)
- **grep**: 输出匹配行 text 字段(每行一个);`-n` 时前缀行号
- **ps**: 经典列格式

## 关键字段映射(已调研确认)
- FileList: `Value::Array<Value::Obj>`,字段 `name`(Str,恒存)
- MatchList: `Value::Array<Value::Obj>`,字段 `text`(Str,恒存)/`line_number`(Int,仅 -n)/`file`(Str)
- CountResult: `Value::Obj` 或 `Value::Array`,字段 `lines`/`words`/`bytes`/`chars`(Int)
- ProcessList: `Value::Array<Value::Obj>`,字段 `pid`(Int)/`name`(Str)/`command`(Str,仅 -l)

---

## Task 1: Shell 字段 + setter (shell.rs)

**Modify**: `ash/auto-shell/src/shell.rs`

- [ ] 加字段 `bash_compat: bool`(行 136 `json_output` 后)
- [ ] 构造初始化 `bash_compat: false`(行 317 `json_output: false` 旁)
- [ ] 加 setter `set_bash_compat`(行 936 `set_json_output` 后,照抄)

## Task 2: 写 bash 格式器 (value_helpers.rs)

**Modify**: `ash-core/src/cmd/value_helpers.rs`(复用同模块私有 `format_value_for_table`)

新增 `pub fn format_atom_as_bash(atom_type: AtomType, value: &Value) -> Option<String>`,按 atom_type 分发:
- `FileList`/`FileEntry`: value=Array/Obj,提取每个 `name` 字段,每行一个 → Some
- `MatchList`: 提取每个 `text` 字段;若 `line_number` 存在则前缀 `行号:text` → Some
- `CountResult`: Obj 则取 `lines`(或首个计数字段);Array 则取 total 行的计数 → 纯数字 → Some
- `ProcessList`/`ProcessEntry`: 经典 `  PID NAME` 列(有 command 则加 COMMAND 列) → Some
- `Path`: 原样字符串 → Some
- 其他(Table/Record/SystemInfo/...): → None(落 fallback)

**测试**(value_helpers.rs mod tests): 为 FileList/MatchList/CountResult/ProcessList 各加一个单测,构造典型 Atom 数据,断言 bash 风格输出。

## Task 3: format_output 注入分支 (shell.rs)

**Modify**: `ash/auto-shell/src/shell.rs:880` `format_output`

在 `json_output` 分支(行 883)之后、表格分支(行 887)之前,插入:
```rust
if self.bash_compat {
    if let AtomPipeline::Atom(ref atom) = pipeline {
        if let Some(rendered) = ash_core::cmd::value_helpers::format_atom_as_bash(
            atom.atom_type, &atom.value) {
            return rendered;
        }
    }
    return pipeline.into_text();
}
```

## Task 4: CLI flag (main.rs)

**Modify**: `ash/auto-shell/src/main.rs`(照抄 `--json`)

- 预扫描(行 40 后): `let bash_compat = args.iter().any(|a| a == "--bash-compat");`
- match skip(行 49 后): 加 `"--bash-compat" => { i += 1; continue; }`
- `-s` 路径(行 107): `shell.set_bash_compat(bash_compat);`
- script 路径(行 181): `shell.set_bash_compat(bash_compat);`
- `-c` 路径(行 75): `execute_for_agent` 会 reset,需给它加 `bash_compat` 参数,或改为 set+execute。**推荐**: 扩 `execute_for_agent(input, json_mode, bash_compat)` 签名。
- help 文本(行 135 后): 加 `--bash-compat` 说明

## Task 5: execute_for_agent 签名扩展 (shell.rs)

**Modify**: `ash/auto-shell/src/shell.rs:911` `execute_for_agent`

改签名为 `(input, json_mode, bash_compat)`,内部 set bash_compat 并在结尾 reset(照抄 json_output 的 reset 模式)。更新所有调用点(main.rs -c 路径 + shell.rs 内的单测)。

## Task 6: parity case (真实 shell 命令)

**Create**: `tests/parity/cases/{51_ls_bash,52_grep_bash,53_wc_bash}/`

每个 case: ash 版用 `--bash-compat` 跑真实命令(`> ls`/`> grep`/`> wc`),bash 版用真实命令。生成 expected.txt,实测对齐。
- 注意: harness 的 `run_ash` 当前不传 `--bash-compat`。需让这些 case 的 ash 脚本在脚本内启用(若有脚本级 API),或让 harness 检测 case 目录有无 `bash-compat` 标记文件来传 flag。**推荐**: 在 case 目录放一个 `bash_compat` 空标记文件,harness 检测到则给 ash 子进程加 `--bash-compat` flag。

## Task 7: 验证 + 回归

- [ ] `cargo build` 通过
- [ ] 新 case 51-53 通过(strict 模式)
- [ ] 现有 50 case 不回归(`cargo test --test parity` 仍 50/50)
- [ ] value_helpers 单测通过
- [ ] 更新 plans/036 标记 P1 完成

## 风险点
- Task 5 的 `execute_for_agent` 签名改动会影响调用点,需仔细更新
- Task 6 的 `--bash-compat` 传递需 harness 配合(标记文件方案)
- CountResult 的 Array/Obj 双形态需稳健 match