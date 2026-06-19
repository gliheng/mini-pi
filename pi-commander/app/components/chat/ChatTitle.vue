<script setup lang="ts">
const props = defineProps<{
  chatId: string
  title?: string | null
  isOwner: boolean
}>()

const displayTitle = computed(() => props.title || 'Untitled')
</script>

<template>
  <span class="text-sm font-medium text-highlighted truncate min-w-0 max-w-3xs">
    {{ displayTitle }}
  </span>
</template>
