<script setup lang="ts">
/**
 * CodeView — Plan 042 M6 (B1): renders syntax-highlighted code from
 * structured CodeSpan data (RGB + bold/italic). No HTML, no ANSI — just
 * CSS inline styles from the span's color fields. Platform-agnostic.
 */
import type { CodeSpan } from '@/types/shell'

defineProps<{
  lines: CodeSpan[][]
  language: string
}>()
</script>

<template>
  <pre class="text-sm font-mono-ash whitespace-pre overflow-x-auto m-0"><template v-for="(line, lineIdx) in lines" :key="lineIdx"><span
      v-for="(span, spanIdx) in line"
      :key="spanIdx"
      :style="{
        color: `rgb(${span.r}, ${span.g}, ${span.b})`,
        fontWeight: span.bold ? 'bold' : 'normal',
        fontStyle: span.italic ? 'italic' : 'normal',
      }"
    >{{ span.text }}</span><span v-if="lineIdx < lines.length - 1">&#10;</span></template></pre>
</template>
