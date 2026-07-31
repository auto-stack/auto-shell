# deploy-pack (example plugin)

A Plan 033 plugin that contributes **a SmartCommand + a helper function**. It
shows the per-command subdirectory smart layout (`smart/<cmd>/command.at`, see
`designs/033-plugin-ecosystem.md` §3.1) plus a `functions.ash`.

```bash
ash plugin install --local ./examples/plugins/deploy-pack
ash plugin show deploy-pack
# restart ash, then:
ash smart list                 # shows deploy.run
ash smart run deploy.run prod  # runs smart/deploy/deploy.ash with $1=prod
```

What it contributes:
- `smart/deploy/command.at` + `smart/deploy/deploy.ash` — the `deploy.run`
  SmartCommand (uses the documented `smart/<cmd>/command.at` layout)
- `functions.ash` — a `deploy_msg(env)` AutoLang function available at the prompt

Capabilities declared: `reads_fs`, `spawns_process` (shown as a warning on load;
v1 does not enforce confirmation).

Remove it with `ash plugin remove deploy-pack`.
