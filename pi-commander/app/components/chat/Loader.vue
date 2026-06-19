<script setup lang="ts">
interface Props {
  count?: number
}

withDefaults(defineProps<Props>(), {
  count: 4
})
</script>

<template>
  <div class="flex flex-col gap-6 p-4 animate-pulse">
    <div
      v-for="i in count"
      :key="i"
      class="flex w-full"
      :class="i % 2 === 0 ? 'justify-end' : 'justify-start'"
    >
      <div class="flex items-end gap-3 max-w-[80%]" :class="i % 2 === 0 ? 'flex-row-reverse' : 'flex-row'">
        <USkeleton
          v-if="i % 2 !== 0"
          class="size-8 rounded-full shrink-0"
        />
        <div class="flex flex-col gap-2" :class="i % 2 === 0 ? 'items-end' : 'items-start'">
          <USkeleton
            class="h-4 rounded-md"
            :class="i % 2 === 0 ? 'w-48 bg-primary/30' : 'w-64'"
          />
          <USkeleton
            class="h-4 rounded-md"
            :class="i % 2 === 0 ? 'w-32 bg-primary/30' : 'w-48'"
          />
          <USkeleton
            v-if="i % 2 !== 0"
            class="h-4 rounded-md w-24"
          />
        </div>
      </div>
    </div>
  </div>
</template>
