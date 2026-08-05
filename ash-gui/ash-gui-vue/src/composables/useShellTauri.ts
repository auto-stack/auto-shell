/**
 * useShell — the frontend's bridge to the Rust Shell worker.
 *
 * The Shell is `!Send` (auto-lang VM uses Rc), so it lives on a dedicated
 * worker thread inside the Tauri backend. This composable:
 *   - invokes `run_command` to submit a command (non-blocking),
 *   - listens on the `command-result` event for finished results,
 *   - listens on the `command-output` event for streamed chunks (Plan 040 M4),
 *   - exposes reactive state the components bind to.
 *
 * Plan 040:
 *   - M3: `runSmartCommand` runs a SmartCommand by name (not via text injection).
 *   - M4: streaming chunks append to a Running block's `streamedText`.
 *   - M5: `cancelCommand` cancels the running command.
 *   - M6: history is loaded from the shared CLI file at boot + merged with the
 *         current session's commands, so ↑/↓ navigation sees both.
 */
import { ref, reactive, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
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

export function useShellTauri() {
  const blocks = reactive<Block[]>([])
  const cwd = ref<string>('')
  const home = ref<string>('')
  const commands = ref<ToolEntry[]>([])
  const smartCommands = ref<SmartCommandEntry[]>([])
  const commandNames = ref<string[]>([])
  /** Plan 041 M5: git branch/status for the prompt. Refreshed after each
   * command (cwd may change) and at boot. */
  const gitInfo = ref<PromptContext>({ git_branch: null, git_status: null })
  /** Commands persisted in the shared CLI history file (oldest first, M6). */
  const persistedHistory = ref<string[]>([])
  let nextId = 0
  let unlistenResult: UnlistenFn | null = null
  let unlistenOutput: UnlistenFn | null = null

  /**
   * Past command lines (newest last), for ↑/↓ history navigation. Plan 040 M6:
   * the shared CLI file history (loaded at boot) followed by this session's
   * commands, so navigation sees everything in chronological order.
   */
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
    await invoke('run_command', { blockId: id, cmd: trimmed })
  }

  /** Plan 040 M3: run a SmartCommand by name (no text injection). */
  async function runSmartCommand(name: string, args: string[] = []) {
    // Show a Running block so the user sees feedback.
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
      // Pass blockId so the worker's OutputHook attributes streamed body output
      // (command-output events) to this block while it runs.
      const out = await invoke<string>('run_smart_command', { blockId: id, name, args })
      const block = blocks.find((b) => b.id === id)
      if (!block) return
      block.status = { kind: 'Success' }
      block.durationMs = Math.round(performance.now() - started)
      block.streamedText = '' // final result replaces the stream
      block.output = out ? { Text: out } : 'Empty'
    } catch (e) {
      const block = blocks.find((b) => b.id === id)
      if (!block) return
      block.status = { kind: 'Failed', message: String(e) }
      block.streamedText = ''
      block.durationMs = Math.round(performance.now() - started)
    }
  }

  /** Plan 040 M5: cancel the running command (best-effort). Marks the newest
   * Running block as Cancelled optimistically; the worker confirms via
   * `command-result` (Failed "cancelled") if it actually stopped the stream. */
  async function cancelCommand() {
    await invoke('cancel_command')
    // Optimistically mark the latest Running block. The final command-result
    // will overwrite with the real status (Failed "cancelled").
    const running = blocks.find((b) => b.status.kind === 'Running')
    if (running && running.status.kind === 'Running') {
      running.status = { kind: 'Cancelled' }
    }
  }

  /** Append a streamed chunk to its Running block (Plan 040 M4). */
  function applyOutput(o: CommandOutputPayload) {
    const block = blocks.find((b) => b.id === o.block_id)
    if (!block) return
    block.streamedText += o.chunk
  }

  /** Plan 041 M5: refresh the git branch/status from the backend (after cwd
   * changes via cd, or at boot). Best-effort — failures leave the old info. */
  async function refreshGit() {
    try {
      gitInfo.value = await invoke<PromptContext>('prompt_context')
    } catch {
      // leave existing info
    }
  }

  /** Apply a finished command result to its block. */
  function applyResult(r: CommandResultPayload) {
    cwd.value = r.cwd
    // Plan 041 M5: cwd may have changed (cd/pushd) → refresh git info.
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
    block.streamedText = '' // final output replaces the stream
    block.durationMs = r.duration_ms
  }

  onMounted(async () => {
    // Boot data: current dir + home + command list (for completion / sidebar).
    const info = await invoke<CommandListResult>('command_list')
    cwd.value = info.cwd
    home.value = info.home
    commands.value = info.commands
    smartCommands.value = info.smart_commands
    commandNames.value = info.commands.map((c) => c.name).sort()

    // Plan 040 M6: load the shared CLI history file for ↑/↓ navigation.
    try {
      persistedHistory.value = await invoke<string[]>('history')
    } catch {
      persistedHistory.value = []
    }
    // Plan 041 M5: initial git info for the prompt.
    await refreshGit()

    // Listen for finished results pushed by the Shell worker thread.
    unlistenResult = await listen<CommandResultPayload>('command-result', (event) => {
      applyResult(event.payload)
    })
    // Plan 040 M4: streamed chunks for long external commands.
    unlistenOutput = await listen<CommandOutputPayload>('command-output', (event) => {
      applyOutput(event.payload)
    })
  })

  /** Open a path with the OS default application (best-effort). */
  async function openPath(path: string) {
    if (!path.trim()) return
    await invoke('open_path', { path })
  }

  /** Plan 041 M7: produce completions via the shared backend engine (same one
   * CLI/TUI use). Returns candidates with description/kind for richer rendering. */
  async function complete(line: string, cursor: number): Promise<CompletionItem[]> {
    try {
      return await invoke<CompletionItem[]>('complete', { line, cursor })
    } catch {
      return []
    }
  }

  onUnmounted(() => {
    unlistenResult?.()
    unlistenOutput?.()
  })

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
