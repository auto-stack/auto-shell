/**
 * useShellMock — a browser-only stand-in for useShell, used to preview the UI
 * without the Tauri backend (which is currently blocked on auto-ai-agent
 * compiling). Returns the SAME shape as useShell, so swapping is zero-effort.
 *
 * It serves canned responses for a few representative commands so we can
 * verify the visual fixes (table alignment, card borders, file coloring,
 * prompt, cwd) in `npm run dev`.
 */
import { ref, reactive, computed } from 'vue'
import type {
  Block,
  RenderedOutput,
  RenderedCell,
  ToolEntry,
  SmartCommandEntry,
  CompletionItem,
  PromptContext,
} from '@/types/shell'

// ── Canned outputs keyed by command ──────────────────────────────────────────

const lsOutput: RenderedOutput = {
  Table: {
    columns: ['name', 'type', 'size', 'modified'],
    atom_type: 'FileList',
    rows: [
      [
        { Tagged: { text: 'src', tag: { FileName: 'Dir' } } },
        { Tagged: { text: 'dir', tag: 'Dir' } },
        { Text: '4096' },
        { Text: 'Aug  4 15:30' },
      ],
      [
        { Tagged: { text: 'main.rs', tag: { FileName: 'CodeAtRs' } } },
        { Text: 'file' },
        { Text: '3421' },
        { Text: 'Aug  4 14:12' },
      ],
      [
        { Tagged: { text: 'app.at', tag: { FileName: 'CodeAtRs' } } },
        { Text: 'file' },
        { Text: '1280' },
        { Text: 'Aug  4 11:05' },
      ],
      [
        { Tagged: { text: 'Cargo.toml', tag: { FileName: 'Config' } } },
        { Text: 'file' },
        { Text: '512' },
        { Text: 'Aug  3 09:44' },
      ],
      [
        { Tagged: { text: 'run.exe', tag: { FileName: 'Executable' } } },
        { Text: 'file' },
        { Text: '1048576' },
        { Text: 'Aug  2 18:20' },
      ],
      [
        { Tagged: { text: 'README.md', tag: { FileName: 'Plain' } } },
        { Text: 'file' },
        { Text: '8192' },
        { Text: 'Aug  1 10:00' },
      ],
    ],
  },
}

const memOutput: RenderedOutput = {
  Record: {
    fields: [
      ['total', { Text: '16384 MB' }],
      ['used', { Text: '8192 MB' }],
      ['free', { Text: '8192 MB' }],
      ['usage_percent', { Text: '50%' }],
    ],
    atom_type: 'MemoryInfo',
  },
}

const helpOutput: RenderedOutput = {
  Text: 'ash — a modern shell\n\nUsage: ash [command] [args]\n\nCommands:\n  ls        list directory contents\n  cd        change directory\n  cat       print file contents\n  grep      search text\n  mem       show memory info\n  help      show this help',
}

function dispatch(cmd: string): { output: RenderedOutput; ok: boolean } {
  const c = cmd.trim().toLowerCase()
  if (c.startsWith('ls')) return { output: lsOutput, ok: true }
  if (c.startsWith('mem')) return { output: memOutput, ok: true }
  if (c === 'help' || c.startsWith('help')) return { output: helpOutput, ok: true }
  if (c.startsWith('echo ')) return { output: { Text: cmd.slice(5) }, ok: true }
  return {
    output: { Error: { message: `command not found: ${cmd}`, kind: 'NotFound' } },
    ok: false,
  }
}

// ── The mock composable (same return shape as useShell) ──────────────────────

export function useShellMock() {
  const blocks = reactive<Block[]>([])
  const cwd = ref<string>('C:\\Users\\zhaop\\projects\\ash-gui')
  const home = ref<string>('C:\\Users\\zhaop')
  const commands = ref<ToolEntry[]>([])
  const smartCommands = ref<SmartCommandEntry[]>([
    { name: 'gitstat', description: 'show git status summary' },
  ])
  const commandNames = ref<string[]>([
    'ls', 'cd', 'cat', 'grep', 'mem', 'help', 'echo', 'ps', 'find', 'smart',
  ])
  const persistedHistory = ref<string[]>(['ls', 'help', 'echo hello'])
  const gitInfo = ref<PromptContext>({
    git_branch: 'main',
    git_status: { staged: 2, unstaged: 1, untracked: 0, conflicted: 0, ahead: 0, behind: 0 },
  })
  let nextId = 0

  const history = computed(() => [
    ...persistedHistory.value,
    ...blocks.filter((b) => b.command).map((b) => b.command),
  ])

  async function runCommand(command: string) {
    const trimmed = command.trim()
    if (!trimmed) return
    const id = nextId++
    blocks.push({
      id,
      command: trimmed,
      cwd: cwd.value,
      status: { kind: 'Running' },
      output: null,
      streamedText: '',
      durationMs: null,
    })
    // Simulate async completion.
    const delay = 120 + Math.random() * 300
    setTimeout(() => {
      const block = blocks.find((b) => b.id === id)
      if (!block) return
      const { output, ok } = dispatch(trimmed)
      block.status = ok ? { kind: 'Success' } : { kind: 'Failed', message: `command failed: ${trimmed}` }
      block.output = ok ? output : null
      if (!ok) block.output = output // still show the error card
      block.streamedText = ''
      block.durationMs = Math.round(delay)
    }, delay)
  }

  /** Plan 040 M3 mock: run a SmartCommand by name. */
  async function runSmartCommand(name: string, _args: string[] = []) {
    const id = nextId++
    blocks.push({
      id,
      command: `smart ${name}`,
      cwd: cwd.value,
      status: { kind: 'Running' },
      output: null,
      streamedText: '',
      durationMs: null,
    })
    setTimeout(() => {
      const block = blocks.find((b) => b.id === id)
      if (!block) return
      block.status = { kind: 'Success' }
      block.output = { Text: `[mock] ran SmartCommand ${name}` }
      block.durationMs = 50
    }, 150)
  }

  /** Plan 040 M5 mock: cancel the running command. */
  async function cancelCommand() {
    const running = blocks.find((b) => b.status.kind === 'Running')
    if (running && running.status.kind === 'Running') {
      running.status = { kind: 'Cancelled' }
    }
  }

  async function openPath(_path: string) {
    // no-op in browser preview
  }

  /** Plan 041 M7 mock: return command-name completions for browser preview. */
  async function complete(line: string, _cursor: number): Promise<CompletionItem[]> {
    const first = line.split(/\s+/)[0] ?? ''
    return commandNames.value
      .filter((n) => n.startsWith(first))
      .slice(0, 8)
      .map((n) => ({ replacement: n, display: n, description: null, kind: 'command' }))
  }

  return {
    blocks,
    cwd,
    home,
    commands,
    smartCommands,
    commandNames,
    history,
    gitInfo,
    runCommand,
    runSmartCommand,
    cancelCommand,
    complete,
    openPath,
  }
}
