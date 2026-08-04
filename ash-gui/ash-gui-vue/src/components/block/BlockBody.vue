<script setup lang="ts">
/**
 * BlockBody — dispatches a RenderedOutput to the right renderer.
 *
 * We compute the discriminant in script (not the template) because the
 * RenderedOutput union includes a bare-string-literal variant ('Empty') which
 * Vue's template type-narrowing can't cleanly narrow through a chain of
 * `v-else-if "X" in output`.
 */
import { computed } from 'vue'
import TextView from './renderers/TextView.vue'
import RecordView from './renderers/RecordView.vue'
import TableView from './renderers/TableView.vue'
import ErrorView from './renderers/ErrorView.vue'
import type { AtomType, RecordField, RenderedCell, RenderedOutput } from '@/types/shell'

const props = defineProps<{ output: RenderedOutput }>()
const emit = defineEmits<{ (e: 'openPath', path: string): void }>()

type Case =
  | { kind: 'Table'; columns: string[]; rows: RenderedCell[][]; atomType: AtomType }
  | { kind: 'Record'; fields: RecordField[]; atomType: AtomType }
  | { kind: 'Text'; text: string }
  | { kind: 'Error'; message: string }
  | { kind: 'Empty' }

const which = computed<Case>(() => {
  const o = props.output
  if (typeof o === 'string') return { kind: 'Empty' }
  if ('Table' in o) {
    return {
      kind: 'Table',
      columns: o.Table.columns,
      rows: o.Table.rows,
      atomType: o.Table.atom_type,
    }
  }
  if ('Record' in o) {
    return {
      kind: 'Record',
      fields: o.Record.fields,
      atomType: o.Record.atom_type,
    }
  }
  if ('Text' in o) return { kind: 'Text', text: o.Text }
  if ('Error' in o) return { kind: 'Error', message: o.Error.message }
  return { kind: 'Empty' }
})
</script>

<template>
  <TableView
    v-if="which.kind === 'Table'"
    :columns="which.columns"
    :rows="which.rows"
    :atom-type="which.atomType"
    @open-path="emit('openPath', $event)"
  />
  <RecordView
    v-else-if="which.kind === 'Record'"
    :fields="which.fields"
    :atom-type="which.atomType"
  />
  <TextView v-else-if="which.kind === 'Text'" :text="which.text" />
  <ErrorView v-else-if="which.kind === 'Error'" :message="which.message" />
  <div v-else class="text-xs text-muted-foreground italic">(no output)</div>
</template>
