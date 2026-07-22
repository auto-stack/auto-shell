# buildtest —— 构建后测试,失败即停

## 运行

```bash
# cargo build && cargo test
ash examples/buildtest/buildtest.ash

# release 构建
ash examples/buildtest/buildtest.ash release
```

## ash 版本亮点

- build/test 封装成函数,`system_status()` 取退出码判断成败
- build 失败立刻 `exit`,不浪费跑测试的时间
- 退出码透传给调用方,便于在 CI 里串联

## bash 对照

```bash
# bash:&& 链 + $? 检查,退出码传递要手动写
cargo build || exit 1
cargo test
```

bash 的问题:错误处理靠 `||`/`$?`,可读性差,无法在中间插入"build 失败原因"等定制逻辑。ash 用函数 + 显式分支。

## ash 脚本

见 [buildtest.ash](buildtest.ash)

## 依赖

- ash v0.5+(system_status 取退出码)
