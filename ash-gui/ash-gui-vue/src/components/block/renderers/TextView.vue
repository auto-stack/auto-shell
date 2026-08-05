<script setup lang="ts">
/**
 * TextView — preformatted text output, with ANSI color rendering.
 *
 * Plan 042 bugfix: the backend's `show` command syntax-highlights code files
 * (.rs/.py/etc.) with ANSI color escapes. This component converts them to
 * colored HTML spans so code shows up properly in the GUI. Plain text (no
 * ANSI) renders as-is.
 */
import { computed } from 'vue'
import { ansiToSegments } from '@/lib/ansi'

const props = defineProps<{ text: string }>()

const segments = computed(() => ansiToSegments(props.text))
</script>

<template>
  <pre class="text-sm font-mono-ash text-foreground/90 whitespace-pre-wrap break-words m-0"><span
    v-for="(seg, idx) in segments"
    :key="idx"
    :style="seg.style"
  >{{ seg.text }}</span></pre>
</template>
