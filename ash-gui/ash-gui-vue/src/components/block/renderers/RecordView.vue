<script setup lang="ts">
/**
 * RecordView — a key/value list. MemoryInfo additionally shows a usage Progress
 * bar if a `usage_percent` / `usage` field is present (mirrors the iced
 * `memory_usage_bar`).
 */
import { computed } from 'vue'
import { Progress } from '@/components/ui/progress'
import type { AtomType, RecordField } from '@/types/shell'
import { cellText } from './cellStyle'

const props = defineProps<{
  fields: RecordField[]
  atomType: AtomType
}>()

const isMemory = computed(() => props.atomType === 'MemoryInfo')

const usagePct = computed(() => {
  if (!isMemory.value) return null
  const found = props.fields.find(([k]) => k === 'usage_percent' || k === 'usage')
  if (!found) return null
  const raw = cellText(found[1]).trim().replace(/%$/, '')
  const n = Number(raw)
  return Number.isFinite(n) ? n : null
})
</script>

<template>
  <div class="space-y-3">
    <div v-if="usagePct !== null" class="space-y-1">
      <div class="text-xs text-muted-foreground">memory usage: {{ Math.round(usagePct) }}%</div>
      <Progress :model-value="usagePct" />
    </div>
    <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm font-mono-ash">
      <template v-for="([key, cell], i) in props.fields" :key="i">
        <dt class="text-muted-foreground">{{ key }}</dt>
        <dd class="text-foreground/90">{{ cellText(cell) }}</dd>
      </template>
    </dl>
  </div>
</template>
