# switch-env —— 切换 .env 文件并校验

## 运行

```bash
# 切到生产环境(.env.production → .env)
ash examples/switch-env/switch-env.ash production

# 切到 staging
ash examples/switch-env/switch-env.ash staging
```

## ash 版本亮点

- 把 `.env.<环境>` 复制成 `.env`,自动备份旧文件到 `.env.backup`
- 校验必填变量(`DATABASE_URL`/`API_KEY` 等),缺项则拒绝切换
- `export("APP_ENV", env)` 让环境名进入当前 shell
- try/catch 包裹,文件缺失等错误不崩溃

## bash 对照

```bash
# bash:cp 一行,无校验、无备份、错误静默
cp .env.production .env
```

bash 的问题:覆盖了现有 .env 无备份、不检查必填项、切完不导出环境名。ash 全包。

## ash 脚本

见 [switch-env.ash](switch-env.ash)

## 依赖

- ash v0.5+
