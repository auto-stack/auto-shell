<script setup lang="ts">
/** TextView — plain preformatted text, monospace, preserves whitespace. */
defineProps<{ text: string }>()
</script>

<template>
  <pre class="text-sm font-mono-ash text-foreground/90 whitespace-pre-wrap break-words m-0">{{ text }}</pre>
</template>
