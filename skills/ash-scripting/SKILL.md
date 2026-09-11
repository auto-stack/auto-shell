---
name: ash-scripting
description: Write and run AutoLang scripts (.ash) with ash — AutoShell's script mode. Use when a task needs multi-step logic (branching, loops, aggregation, meaningful exit codes) that a single run_command one-liner can't express, when asked to write an .ash script, or before passing content to the run_ash_script tool.
---

# ash scripting (AutoLang script mode)

`.ash` files are **AutoLang in script mode**, executed by ash. They add what one-liners
can't do: variables, branching, loops, functions, and exit codes. Everything below is
probe-verified against **ash v0.1.0** (2026-09-11 build); where the `examples/` folder or
older docs disagree, this skill wins.

## Skeleton (always this shape)

```auto
// comments with //
fn main() {
    ... statements ...
    exit(0)          // ALWAYS end with an explicit exit(code)
}
main()               // main() is NOT auto-called — invoke it at the end
```

**Always `exit(code)`.** Success is `exit(0)`; every failure path gets its own non-zero
code. ash's implicit exit codes are unreliable: undefined functions/variables exit 1,
but some VM runtime errors (e.g. an `IndexError` from out-of-range indexing) print an
error and still exit 0. An explicit `exit(code)` is the only dependable failure signal.
When run via the `run_ash_script` tool, the exit code is the success/failure signal
surfaced back to the agent (`[exit: N]`).

## Language quick reference (verified)

```auto
var x = "text"             // variables; assignment without `var` rebinds
var i = 0
i = i + 1                  // integer arithmetic
var s = "a" + "b"          // string concatenation with + (keep both sides strings)

if cond { ... }                        // no parentheses around the condition
} else if other { ... }
} else { ... }

for col in header { ... }              // iterate an array
for (key, value) in counts { ... }     // iterate map pairs

"a b".trim()               // string methods: .trim() .len() .split(" ") .lines()
parts[0]                   // indexing
print("text" + x)          // stdout
exit(3)                    // stop with exit code 3
```

Numbers in text: keep counts as strings (`var n = "5"`) and concatenate — the official
examples do this; avoid mixing int operands into string `+`.

## Reading script arguments

Positional args come through the shell bridge, one `system()` call each:

```auto
var file = system("echo $1").trim()    // $1, $2, ... ; empty string if absent
```

**Known bug:** `system("echo $@")` returns the literal `"$@"` when no args were passed
(auto-shell Plan 034, Bug 2). Treat that exact string as "no args":

```auto
var args = system("echo $@").trim()
if args == "$@" { args = "" }
```

## Running commands from a script

```auto
var out = system("ls | where type == file | to_json")   // returns stdout as a string
> echo hello                                             // `> cmd` runs a command line
```

- Pipelines (`|`) only work **inside `system("...")`** — the bare `> $cmd` form does not
  support pipes.
- Unknown/delegated command names go to the OS shell. On Windows that's PowerShell: a
  `Missing name after filter keyword`-style parse error or a `sort: ... (os error 2)`
  means ash passed the name through (e.g. `filter` is **not** an ash builtin in v0.1.0 —
  use `where`; bare `sort size` reaches the system `sort.exe` — see the dot-form rule).
- `ash agent ...` subcommands (describe-tools etc.) are **not implemented** in v0.1.0 —
  don't script against them.

## Structured pipeline cheat sheet (v0.1.0 verified)

`ls` emits records with fields `name` `type` `size` `modified`. Pipelines carry those
records; transform by field name instead of text-slicing.

```bash
ls | where type == file | sort .size descending | select name size
cat data.json | from_json | get users | to_csv
echo "[1,2,3]" | from_json | math-sum
ash -c "ls | where type == file | select name" --json   # debug any pipeline first
```

| Op | Syntax | Notes |
|----|--------|-------|
| `where` | `where FIELD OP VALUE` | **Bare field name — NO dot.** `where size > 10` works; `where .size > 10` silently returns NOTHING. Ops: `==` `!=` `<` `>` `<=` `>=` |
| `sort` | `sort .FIELD [descending]` | **Dot form required.** Bare `sort size` is NOT the builtin — it delegates to the system sort and fails with an os-error |
| `select` / `get` | `select a b` / `get field` | keep fields per record / extract a field from an object |
| `insert` / `update` | `update count 0` | add/set a field on each record |
| formats | `from_json` `to_json` `from_csv` `to_csv` `from_toml` `to_toml` `from_yaml` `to_yaml` `from_xml` `to_xml` | round-trip conversions |
| math | `math-sum` `math-avg` `math-min` `math-max` `math-round` | numeric arrays only: `... \| each amount \| math-sum` |
| strings | `str-replace` `str-trim` `str-case` `str-split` `str-join` `str-contains` `str-length` `url-encode` | |
| `show` | `show file.json` (standalone) | viewer that parses by extension; **when piped it degrades to text** — for structured access use `cat f.json \| from_json` instead |

Run `ash -c "help"` for the full 80-command list.

**Value types matter (silent-failure trap):** `from_csv` values arrive as **strings**
(`"150"`), so numeric comparisons and math on them silently miss (`where amount > 100`
returns `[]`, `math-sum` returns 0). `from_json` keeps real numbers — prefer JSON as the
interchange format, or do numeric logic in AutoLang code.

**`head` takes text lines, not records:** `cat f.txt | head -n 2` is fine, but
`... | select name | head -n 2` errors with "no file argument and no pipeline input".
Cap record sets before rendering, or slice in AutoLang.

## Script-mode pitfalls

1. **`&&`/`||` do not short-circuit.** `parts.len() > 1 && parts[1] == x` still
   evaluates `parts[1]` when there is only one part — that raises an IndexError which
   exits 0 (silent failure). Guard with nested `if`s instead of `&&` chains.
2. **Empty result ≠ error.** A bad `where` field or a type-mismatched comparison yields
   `[]` with exit 0. Check `out.trim().len() == 0` and `exit` non-zero yourself.
3. **where/sort dot asymmetry:** `where size` (bare) vs `sort .size` (dot). Getting
   either wrong is silent (`[]`) or delegates to the OS (`os error 2`).
4. **ash's builtins differ from GNU.** e.g. `find -n "pat"`, not `-name`. See
   `docs/bash-to-ash.md` in the auto-shell repo before translating bash muscle memory.
5. **No heredocs.** Build files with AutoLang code or `echo`/`>`.
6. **`open` is a GUI launcher**, and `less`/`more` are interactive TUIs — never in
   scripts; use `show`/`head`/`tail`/`cat`.
7. **Write outside the sandbox is refused.** When run via agent tooling the script gets
   `--sandbox <cwd>`; writes outside the working directory come back as
   `[exec: ash script (denied)]` PAUSED notices, not partial writes.

## Tool integration (auto-ai-cli)

- Prefer the `run_ash_script` tool: pass `content` (staged to a temp file, cleaned up) or
  an existing `path`, plus `args` (positional) and `timeout_ms`. It exists only when ash
  was probed successfully and **never falls back** to the system shell.
- One-liners go through `run_command` instead (ash track, `[exec: ash]` annotation);
  keep scripts for anything with branching, loops, or exit-code semantics.
- After writing a script, smoke-test it: `ash script.ash` — check both the output and
  `echo $?` before declaring it done.

## Worked example

```auto
// biggest.ash — list files in a directory, biggest first
// usage: ash biggest.ash [dir]     exit: 0 ok, 1 nothing found
fn main() {
    var dir = system("echo $1").trim()
    if dir.len() == 0 { dir = "." }

    var rows = system("ls " + dir + " | where type == file | sort .size descending | select name size")
    if rows.trim().len() == 0 {
        print("no files under " + dir)
        exit(1)
    }
    print(rows.trim())
    exit(0)
}
main()
```
