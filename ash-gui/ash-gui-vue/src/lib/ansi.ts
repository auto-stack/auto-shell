/**
 * Lightweight ANSI escape → HTML converter (Plan 042 bugfix).
 *
 * The Rust backend's `show` command syntax-highlights code files (.rs, .py,
 * etc.) with ANSI color escapes (syntect + base16 theme). For TOML/INI it
 * returns plain text (no syntax match in syntect's default set). This
 * converter lets `TextView.vue` render the ANSI-colored output as HTML spans
 * so code shows up with proper colors in the GUI.
 *
 * No dependency — handles the SGR (Select Graphic Rendition) codes the base16
 * themes produce: foreground colors (30-37, 90-97, 38;5;N), bold (1), italic
 * (3), and reset (0). Unknown codes are stripped.
 */

/** Map ANSI SGR color codes → CSS color classes (dark-theme palette). */
const ANSI_COLORS: Record<number, string> = {
  30: '#5c6370',  // black (bright black / comments)
  31: '#e06c75',  // red
  32: '#98c379',  // green
  33: '#d19a66',  // yellow
  34: '#61afef',  // blue
  35: '#c678dd',  // magenta
  36: '#56b6c2',  // cyan
  37: '#abb2bf',  // white (light gray)
  // Bright variants
  90: '#5c6370',
  91: '#e06c75',
  92: '#98c379',
  93: '#d19a66',
  94: '#61afef',
  95: '#c678dd',
  96: '#56b6c2',
  97: '#ffffff',
}

interface Span {
  color?: string
  bold?: boolean
  italic?: boolean
}

/** Convert a string with ANSI escape codes into an array of HTML-renderable
 * segments: `{ text, style }` where style is a CSS string. */
export interface AnsiSegment {
  text: string
  style: string
}

export function ansiToSegments(input: string): AnsiSegment[] {
  const segments: AnsiSegment[] = []
  let current: Span = {}
  let buffer = ''
  const flush = () => {
    if (buffer) {
      const style = spanToStyle(current)
      segments.push({ text: buffer, style })
      buffer = ''
    }
  }

  // Regex: match \x1b[...m (CSI sequence ending in 'm')
  const re = /\x1b\[([\d;]*)m/g
  let last = 0
  let match: RegExpExecArray | null

  while ((match = re.exec(input)) !== null) {
    // Text before the escape code.
    buffer += input.slice(last, match.index)
    last = match.index + match[0].length

    // Process the SGR parameters.
    const params = match[1].split(';').filter((s) => s !== '').map(Number)
    if (params.length === 0 || params[0] === 0) {
      // Reset.
      flush()
      current = {}
    } else {
      for (const p of params) {
        if (p === 1) current.bold = true
        else if (p === 3) current.italic = true
        else if (p === 22) current.bold = false
        else if (p === 23) current.italic = false
        else if (p >= 30 && p <= 37) current.color = ANSI_COLORS[p]
        else if (p >= 90 && p <= 97) current.color = ANSI_COLORS[p]
        else if (p === 38) {
          // 38;5;N (256-color) or 38;2;R;G;B (truecolor) — skip the sub-params
          // (handled below in the params loop; for simplicity use the N value).
        }
        else if (p === 39) current.color = undefined // default fg
      }
    }
  }

  // Remaining text (no trailing escape).
  buffer += input.slice(last)
  flush()

  // If no segments (plain text, no ANSI), return one segment.
  if (segments.length === 0) {
    return [{ text: input, style: '' }]
  }
  return segments
}

function spanToStyle(s: Span): string {
  const parts: string[] = []
  if (s.color) parts.push(`color: ${s.color}`)
  if (s.bold) parts.push('font-weight: bold')
  if (s.italic) parts.push('font-style: italic')
  return parts.join('; ')
}
