<script setup lang="ts">
import { getToolName, type DynamicToolUIPart } from 'ai'
import { isToolStreaming } from '@nuxt/ui/utils/ai'

const props = defineProps<{
  invocation: DynamicToolUIPart
}>()

const toolName = computed(() => getToolName(props.invocation))
const state = computed(() => props.invocation.state)

const isLoading = computed(() => state.value === 'input-streaming')
const isStreaming = computed(() => isToolStreaming(props.invocation))

const statusText = computed(() => {
  switch (state.value) {
    case 'input-streaming':
      return `Calling ${toolName.value}...`
    case 'input-available':
      return `Called ${toolName.value}`
    case 'output-available':
      return `Result from ${toolName.value}`
    case 'output-error':
      return `Error from ${toolName.value}`
    case 'output-denied':
      return `Denied ${toolName.value}`
    default:
      return toolName.value
  }
})

const statusIcon = computed(() => {
  if (state.value === 'output-error' || state.value === 'output-denied') {
    return 'i-lucide-alert-circle'
  }
  if (isStreaming.value) {
    return isLoading.value ? 'i-lucide-loader-circle' : 'i-lucide-wrench'
  }
  return 'i-lucide-check'
})

const input = computed(() => props.invocation.input)
const output = computed(() => props.invocation.output)
const errorText = computed(() => props.invocation.errorText)

function formatData(data: unknown): string {
  if (data === undefined) return ''
  if (typeof data === 'string') return data
  try {
    return JSON.stringify(data, null, 2)
  } catch {
    return String(data)
  }
}
</script>

<template>
  <UChatTool
    :text="statusText"
    :icon="statusIcon"
    :loading="isLoading"
    :streaming="isStreaming"
    variant="card"
    chevron="leading"
  >
    <div class="space-y-3 text-sm">
      <div v-if="input !== undefined">
        <div class="text-xs text-muted mb-1">
          Input
        </div>
        <pre class="bg-muted rounded-md p-2 overflow-x-auto"><code>{{ formatData(input) }}</code></pre>
      </div>
      <div v-if="output !== undefined">
        <div class="text-xs text-muted mb-1">
          Output
        </div>
        <pre class="bg-muted rounded-md p-2 overflow-x-auto"><code>{{ formatData(output) }}</code></pre>
      </div>
      <div v-if="errorText">
        <div class="text-xs text-error mb-1">
          Error
        </div>
        <pre class="bg-muted rounded-md p-2 overflow-x-auto text-error"><code>{{ errorText }}</code></pre>
      </div>
    </div>
  </UChatTool>
</template>
