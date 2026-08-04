<script setup lang="ts">
/**
 * PromptBar — the bottom command input. Fixes the iced prototype's "没 prompt"
 * and "不知当前目录": shows a ❯ prompt symbol + the cwd, with a completion
 * suggestion row and ↑/↓ history navigation.
 */
import { ref, computed, watch, nextTick } from 'vue'
import { abbrevPath } from '@/lib/path'

const cwdDisplay = computed(() => abbrevPath(props.cwd, props.home))

const props = defineProps<{
  cwd: string
  home: string
  commandNames: string[]
  /** Past command lines (newest last), for ↑/↓ history navigation. */
  history: string[]
  /** When set (non-empty), the input is replaced with this command once. */
  injectedCommand?: string
}>()

const emit = defineEmits<{
  (e: 'run', command: string): void
  (e: 'injected'): void
}>()

const input = ref('')
const inputEl = ref<HTMLInputElement | null>(null)
const historyCursor = ref<number | null>(null)

// Completion suggestions: match the first token (command name) as a prefix.
const suggestions = computed(() => {
  const first = input.value.split(/\s+/)[0] ?? ''
  if (!first) return [] as string[]
  return props.commandNames.filter((n) => n.startsWith(first)).slice(0, 8)
})

// Reset history cursor whenever the user types.
watch(input, () => {
  historyCursor.value = null
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

function pickCompletion(name: string) {
  const rest = input.value.split(/\s+/).slice(1).join(' ')
  input.value = rest ? `${name} ${rest}` : name
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
    <!-- Completion suggestions -->
    <div v-if="suggestions.length" class="flex flex-wrap gap-1.5">
      <button
        v-for="s in suggestions"
        :key="s"
        @click="pickCompletion(s)"
        class="text-xs font-mono-ash px-2 py-0.5 rounded bg-muted/60 hover:bg-muted text-sky-300 transition-colors"
      >
        {{ s }}
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
