<script setup lang="ts">
/**
 * ToolSidebar — a narrow left rail listing the available commands.
 * Click a command to drop its name into the input (M4, ported from the iced
 * frontend's tool browser). Grouped loosely: commands + SmartCommands.
 */
import { computed } from 'vue'
import type { SmartCommandEntry, ToolEntry } from '@/types/shell'

const props = defineProps<{
  commands: ToolEntry[]
  smartCommands: SmartCommandEntry[]
}>()

const emit = defineEmits<{ (e: 'pick', command: string): void }>()

const commandList = computed(() => props.commands)

function pick(name: string) {
  emit('pick', name)
}
</script>

<template>
  <aside class="w-56 shrink-0 border-r border-border bg-card/30 overflow-y-auto flex flex-col">
    <div class="px-3 py-2 text-[11px] font-semibold text-muted-foreground uppercase tracking-wide">
      Commands
    </div>
    <div class="px-1.5 space-y-0.5">
      <button
        v-for="c in commandList"
        :key="c.name"
        class="w-full text-left px-2 py-1 rounded text-xs font-mono-ash text-sky-300/90 hover:bg-muted/60 hover:text-sky-200 transition-colors flex items-baseline gap-1.5"
        :title="c.description"
        @click="pick(c.name)"
      >
        <span class="shrink-0">{{ c.name }}</span>
        <span v-if="c.description" class="truncate text-[10px] text-muted-foreground/70">
          {{ c.description }}
        </span>
      </button>
    </div>
    <template v-if="props.smartCommands.length">
      <div class="px-3 pt-3 pb-2 text-[11px] font-semibold text-muted-foreground uppercase tracking-wide">
        SmartCommands
      </div>
      <div class="px-1.5 space-y-0.5">
        <button
          v-for="s in props.smartCommands"
          :key="s.name"
          class="w-full text-left px-2 py-1 rounded text-xs font-mono-ash text-purple-300/90 hover:bg-muted/60 hover:text-purple-200 transition-colors flex items-baseline gap-1.5"
          :title="s.description"
          @click="pick(`smart run ${s.name}`)"
        >
          <span class="shrink-0">{{ s.name }}</span>
          <span v-if="s.description" class="truncate text-[10px] text-muted-foreground/70">
            {{ s.description }}
          </span>
        </button>
      </div>
    </template>
  </aside>
</template>
