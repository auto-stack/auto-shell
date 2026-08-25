<script setup lang="ts">
// Plan 055 stub: codegen 的 button 元素引用 <Button>,但 library 未生成
// ui/button(仅 progress/table)。此 stub 让 import 解析通过(class prop +
// slot + @click fallthrough 到原生 button)。根因(codegen button→Button 但
// LIBRARY_WIDGETS 不含 button)后续 codegen 修。
</script>

<template>
  <button><slot /></button>
</template>
