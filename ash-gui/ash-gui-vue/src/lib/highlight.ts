/**
 * Plan 041 M4: a lightweight shell syntax tokenizer for the GUI input.
 *
 * Mirrors the TUI `AshHighlighter` (ash-tui/src/term/highlight.rs) coloring
 * scheme — command (builtin bold vs external), string, flag, operator,
 * variable, redirect, comment. Pure function returning colored spans the
 * PromptBar renders as a `<pre>` overlay behind a transparent textarea.
 *
 * Kept dependency-free (no prismjs/shiki) — the shell grammar is simple enough
 * that a small state machine suffices, and it stays in sync with AshHighlighter.
 */

/** One colored span of the input line. */
export interface HighlightSpan {
  text: string
  cls: string
}

/** Semantic token classes → CSS color classes (defined in PromptBar / tailwind). */
const CLS = {
  cmdBuiltin: 'text-emerald-400 font-semibold',
  cmdExternal: 'text-sky-300',
  string: 'text-amber-300',
  flag: 'text-purple-300',
  operator: 'text-pink-400 font-semibold',
  variable: 'text-red-400',
  redirect: 'text-muted-foreground',
  comment: 'text-muted-foreground/50 italic',
  plain: 'text-foreground',
}

/** Known builtin/registered command names (mirrors AshHighlighter::new). */
const BUILTINS = new Set([
  'cd', 'pwd', 'echo', 'help', 'clear', 'exit', 'quit', 'q',
  'alias', 'unalias', 'source', '.', 'set', 'export', 'unset', 'use',
  'jobs', 'fg', 'bg', 'suspend', 'history',
  'ls', 'l', 'mkdir', 'rm', 'cp', 'mv', 'touch', 'find', 'glob',
  'stat', 'du', 'file', 'tee', 'ln', 'cat', 'head', 'tail', 'sort',
  'uniq', 'wc', 'grep', 'cut', 'paste', 'tr', 'split', 'rev', 'column',
  'fmt', 'diff', 'from_json', 'to_json', 'from_csv', 'to_csv', 'from_toml',
  'to_toml', 'from_yaml', 'to_yaml', 'from_xml', 'to_xml', 'str_replace',
  'str_contains', 'str_split', 'str_join', 'str_trim', 'str_case',
  'str_length', 'math_sum', 'math_avg', 'math_min', 'math_max', 'math_round',
  'select', 'get', 'where', 'update', 'insert', 'each', 'build', 'run',
  'http_get', 'http_post', 'http_put', 'http_delete', 'http_head', 'url_encode',
  'date', 'sleep', 'which', 'version', 'ps', 'sys', 'up', 'u', 'b',
  'less', 'more', 'color', 'completions', 'config', 'bind', 'abbr', 'hook',
  'def', 'pushd', 'popd', 'dirs', 'path', 'env', 'env.path',
])

/** Tokenize a shell input line into colored spans. */
export function tokenize(line: string): HighlightSpan[] {
  const spans: HighlightSpan[] = []
  let i = 0
  let firstWord = true // command-name position
  const n = line.length

  const push = (text: string, cls: string) => {
    if (text) spans.push({ text, cls })
  }

  while (i < n) {
    const c = line[i]

    // Comment: # to end of line (outside quotes).
    if (c === '#' && (i === 0 || /\s/.test(line[i - 1]))) {
      push(line.slice(i), CLS.comment)
      break
    }

    // Whitespace — emit as plain (preserves layout).
    if (/\s/.test(c)) {
      let j = i
      while (j < n && /\s/.test(line[j])) j++
      push(line.slice(i, j), CLS.plain)
      i = j
      firstWord = false
      continue
    }

    // String literal: "..." or '...'
    if (c === '"' || c === "'") {
      const quote = c
      let j = i + 1
      while (j < n && line[j] !== quote) {
        if (quote === '"' && line[j] === '\\' && j + 1 < n) j++ // skip escaped
        j++
      }
      j = Math.min(j + 1, n) // include closing quote (or to EOL if unclosed)
      push(line.slice(i, j), CLS.string)
      i = j
      firstWord = false
      continue
    }

    // Variable: $VAR or ${VAR}
    if (c === '$') {
      let j = i + 1
      if (line[j] === '{') {
        while (j < n && line[j] !== '}') j++
        j = Math.min(j + 1, n)
      } else {
        while (j < n && /[A-Za-z0-9_]/.test(line[j])) j++
      }
      push(line.slice(i, j), CLS.variable)
      i = j
      firstWord = false
      continue
    }

    // Operators: | && || ;
    if (c === '|' || (c === '&' && line[i + 1] === '&') || c === ';') {
      let len = 1
      if (c === '|' && line[i + 1] === '|') len = 2
      if (c === '&') len = 2
      push(line.slice(i, i + len), CLS.operator)
      i += len
      firstWord = true // next word is a command again (after | / &&)
      continue
    }

    // Redirects: > >> < 2>
    if (c === '>' || c === '<') {
      let len = c === '>' && line[i + 1] === '>' ? 2 : 1
      // 2>, 2>> etc — include leading digit if present
      if (i > 0 && /[0-9]/.test(line[i - 1]) && c === '>') {
        // the digit was emitted as part of a previous token; just emit the >
      }
      push(line.slice(i, i + len), CLS.redirect)
      i += len
      firstWord = false
      continue
    }

    // Word: read until whitespace / operator / quote / $ / #
    let j = i
    while (
      j < n &&
      !/\s/.test(line[j]) &&
      line[j] !== '"' &&
      line[j] !== "'" &&
      line[j] !== '$' &&
      line[j] !== '|' &&
      line[j] !== '&' &&
      line[j] !== ';' &&
      line[j] !== '#' &&
      line[j] !== '>' &&
      line[j] !== '<'
    ) {
      j++
    }
    const word = line.slice(i, j)

    if (firstWord) {
      // Command position: builtin vs external.
      push(word, BUILTINS.has(word) ? CLS.cmdBuiltin : CLS.cmdExternal)
      firstWord = false
    } else if (word.startsWith('-') && word.length > 1) {
      // Flag: -x or --xxx
      push(word, CLS.flag)
    } else {
      push(word, CLS.plain)
    }
    i = j
  }

  return spans
}
