<script setup lang="ts">
/**
 * PromptBar — the bottom command input. Fixes the iced prototype's "没 prompt"
 * and "不知当前目录": shows a ❯ prompt symbol + the cwd, with a completion
 * suggestion row and ↑/↓ history navigation.
 *
 * Plan 041 M7: completion candidates come from the shared backend engine
 * (the same one CLI/TUI use) via the `complete` prop — file paths, flags,
 * subcommands, descriptions — not just a command-name prefix filter.
 */
import { ref, computed, watch, nextTick } from 'vue'
import { abbrevPath } from '@/lib/path'
import type { CompletionItem } from '@/types/shell'

const cwdDisplay = computed(() => abbrevPath(props.cwd, props.home))

const props = defineProps<{
  cwd: string
  home: string
  commandNames: string[]
  /** Past command lines (newest last), for ↑/↓ history navigation. */
  history: string[]
  /** When set (non-empty), the input is replaced with this command once. */
  injectedCommand?: string
  /** Plan 041 M7: backend completion (shared engine). Returns candidates with
   * description/kind for richer rendering than a prefix filter. */
  complete?: (line: string, cursor: number) => Promise<CompletionItem[]>
}>()

const emit = defineEmits<{
  (e: 'run', command: string): void
  (e: 'injected'): void
}>()

const input = ref('')
const inputEl = ref<HTMLInputElement | null>(null)
const historyCursor = ref<number | null>(null)

// Plan 041 M7: completion candidates from the shared backend engine. Async —
// fetched (debounced) on input change. Falls back to a local command-name
// prefix filter if no `complete` prop (e.g. during early boot).
const suggestions = ref<CompletionItem[]>([])
let completeTimer: ReturnType<typeof setTimeout> | null = null
let completeSeq = 0

function refreshCompletions() {
  const line = input.value
  // No backend: fall back to the old command-name prefix filter.
  if (!props.complete) {
    const first = line.split(/\s+/)[0] ?? ''
    suggestions.value = first
      ? props.commandNames
          .filter((n) => n.startsWith(first))
          .slice(0, 8)
          .map((n) => ({ replacement: n, display: n, description: null, kind: 'command' }))
      : []
    return
  }
  // Debounce: the backend engine may probe `--help` (cache-after-first), so we
  // avoid firing on every keystroke. 80ms feels instant yet coalesces typing.
  if (completeTimer) clearTimeout(completeTimer)
  const seq = ++completeSeq
  completeTimer = setTimeout(async () => {
    const items = await props.complete!(line, line.length)
    // Drop stale results if the user typed past this fetch.
    if (seq === completeSeq) suggestions.value = items.slice(0, 8)
  }, 80)
}

// Reset history cursor + refresh completions whenever the user types.
watch(input, () => {
  historyCursor.value = null
  refreshCompletions()
})

// Inject a command from the sidebar (or elsewhere) into the input.
watch(
  () => props.injectedCommand,
  (cmd) => {
    if (cmd) {
      input.value = cmd
      historyCursor.value = null
      emit('injected')
      nextTick(() => {
        inputEl.value?.focus()
        inputEl.value?.setSelectionRange(cmd.length, cmd.length)
      })
    }
  },
)

function run() {
  const cmd = input.value
  if (!cmd.trim()) return
  input.value = ''
  historyCursor.value = null
  suggestions.value = []
  emit('run', cmd)
}

/** Navigate history: older=true (↑), newer=false (↓). */
function navigateHistory(older: boolean) {
  if (props.history.length === 0) return
  const cur = historyCursor.value ?? props.history.length
  const next = older ? Math.max(0, cur - 1) : Math.min(props.history.length, cur + 1)
  // ↑ beyond newest → show the last command; ↓ past newest → clear.
  if (!older && next >= props.history.length) {
    historyCursor.value = null
    input.value = ''
    return
  }
  historyCursor.value = next
  input.value = props.history[next] ?? ''
}

/** Apply a completion: replace the last token with the candidate's replacement. */
function pickCompletion(item: CompletionItem) {
  const parts = input.value.split(/\s+/)
  // Replace the last token (the one being completed).
  parts[parts.length - 1] = item.replacement
  input.value = parts.join(' ')
  nextTick(() => {
    inputEl.value?.focus()
    const len = input.value.length
    inputEl.value?.setSelectionRange(len, len)
  })
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowUp') {
    e.preventDefault()
    navigateHistory(true)
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    navigateHistory(false)
  }
}
</script>

<template>
  <div class="border-t border-border bg-background/80 backdrop-blur px-3 py-2 space-y-1">
    <!-- Completion suggestions (Plan 041 M7: from the shared backend engine) -->
    <div v-if="suggestions.length" class="flex flex-wrap gap-1.5">
      <button
        v-for="(s, idx) in suggestions"
        :key="s.replacement + idx"
        @click="pickCompletion(s)"
        :title="s.description ?? ''"
        class="text-xs font-mono-ash px-2 py-0.5 rounded bg-muted/60 hover:bg-muted text-sky-300 transition-colors flex items-baseline gap-1"
      >
        <span>{{ s.display }}</span>
        <span v-if="s.description" class="text-[10px] text-muted-foreground/60 truncate max-w-[12rem]">
          {{ s.description }}
        </span>
      </button>
    </div>
    <!-- Input row: ❯ cwd + input -->
    <div class="flex items-center gap-2">
      <span class="text-emerald-500 font-mono-ash select-none shrink-0">❯</span>
      <span class="text-[11px] text-sky-300/80 font-mono-ash shrink-0 max-w-[40%] truncate" :title="props.cwd">
        {{ props.cwd ? cwdDisplay : '…' }}
      </span>
      <input
        ref="inputEl"
        v-model="input"
        @keydown.enter="run"
        @keydown="onKeydown"
        spellcheck="false"
        autocomplete="off"
        placeholder="type a command…"
        class="flex-1 bg-transparent outline-none text-sm font-mono-ash placeholder:text-muted-foreground/40"
      />
    </div>
  </div>
</template>
