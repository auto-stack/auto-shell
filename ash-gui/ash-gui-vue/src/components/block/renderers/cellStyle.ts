/**
 * CellTag → styling helpers.
 *
 * Mirrors the color conventions established by the iced renderer
 * (ash-gui-bin/src/renderer.rs:324-337 `tag_color`) and the TUI, so the Vue
 * and iced frontends stay visually consistent:
 *   dir → blue, .rs/.at → green, executable → cyan, config → amber, plain → default.
 */
import type { CellTag, FileNameKind, RenderedCell } from '@/types/shell'

/** Extract the text value from a RenderedCell. */
export function cellText(cell: RenderedCell): string {
  return 'Text' in cell ? cell.Text : cell.Tagged.text
}

/** Extract the tag (Plain if the cell is untagged). */
export function cellTag(cell: RenderedCell): CellTag {
  return 'Tagged' in cell ? cell.Tagged.tag : 'Plain'
}

/** Is this cell a clickable path (file / dir)? */
export function isClickable(tag: CellTag): boolean {
  if (tag === 'Dir') return true
  if (typeof tag === 'object' && 'FileName' in tag) return true
  return false
}

/**
 * Tailwind text-color class for a tag. Colors chosen to match the iced frontend:
 *   Dir / FileName(Dir)        → sky-400     (blue)
 *   FileName(CodeAtRs)         → emerald-400 (green)
 *   FileName(Executable)       → cyan-300    (cyan)
 *   FileName(Config)           → amber-300   (gold)
 *   Permission                 → muted       (dim gray)
 *   Plain                      → foreground  (default)
 */
export function tagTextClass(tag: CellTag): string {
  if (tag === 'Dir') return 'text-sky-400'
  if (tag === 'Permission') return 'text-muted-foreground'
  if (tag === 'Plain') return 'text-foreground'
  // FileName(kind)
  const kind: FileNameKind = (tag as { FileName: FileNameKind }).FileName
  switch (kind) {
    case 'Dir': return 'text-sky-400 hover:underline cursor-pointer'
    case 'CodeAtRs': return 'text-emerald-400 hover:underline cursor-pointer'
    case 'Executable': return 'text-cyan-300 hover:underline cursor-pointer'
    case 'Config': return 'text-amber-300 hover:underline cursor-pointer'
    case 'Plain':
    default: return 'text-foreground hover:underline cursor-pointer'
  }
}
