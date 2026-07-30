# git.finish-worktree — a SmartCommand example (Plan 029)

A "few deterministic steps" SmartCommand: stage → commit → push, with the
commit message supplied as `$1`.

## Files

- `finish-worktree.at` — the SmartCommand declaration (name, description, args, body)
- `finish-worktree.ash` — the body script (AutoLang + `system()` git calls)

## Install

Copy both files into your smart-command search path:

```bash
# project-local (this project only)
mkdir -p ./smart && cp finish-worktree.* ./smart/

# or user-global (all projects)
mkdir -p ~/.config/ash/smart && cp finish-worktree.* ~/.config/ash/smart/
```

## Use

```bash
ash smart list                       # confirm it's discovered
ash smart run git.finish-worktree "fix: handle empty input"
```

## How it works

The body runs as a normal `.ash` script via `Shell::execute_script_content`:

- `$1` expands to the first positional arg (the commit message) — Plan 034's
  script-arg mechanism.
- `system("git ...")` calls back into the shell to run git (Plan 011 host bridge).
- AutoLang `if`/`print`/`exit` provide the control flow.

v1 has no AI judgment step (that needs the SmartCommandRole + a running Ollama
daemon). This example shows the deterministic-body half of SmartCommand.
