<script setup lang="ts">
/** BlockList — a scrollable list of BlockItem cards, newest at the bottom. */
import { ref, watch, nextTick } from 'vue'
import type { Block } from '@/types/shell'
import BlockItem from './BlockItem.vue'

const props = defineProps<{ blocks: Block[]; home: string }>()
const emit = defineEmits<{
  (e: 'openPath', path: string): void
  (e: 'rerun', command: string): void
  /** Plan 040 M5: stop the running command (from a Running block's stop button). */
  (e: 'stop'): void
}>()

const scrollRef = ref<HTMLElement | null>(null)

// Auto-scroll to the newest block when the list grows.
watch(
  () => props.blocks.length,
  async () => {
    await nextTick()
    const el = scrollRef.value
    if (el) el.scrollTop = el.scrollHeight
  },
)
</script>

<template>
  <div ref="scrollRef" class="flex-1 overflow-y-auto p-3 space-y-2.5">
    <div v-if="blocks.length === 0" class="h-full flex items-center justify-center">
      <div class="text-center space-y-1">
        <p class="text-sm text-muted-foreground">No commands yet</p>
        <p class="text-xs text-muted-foreground/60">
          Type a command below, e.g. <code class="text-foreground/80">ls -al</code>
        </p>
      </div>
    </div>
    <BlockItem
      v-for="b in blocks"
      :key="b.id"
      :block="b"
      :home="props.home"
      @open-path="emit('openPath', $event)"
      @rerun="emit('rerun', $event)"
      @stop="emit('stop')"
    />
  </div>
</template>
