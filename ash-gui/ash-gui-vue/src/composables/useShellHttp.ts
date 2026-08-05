/**
 * useShellHttp — the browser-version backend (Plan 042 M4).
 *
 * Connects to the `ash-server` HTTP backend (axum on localhost:3000) via:
 *   - `fetch` for request-response endpoints (command_list, complete, etc.)
 *   - `EventSource` for the `/api/stream` SSE channel (command-output/result)
 *
 * Returns the SAME shape as `useShellTauri` (the Tauri version), so the
 * frontend components don't know or care which transport is active.
 */
import { ref, reactive, computed } from 'vue'
import type {
  Block,
  CommandResultPayload,
  CommandOutputPayload,
  CommandListResult,
  CompletionItem,
  PromptContext,
  ToolEntry,
  SmartCommandEntry,
} from '@/types/shell'

export function useShellHttp() {
  const blocks = reactive<Block[]>([])
  const cwd = ref<string>('')
  const home = ref<string>('')
  const commands = ref<ToolEntry[]>([])
  const smartCommands = ref<SmartCommandEntry[]>([])
  const commandNames = ref<string[]>([])
  const persistedHistory = ref<string[]>([])
  const gitInfo = ref<PromptContext>({ git_branch: null, git_status: null })
  let nextId = 0
  let eventSource: EventSource | null = null

  const history = computed(() => [
    ...persistedHistory.value,
    ...blocks.filter((b) => b.command).map((b) => b.command),
  ])

  /** Submit a command (non-blocking); a Running block appears immediately. */
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
    await fetch('/api/run_command', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ block_id: id, cmd: trimmed }),
    })
  }

  /** Plan 040 M3: run a SmartCommand by name. */
  async function runSmartCommand(name: string, args: string[] = []) {
    const id = nextId++
    const display = args.length ? `smart ${name} ${args.join(' ')}` : `smart ${name}`
    blocks.push({
      id,
      command: display,
      cwd: cwd.value,
      status: { kind: 'Running' },
      output: null,
      streamedText: '',
      durationMs: null,
    })
    const started = performance.now()
    try {
      const resp = await fetch('/api/run_smart', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ block_id: id, name, args }),
      })
      const out = await resp.text()
      const block = blocks.find((b) => b.id === id)
      if (!block) return
      block.status = { kind: 'Success' }
      block.durationMs = Math.round(performance.now() - started)
      block.streamedText = ''
      block.output = out ? { Text: out } : 'Empty'
    } catch (e) {
      const block = blocks.find((b) => b.id === id)
      if (!block) return
      block.status = { kind: 'Failed', message: String(e) }
      block.streamedText = ''
      block.durationMs = Math.round(performance.now() - started)
    }
  }

  /** Plan 040 M5: cancel the running command. */
  async function cancelCommand() {
    await fetch('/api/cancel', { method: 'POST' })
    const running = blocks.find((b) => b.status.kind === 'Running')
    if (running && running.status.kind === 'Running') {
      running.status = { kind: 'Cancelled' }
    }
  }

  async function complete(line: string, cursor: number): Promise<CompletionItem[]> {
    try {
      const resp = await fetch('/api/complete', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ line, cursor }),
      })
      return await resp.json()
    } catch {
      return []
    }
  }

  async function refreshGit() {
    try {
      const resp = await fetch('/api/prompt_context')
      gitInfo.value = await resp.json()
    } catch {
      // leave existing
    }
  }

  async function openPath(path: string) {
    if (!path.trim()) return
    await fetch('/api/open_path', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    })
  }

  // ── SSE event handling ────────────────────────────────────────────────────

  function applyOutput(o: CommandOutputPayload) {
    const block = blocks.find((b) => b.id === o.block_id)
    if (!block) return
    block.streamedText += o.chunk
  }

  function applyResult(r: CommandResultPayload) {
    cwd.value = r.cwd
    void refreshGit()
    const block = blocks.find((b) => b.id === r.block_id)
    if (!block) return
    if (r.status === 'Success') {
      block.status = { kind: 'Success' }
      block.output = r.output
    } else {
      block.status = { kind: 'Failed', message: r.status.Failed }
      block.output = null
    }
    block.streamedText = ''
    block.durationMs = r.duration_ms
  }

  /** Connect to the SSE stream and dispatch events. */
  function connectSSE() {
    eventSource = new EventSource('/api/stream')
    eventSource.onmessage = (ev) => {
      try {
        const data = JSON.parse(ev.data)
        // ShellEvent is tagged: { event: "command_output", ... } or
        // { event: "command_result", ... }
        if (data.event === 'command_output') {
          applyOutput({ block_id: data.block_id, chunk: data.chunk })
        } else if (data.event === 'command_result') {
          applyResult(data.CommandResult ?? data)
        }
      } catch {
        // ignore parse errors
      }
    }
  }

  // ── Boot ──────────────────────────────────────────────────────────────────

  async function boot() {
    // Boot data
    try {
      const resp = await fetch('/api/command_list')
      const info = await resp.json() as CommandListResult
      cwd.value = info.cwd
      home.value = info.home
      commands.value = info.commands
      smartCommands.value = info.smart_commands
      commandNames.value = info.commands.map((c) => c.name).sort()
    } catch {
      // server not running — leave defaults
    }

    // History
    try {
      const resp = await fetch('/api/history')
      persistedHistory.value = await resp.json()
    } catch {
      persistedHistory.value = []
    }

    // Git info
    await refreshGit()

    // SSE stream
    connectSSE()
  }

  void boot()

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
