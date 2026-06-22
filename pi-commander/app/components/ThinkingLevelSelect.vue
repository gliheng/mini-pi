<script setup lang="ts">
const { level, items } = useThinkingLevel()
</script>

<template>
  <USelect
    v-model="level"
    :items="items"
    size="sm"
    variant="ghost"
    value-key="value"
    label-key="label"
    class="data-[state=open]:bg-elevated"
  />
</template>
