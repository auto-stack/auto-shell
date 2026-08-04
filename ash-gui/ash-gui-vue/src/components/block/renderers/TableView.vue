<script setup lang="ts">
/**
 * TableView — renders a RenderedOutput.Table as a real, aligned HTML table.
 *
 * This fixes the iced prototype's "表格没对齐" problem (which used `row + fixed
 * spacing` instead of a table). Here the browser's own table layout aligns
 * columns; cells get tag-based coloring via cellStyle.
 */
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { AtomType, RenderedCell } from '@/types/shell'
import { cellText, cellTag, isClickable, tagTextClass } from './cellStyle'

const props = defineProps<{
  columns: string[]
  rows: RenderedCell[][]
  atomType: AtomType
}>()

const emit = defineEmits<{ (e: 'openPath', path: string): void }>()

function onCellClick(cell: RenderedCell) {
  const tag = cellTag(cell)
  if (isClickable(tag)) {
    emit('openPath', cellText(cell))
  }
}
</script>

<template>
  <div class="w-full overflow-auto rounded-md border border-border">
    <Table>
      <TableHeader>
        <TableRow class="bg-muted/40 hover:bg-muted/40 border-border">
          <TableHead
            v-for="(col, i) in props.columns"
            :key="i"
            class="h-9 px-3 text-xs font-medium text-muted-foreground"
          >
            {{ col }}
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow
          v-for="(row, ri) in props.rows"
          :key="ri"
          class="border-border/60"
        >
          <TableCell
            v-for="(cell, ci) in row"
            :key="ci"
            class="px-3 py-1.5 text-sm font-mono-ash"
            :class="tagTextClass(cellTag(cell))"
            @click="onCellClick(cell)"
          >
            {{ cellText(cell) }}
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
  </div>
</template>
