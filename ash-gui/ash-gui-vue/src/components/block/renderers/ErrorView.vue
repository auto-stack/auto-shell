<script setup lang="ts">
/** ErrorView — a red-tinted error card. */
defineProps<{ message: string }>()
</script>

<template>
  <div class="rounded-md border border-red-500/40 bg-red-500/10 px-3 py-2">
    <div class="flex items-start gap-2">
      <span class="text-red-400 mt-0.5">✗</span>
      <pre class="text-sm font-mono-ash text-red-300 whitespace-pre-wrap break-words m-0">{{ message }}</pre>
    </div>
  </div>
</template>
