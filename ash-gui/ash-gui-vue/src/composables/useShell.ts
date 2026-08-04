/**
 * useShell — the frontend's bridge to the Rust Shell worker.
 *
 * The Shell is `!Send` (auto-lang VM uses Rc), so it lives on a dedicated
 * worker thread inside the Tauri backend. This composable:
 *   - invokes `run_command` to submit a command (non-blocking),
 *   - listens on the `command-result` event for finished results,
 *   - exposes reactive state the components bind to.
 */
import { ref, reactive, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  Block,
  CommandResultPayload,
  CommandListResult,
  ToolEntry,
} from '@/types/shell'

export function useShell() {
  const blocks = reactive<Block[]>([])
  const cwd = ref<string>('')
  const home = ref<string>('')
  const commands = ref<ToolEntry[]>([])
  const commandNames = ref<string[]>([])
  let nextId = 0
  let unlisten: UnlistenFn | null = null

  /** Past command lines (newest last), for ↑/↓ history navigation. */
  const history = computed(() =>
    blocks.filter((b) => b.command).map((b) => b.command),
  )

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
      durationMs: null,
    })
    await invoke('run_command', { blockId: id, cmd: trimmed })
  }

  /** Apply a finished command result to its block. */
  function applyResult(r: CommandResultPayload) {
    cwd.value = r.cwd
    const block = blocks.find((b) => b.id === r.block_id)
    if (!block) return
    if (r.status === 'Success') {
      block.status = { kind: 'Success' }
      block.output = r.output
    } else {
      block.status = { kind: 'Failed', message: r.status.Failed }
      block.output = null
    }
    block.durationMs = r.duration_ms
  }

  onMounted(async () => {
    // Boot data: current dir + home + command list (for completion / sidebar).
    const info = await invoke<CommandListResult>('command_list')
    cwd.value = info.cwd
    home.value = info.home
    commands.value = info.commands
    commandNames.value = info.commands.map((c) => c.name).sort()

    // Listen for finished results pushed by the Shell worker thread.
    unlisten = await listen<CommandResultPayload>('command-result', (event) => {
      applyResult(event.payload)
    })
  })

  /** Open a path with the OS default application (best-effort). */
  async function openPath(path: string) {
    if (!path.trim()) return
    await invoke('open_path', { path })
  }

  onUnmounted(() => {
    unlisten?.()
  })

  return { blocks, cwd, home, commands, commandNames, history, runCommand, openPath }
}
