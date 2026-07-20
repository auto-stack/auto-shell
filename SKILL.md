---
name: ash-shell
description: Use ash (AutoShell) commands to read, search, transform, and analyze files and data with structured pipelines and syntax highlighting. Use when the agent needs to view file contents, search codebases, work with structured data (JSON/CSV/TOML), or run POSIX-compatible shell commands. Ash extends bash-compatible commands with zero-copy structured data pipelines, extension-aware file viewing, and a JSON output mode for programmatic consumption.
---

# ash — AutoShell

A modern shell where **commands exchange structured data** (not just text), `show` renders files with syntax highlighting, and `--json` mode returns machine-readable output for agent consumption.

## When to use ash

- **Viewing a file** with syntax highlighting: `show file.rs`
- **Searching a codebase**: `grep -rn "pattern" src/`
- **Working with structured data**: `show data.json | where age > 18 | select name | to_csv`
- **Listing/inspecting files**: `ls -l`, `find . -n "*.rs"`, `wc -l file.txt`
- **Any standard shell task**: most POSIX commands (cat, grep, sort, cut, tr, cp, mv, ...) work as you'd expect

## Bash compatibility

ash implements **80+ commands** with POSIX/GNU-compatible flags. For standard shell tasks, treat ash commands like their bash counterparts — `ls -la`, `grep -rn pattern`, `cp -r src dst`, `mkdir -p dir`, `head -n 20`, `sort -u`, `uniq -c`, `wc -l`, `find . -name "*.py"` all work as expected. Shell features like `|` pipes, `&&`/`||` chains, `>`/`>>` redirects, `$?` exit codes, `cd`, `pwd`, `echo`, and job control (`&`, `fg`, `bg`) are all supported.

**What follows is ash-specific behavior that goes beyond bash.** Learn these patterns — they are where ash shines.

---

## ash-specific command: `show` (the smart file reader)

`show` is ash's primary file viewer. It auto-detects the file type:

| Extension | Behavior |
|-----------|----------|
| `.json` | Parsed into a structured Value (renders as a table or record) |
| `.csv` | Parsed into an Array of objects (renders as a table) |
| `.toml` `.rs` `.py` `.js` `.go` `.c` `.md` ... (60+ code/config types) | **Syntax-highlighted** with ANSI colors |
| anything else | Raw text (same as `cat`) |

```
show package.json                  → renders as a table
show src/main.rs                   → syntax-highlighted Rust
show config.toml                   → syntax-highlighted TOML
show data.csv                      → renders as a table
show README.md                     → syntax-highlighted Markdown
```

Flags:
- `--as json|csv|text` — force a format (overrides extension detection)
- `-p` / `--pager` — interactive pager with lazy syntax highlighting (for large code files; navigates with j/k/Space/q like `less`)

> **`show` vs `cat`**: use `show` for code/config files (gets highlighting + structured parsing). Use `cat` for raw text or when you need `-n` line numbers on plain text. **`open` is NOT a reader — it launches the OS default GUI application.**

### Streaming behavior

`show` streams highlighted output line-by-line, so the first line appears immediately even on large files. When piped (`show file.rs | grep "fn "`), it re-spawns as a subprocess streaming through an OS pipe — the consumer sees output immediately.

---

## Core concept: structured pipelines

This is ash's superpower. Unlike bash where `|` passes raw text, ash pipelines carry **structured data**. Commands like `ls`, `find`, `show data.json`, and `grep` emit structured records (arrays of objects), and downstream commands can filter, select, and transform by **field name** — no `awk`/`cut` text slicing needed.

```bash
# List .rs files, keep only the name field
find . -n "*.rs" | select name

# Show a JSON file, filter rows, extract columns, export as CSV
show users.json | where age ">" 18 | select name email | to_csv

# grep output is structured — pipe into field selection
grep -rn "TODO" src/ | select file line_number

# Count files by type
ls | select type | sort | uniq -c
```

### Structured-data commands (the killer feature)

These operate on arrays of objects in the pipeline:

| Command | What it does | Example |
|---------|-------------|---------|
| `where FIELD OP VALUE` | Filter records by condition | `where age ">" 18` |
| `select FIELD...` | Keep only specified fields | `select name email` |
| `get FIELD...` | Extract field values | `get name` |
| `each FIELD` | Extract one field from each record | `each filename` |
| `insert FIELD VALUE` | Add a field to each record | `insert status "new"` |
| `update FIELD VALUE` | Update a field in each record | `update count 0` |

Operators for `where`: `==` `!=` `<` `>` `<=` `>=`

### Format conversion (round-trip)

| Command | Direction | Notes |
|---------|-----------|-------|
| `from_json` | text → Value | parse JSON |
| `to_json` | Value → text | `--pretty` for indented, `--compact` (default) |
| `from_csv` / `to_csv` | text ↔ table | `-d` delimiter, `--no-header` |
| `from_toml` / `to_toml` | text ↔ Value | |
| `from_yaml` / `to_yaml` | text ↔ Value | |
| `from_xml` / `to_xml` | Value → text | `-r` root element, `-i` indent |

Pattern: `show file.json | <transform> | to_csv > output.csv`

---

## Agent integration: `--json` mode

The `--json` flag makes ash emit **clean JSON** on stdout (diagnostics go to stderr), so agents can parse output programmatically:

```bash
# Returns a JSON array of file objects
ash -c "ls src/" --json

# Returns a JSON array of grep results
ash -c "grep -rn 'fn ' src/" --json

# Pipeline result serialized as JSON
ash -c "show data.json | where score '>' 80 | select name" --json
```

`--json` may appear anywhere on the command line. With `-c` it outputs one JSON value; with `-s` (stdin script) or a script file it emits NDJSON (one JSON value per command).

### Other invocation modes

| Mode | Usage |
|------|-------|
| `ash -c "command"` | Run a single command string |
| `ash -s` | Read and execute a script from stdin |
| `ash script.at` | Execute an AutoLang script file |
| `ash` (no args) | Interactive REPL |

### Security flags (for sandboxing agents)

| Flag | Effect |
|------|--------|
| `--sandbox DIR` | Confine all file operations to DIR |
| `--read-only` | Block all writes |
| `--no-network` | Block HTTP commands |
| `--no-exec` | Block external process spawning |
| `--allow CMD` / `--deny CMD` | Whitelist/blacklist specific commands |
| `--dry-run` | Show what would run without executing |

Example: `ash --sandbox ./project --read-only -c "find . -n '*.rs' | wc -l"`

---

## String & text operations (pipeline filters)

These take pipeline text input and emit transformed text:

| Command | Example |
|---------|---------|
| `str-replace PATTERN REPLACEMENT` | `--first` for single replace |
| `str-trim` | `-l` left only, `-r` right only |
| `str-case OP` | `upper`, `lower`, `capitalize`, `snake`, `kebab`, `camel`, `title` |
| `str-split SEP` | → Array |
| `str-join SEP` | Array → string |
| `str-contains PATTERN` | → boolean |
| `str-length` | → integer |
| `url-encode TEXT` | `--decode` for decoding |

Standard text tools — `sort`, `uniq`, `cut`, `tr`, `diff`, `paste`, `rev`, `column`, `fmt`, `tee` — all work with bash-compatible flags. Notable: `sort -w FIELD` sorts structured records by a field; `sort -n` does numeric sort.

---

## Math commands

Operate on numeric arrays/values in the pipeline:

| Command | Example |
|---------|---------|
| `math-sum` | Sum all numeric values (Int or Float result) |
| `math-avg` | Average |
| `math-min` / `math-max` | Min/max (optional `field` arg for arrays of objects) |
| `math-round` | Round; `--floor`, `--ceil`, `--abs`; optional precision |

Pattern: `show prices.json | each price | math-sum`

---

## System commands

| Command | Notes |
|---------|-------|
| `ls` | Structured output (array of file objects) — pipe into `select`/`where`/`grep` |
| `ps` | `-l` long format, `-a` include system processes |
| `sys` | `sys cpu`, `sys mem`, `sys disks` |
| `date` | `--format` (strftime), `-u` UTC, `--unix` timestamp |
| `which CMD` | `--all` for all matches |
| `realpath PATH` | Resolve absolute path |
| `stat FILE` | File metadata |
| `du` | `-s` summarize, `-h` human-readable, `-d N` max depth |

---

## Full command reference

**File viewing**: `show` `cat` `less` `more` `head` `tail` `open`(GUI launcher)
**Searching**: `grep` `find` `glob`
**File info**: `ls` `stat` `du` `wc` `file` `realpath` `which`
**Filesystem**: `cp` `mv` `rm` `mkdir` `touch` `ln` `cd` `pwd`
**Text processing**: `sort` `uniq` `cut` `tr` `paste` `split` `column` `fmt` `diff` `rev` `tee`
**String ops**: `str-replace` `str-contains` `str-split` `str-join` `str-trim` `str-case` `str-length` `url-encode`
**Data formats**: `from_json` `to_json` `from_csv` `to_csv` `from_toml` `to_toml` `from_yaml` `to_yaml` `from_xml` `to_xml`
**Record ops**: `select` `get` `where` `each` `insert` `update`
**Math**: `math-sum` `math-avg` `math-min` `math-max` `math-round`
**System**: `ps` `sys` `date` `sleep`
**HTTP**: `http get` `http post` `http put` `http delete` `http head`
**Meta**: `echo` `help` `version` `build` `run`

---

## Important notes

1. **`open` ≠ file reader.** `open file.pdf` launches the OS default app (like double-clicking). To read a file, use `show`.
2. **`less`/`more` are interactive TUIs** — they take over the terminal. In scripts or non-interactive contexts, use `head`/`tail`/`cat` instead. For viewing large code files interactively, use `show -p file.rs`.
3. **`--json` is the agent bridge.** When you need to parse ash output programmatically, always add `--json`. Without it, table rendering adds borders and ANSI colors that break JSON parsing.
4. **`show` auto-detects format** by extension. If auto-detection fails or you need a specific format, use `--as json|csv|text`.
5. **Structured pipelines are preferred over text slicing.** Instead of `ls | cut -d' ' -f1`, use `ls | select name`. Instead of `awk`, use `where`/`select`.
