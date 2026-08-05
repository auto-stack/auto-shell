<script setup lang="ts">
/**
 * HistorySearch — Plan 041 M1: a fzf-style history search popover.
 *
 * Triggered by Ctrl+R in the PromptBar. Shows a floating panel listing matching
 * history entries (newest first), filtered live by a fuzzy substring match on
 * the query. ↑/↓ navigates, Enter runs the selected entry, Esc/Backspace-empty
 * closes. Mirrors the TUI's Ctrl+R history menu (`repl.rs:169-173`).
 */
import { ref, computed, watch, nextTick } from 'vue'

const props = defineProps<{
  /** Past command lines (newest last). */
  history: string[]
  /** Whether the panel is open. */
  open: boolean
}>()

const emit = defineEmits<{
  (e: 'run', command: string): void
  (e: 'close'): void
}>()

const query = ref('')
const selected = ref(0)
const queryEl = ref<HTMLInputElement | null>(null)

// Matching entries: newest first, fuzzy substring match (case-insensitive).
// Empty query shows the most recent entries.
const matches = computed(() => {
  const q = query.value.trim().toLowerCase()
  const reversed = [...props.history].reverse() // newest first
  const filtered = q
    ? reversed.filter((h) => h.toLowerCase().includes(q))
    : reversed
  return filtered.slice(0, 50) // cap for perf
})

// Reset selection + query whenever the panel opens.
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      query.value = ''
      selected.value = 0
      nextTick(() => queryEl.value?.focus())
    }
  },
)

// Keep selection in range as the filtered list changes.
watch(matches, (m) => {
  if (selected.value >= m.length) selected.value = Math.max(0, m.length - 1)
})

function runSelected() {
  const entry = matches.value[selected.value]
  if (entry) {
    emit('run', entry)
    emit('close')
  }
}

function move(delta: number) {
  if (matches.value.length === 0) return
  selected.value = (selected.value + delta + matches.value.length) % matches.value.length
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowUp') {
    e.preventDefault()
    move(-1)
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    move(1)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    runSelected()
  } else if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
  } else if (e.key === 'Backspace' && query.value === '') {
    // Backspace on empty query closes (like fzf-history).
    emit('close')
  }
}
</script>

<template>
  <div
    v-if="open"
    class="absolute bottom-full left-0 right-0 mb-1 max-h-64 overflow-hidden rounded-md border border-border bg-popover shadow-lg flex flex-col z-50"
  >
    <!-- Query input -->
    <div class="flex items-center gap-2 px-3 py-1.5 border-b border-border">
      <span class="text-xs text-amber-400 font-mono-ash select-none shrink-0">⌕</span>
      <input
        ref="queryEl"
        v-model="query"
        @keydown="onKeydown"
        spellcheck="false"
        autocomplete="off"
        placeholder="搜索历史…"
        class="flex-1 bg-transparent outline-none text-sm font-mono-ash placeholder:text-muted-foreground/40"
      />
      <span class="text-[10px] text-muted-foreground/50 select-none shrink-0">
        {{ matches.length }} 条 · ↑↓ 选择 · ⏎ 执行 · Esc 关闭
      </span>
    </div>
    <!-- Match list -->
    <div class="overflow-y-auto py-1">
      <button
        v-for="(entry, idx) in matches"
        :key="entry + idx"
        @click="selected = idx; runSelected()"
        @mousemove="selected = idx"
        class="w-full text-left px-3 py-1 text-sm font-mono-ash transition-colors flex items-baseline gap-2"
        :class="idx === selected ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:bg-muted/50'"
      >
        <span class="text-[10px] text-muted-foreground/40 shrink-0 w-6 text-right">{{ idx + 1 }}</span>
        <span class="truncate">{{ entry }}</span>
      </button>
      <div v-if="matches.length === 0" class="px-3 py-4 text-center text-xs text-muted-foreground/50">
        无匹配历史
      </div>
    </div>
  </div>
</template>
