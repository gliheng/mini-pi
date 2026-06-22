<script setup lang="ts">
const { level, items } = useThinkingLevel()
</script>

<template>
  <USelectMenu
    v-model="level"
    :items="items"
    size="sm"
    variant="ghost"
    value-key="value"
    label-key="label"
    class="data-[state=open]:bg-elevated"
    :ui="{
      trailingIcon: 'group-data-[state=open]:rotate-180 transition-transform duration-200'
    }"
  />
</template>
