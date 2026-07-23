# Plan 036: ash 脚本 Parity 测试框架

> **日期**: 2026-07-23
> **状态**: 实施中
> **目标**: 建立 ash 脚本与 bash/PowerShell/fish/nu 的 parity 测试，验证跨 shell 行为一致性
> **范围**: 50 个用例，Rust 集成测试框架，4 个参照 shell

## 愿景

ash 的脚本能力（AutoLang）目标是替代 bash/fish/nu/pwsh 各自的脚本。为了证明这一点，需要系统的 parity 测试——**同一逻辑用不同 shell 语言写，执行后比较输出是否一致**。

## 参照的 auto-lang parity 体系

auto-lang 有三套测试：
- **System A**（per-transpiler golden）：Auto 源码 → 转译输出，byte 比较
- **System B**（conformance）：Auto 源码 → AutoVM 执行，与 golden stdout 比较
- **System C**（parity workspace）：同一测试经三种后端执行，TAP 比较

ash 的 parity 参照 System C 的思路，但参照物不是"同一语言的多后端"，而是"同一逻辑的多 shell 语言"。

## 文件结构

```
ash/auto-shell/tests/parity/
├── README.md                    # 框架说明 + 用例索引
├── cases/                       # 50 个用例
│   ├── 01_echo/
│   │   ├── desc.md              # 描述
│   │   ├── ash.ash              # ash 版
│   │   ├── bash.sh              # bash 版
│   │   ├── pwsh.ps1             # PowerShell 版（可选）
│   │   ├── fish.fish            # fish 版（可选）
│   │   ├── nu.nu                # nushell 版（可选）
│   │   └── expected.txt         # golden output（bash 生成）
│   └── ...
└── (harness 在 tests/parity.rs 集成测试文件里)
```

## 用例分类（50 个）

### A. 基础命令（10 个）
01. echo 输出 | 02. 变量赋值与引用 | 03. 命令替换 | 04. 管道 | 05. 重定向
06. 退出码 | 07. 环境变量 | 08. 多命令串联(&&) | 09. 多命令串联(||) | 10. 分组命令

### B. 字符串操作（8 个）
11. 字符串拼接 | 12. 字符串长度 | 13. 子串提取 | 14. 字符串替换
15. 大小写转换 | 16. 分割字符串 | 17. 去空白 | 18. 字符串包含

### C. 条件与循环（10 个）
19. if-else | 20. if-elif-else | 21. for 遍历列表 | 22. for 范围
23. while 循环 | 24. break | 25. continue | 26. 嵌套循环
27. 文件存在测试 | 28. 字符串比较测试

### D. 函数（5 个）
29. 函数定义与调用 | 30. 函数参数 | 31. 函数返回值 | 32. 递归 | 33. 局部变量

### E. 文件操作（8 个）
34. 创建文件 | 35. 读取文件 | 36. 追加写入 | 37. 文件存在检查
38. 文件大小 | 39. 行数统计 | 40. 文件复制 | 41. 目录创建

### F. 数据处理（5 个）
42. grep 搜索 | 43. sort 排序 | 44. uniq 去重 | 45. head/tail | 46. wc 统计

### G. 错误处理（4 个）
47. try-catch vs trap | 48. 命令失败处理 | 49. 空输入处理 | 50. 数学运算

## Harness 设计

### 执行流程
```
对每个用例:
  1. Shell::execute(ash_content) → ash stdout
  2. Command("bash").arg(bash_script) → bash stdout
  3. normalize(ash_stdout) vs normalize(bash_stdout)
  4. assert_eq!
  5. pwsh/fish/nu（存在则比较，不存在跳过）
```

### 归一化规则
- 去 ANSI 颜色码
- CRLF → LF
- 去 trailing whitespace
- 绝对路径 → `<TMPDIR>`

### 跳过策略
- shell 未安装 → 跳过（不 fail）
- 用例 desc.md 可标 `skip_shells: [nu]`

## 实施步骤

1. harness.rs + normalize.rs
2. 3 个示范用例验证框架
3. 批量 50 个用例
4. CI 集成
