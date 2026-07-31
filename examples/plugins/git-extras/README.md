# git-extras (example plugin)

A minimal Plan 033 plugin that contributes **one completion spec** for `git`.

It demonstrates the smallest useful plugin: a `plugin.at` manifest plus a single
`completions/git.at` spec. Install it to see the plugin-completion tier in
action (the 4th completion tier, highest precedence):

```bash
ash plugin install --local ./examples/plugins/git-extras
ash plugin show git-extras
# restart ash, then type `git stash` / `git blame` and press Tab
```

What it contributes:
- `completions/git.at` — adds `git stash` / `git blame` subcommands + a `-C` flag

Remove it with `ash plugin remove git-extras`.
