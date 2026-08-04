/**
 * Path display helpers.
 *
 * Mirrors the TUI's directory module convention (ash-tui prompt/modules/directory.rs):
 *   - backslashes normalized to forward slashes,
 *   - home directory abbreviated to `~`,
 *   - no truncation.
 */

/** Normalize a Windows path to forward slashes. */
export function normalizePath(p: string): string {
  return p.replace(/\\/g, '/')
}

/**
 * Abbreviate `path` by replacing the home prefix with `~`.
 * If `home` is empty or the path isn't under home, returns the normalized path.
 */
export function abbrevPath(path: string, home: string): string {
  const p = normalizePath(path)
  const h = normalizePath(home).replace(/\/+$/, '')
  if (h && p === h) return '~'
  if (h && p.startsWith(h + '/')) return `~${p.slice(h.length)}`
  return p
}
