# Plan: ash 脚本 Parity 测试框架

## 目标

建立 ash 脚本能力与 bash/PowerShell/fish/nu 的 parity 测试框架，验证"同一逻辑用不同 shell 语言写，输出一致"。v1 做 50 个用例。

## 文件结构

```
tests/parity/
├── README.md                    # 框架说明 + 用例索引
├── harness.rs                   # Rust 集成测试入口（cargo test --test parity）
├── normalize.rs                 # 输出归一化（去 ANSI/路径/换行差异）
└── cases/                       # 50 个用例目录
    ├── 01_echo/
    │   ├── desc.md              # 描述
    │   ├── ash.ash              # ash 版
    │   ├── bash.sh              # bash 版
    │   ├── pwsh.ps1             # PowerShell 版
    │   ├── fish.fish            # fish 版
    │   ├── nu.nu                # nushell 版
    │   └── expected.txt         # golden output（从 bash 生成）
    ├── 02_variables/
    │   └── ...
    └── ...（共 50 个）
```

## 用例分类（50 个，v1）

### A. 基础命令（10 个）
1. echo 输出 | 2. 变量赋值与引用 | 3. 命令替换 | 4. 管道 | 5. 重定向
6. 退出码 $? | 7. 环境变量 | 8. 多命令串联(&&) | 9. 多命令串联(||) | 10. 子shell/分组

### B. 字符串操作（8 个）
11. 字符串拼接 | 12. 字符串长度 | 13. 子串提取 | 14. 字符串替换
15. 大小写转换 | 16. 分割字符串 | 17. 去空白 | 18. 字符串包含检查

### C. 条件与循环（10 个）
19. if-else | 20. if-elif-else | 21. for 循环(遍历列表) | 22. for 循环(范围)
23. while 循环 | 24. break | 25. continue | 26. 嵌套循环
27. 条件测试(文件存在) | 28. 条件测试(字符串比较)

### D. 函数（5 个）
29. 函数定义与调用 | 30. 函数参数 | 31. 函数返回值 | 32. 递归 | 33. 局部变量

### E. 文件操作（8 个）
34. 创建文件 | 35. 读取文件 | 36. 追加写入 | 37. 文件存在检查
38. 文件大小 | 39. 行数统计 | 40. 文件复制 | 41. 目录创建

### F. 数据处理（5 个）
42. grep 搜索 | 43. sort 排序 | 44. uniq 去重 | 45. head/tail | 46. wc 统计

### G. 错误处理（4 个）
47. try-catch(ash) vs trap(bash) | 48. 命令失败处理 | 49. 空输入处理 | 50. 数学运算

## 测试 harness 设计

### 执行流程
```
对每个用例目录:
  1. 读 ash.ash → Shell::execute(ash_content) → 拿到 stdout
  2. 读 bash.sh → std::process::Command("bash") → 拿到 stdout
  3. 归一化两边输出（normalize）
  4. assert_eq!(ash_normalized, bash_normalized)
  5. 如果 pwsh.ps1 存在且系统有 pwsh → 同样比较
  6. 如果 fish.fish 存在且系统有 fish → 同样比较
  7. 如果 nu.nu 存在且系统有 nu → 同样比较
```

### 归一化规则（normalize.rs）
- 去 ANSI 颜色码（`\x1b[...m`）
- 统一换行（CRLF → LF）
- 去 trailing whitespace
- 绝对路径替换为 `<TMPDIR>`（跨平台路径差异）
- 时间戳替换为 `<TIME>`（如果有）

### 跳过策略
- 如果系统没装某 shell（fish/nu/pwsh），自动跳过该 shell 的比较（不 fail）
- 用例可以标注 `skip_shells: [nu]` 在 desc.md 里，主动排除不适用的 shell

### 用例注册
用 macro 或 glob 自动发现 `cases/*/ash.ash`，每个目录一个 `#[test]`。不需要手写 50 个 `#[test]` 函数。

## 实施步骤

1. **harness + normalize** — 写 `tests/parity/harness.rs` + `normalize.rs`
2. **3 个示范用例** — 手写 01_echo / 02_variables / 04_pipe，验证 harness 工作
3. **批量写 50 个用例** — 按 7 个分类批量创建，每个含 ash + bash + expected（pwsh/fish/nu 视适用性）
4. **CI 集成** — 在 ci.yml 加 `cargo test --test parity`

## 关键约束

- ash 的输出用 `Shell::execute`（in-process），其他 shell 用 subprocess
- 输出必须归一化——跨平台/跨 shell 的 ANSI/路径/换行差异是噪音
- v1 只确保 bash parity（最核心），pwsh/fish/nu 作为 best-effort（有就比，没有跳过）
- expected.txt 从 bash 运行结果生成（bash 是 oracle）
