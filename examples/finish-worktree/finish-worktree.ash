// git.finish-worktree body — "few deterministic steps" SmartCommand (Plan 029).
//
// Usage:  ash smart run git.finish-worktree "fix-empty-input"
//
// Demonstrates the SmartCommand body pattern: AutoLang control flow + shell
// lines (`>`) to drive git. The commit message is passed as $1 and used in a
// `>` line (where shell-level $1 expansion applies).
//
// NOTE on $1: it is a SHELL-LEVEL expansion (Plan 034), so it works inside `>`
// lines and system("...") strings, but not as a bare AutoLang term. Keep $1
// usage in shell lines. For multi-word messages, quote appropriately in the
// shell line (the example uses a single-token message for clarity).

print("> staging all changes...")
> git add -A

print("> committing...")
> git commit -m $1

print("> pushing...")
> git push

print("done.")
