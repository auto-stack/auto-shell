<script setup lang="ts">
/**
 * BlockItem — one command's card. Fixes the iced prototype's "block 无边界":
 * wrapped in a shadcn Card with rounded border + the header carries the ❯
 * prompt, status badge (✓/✗/…), and duration. A metadata row shows the cwd.
 */
import { computed } from 'vue'
import { Card } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import type { Block } from '@/types/shell'
import { abbrevPath } from '@/lib/path'
import BlockBody from './BlockBody.vue'

const props = defineProps<{
  block: Block
  home: string
}>()
const emit = defineEmits<{
  (e: 'openPath', path: string): void
  (e: 'rerun', command: string): void
  /** Plan 040 M5: stop the running command. */
  (e: 'stop'): void
}>()

const cwdDisplay = computed(() => abbrevPath(props.block.cwd, props.home))

const statusIcon = computed(() => {
  switch (props.block.status.kind) {
    case 'Success': return { glyph: '✓', cls: 'text-emerald-500' }
    case 'Failed': return { glyph: '✗', cls: 'text-red-500' }
    case 'Cancelled': return { glyph: '⊘', cls: 'text-amber-500' }
    case 'Running': return { glyph: '…', cls: 'text-amber-500' }
  }
})

const durationLabel = computed(() => {
  const ms = props.block.durationMs
  if (ms === null) return ''
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
})

/** Plan 040 M4: live streamed text to show while the command is running. */
const hasStream = computed(
  () => props.block.status.kind === 'Running' && props.block.streamedText.length > 0,
)

function stop() {
  emit('stop')
}

/** Copy the command line to the clipboard (best-effort). */
function copyCommand() {
  navigator.clipboard?.writeText(props.block.command).catch(() => {})
}

function rerun() {
  emit('rerun', props.block.command)
}
</script>

<template>
  <Card class="border-border bg-card/60 px-3 py-2.5 shadow-sm overflow-hidden">
    <!-- Header row: ❯ command  ...  actions + status badge -->
    <div class="group flex items-center gap-2 mb-1.5">
      <span class="text-muted-foreground select-none">❯</span>
      <span class="font-mono-ash text-sm text-foreground/90 truncate">{{ block.command }}</span>
      <div class="ml-auto flex items-center gap-1.5 shrink-0">
        <!-- Plan 040 M5: stop button while a command is running -->
        <button
          v-if="block.status.kind === 'Running'"
          title="Stop command"
          class="px-1.5 py-0.5 text-[11px] rounded text-red-400 hover:text-red-300 hover:bg-red-500/10 transition-colors"
          @click="stop"
        >
          ■
        </button>
        <!-- Actions (hover-revealed) -->
        <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
          <button
            title="Copy command"
            class="px-1.5 py-0.5 text-[11px] rounded text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors"
            @click="copyCommand"
          >
            ⧉
          </button>
          <button
            title="Re-run"
            class="px-1.5 py-0.5 text-[11px] rounded text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors"
            @click="rerun"
          >
            ↻
          </button>
        </div>
        <Badge
          v-if="durationLabel"
          variant="secondary"
          class="text-[10px] h-5 font-mono-ash"
        >
          {{ durationLabel }}
        </Badge>
        <span class="text-xs font-mono-ash" :class="statusIcon.cls">{{ statusIcon.glyph }}</span>
      </div>
    </div>
    <!-- Metadata row: cwd -->
    <div v-if="cwdDisplay" class="text-[11px] text-muted-foreground/70 font-mono-ash mb-2 truncate">
      📁 {{ cwdDisplay }}
    </div>
    <!-- Plan 040 M4: live streamed output while the command runs -->
    <pre
      v-if="hasStream"
      class="text-xs font-mono-ash text-foreground/80 whitespace-pre-wrap break-all mb-2"
    >{{ block.streamedText }}</pre>
    <!-- Body -->
    <div v-if="block.output" @open-path="emit('openPath', $event)">
      <BlockBody :output="block.output" />
    </div>
  </Card>
</template>
