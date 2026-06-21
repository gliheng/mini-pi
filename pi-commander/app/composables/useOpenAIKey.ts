import { useLocalStorage } from '@vueuse/core'
import type { Ref } from 'vue'

interface SharedConfig {
  apiKey: Ref<string>
  isConfigured: ComputedRef<boolean>
  set: (value: string) => void
}

let shared: SharedConfig | null = null

export function useOpenAIKey() {
  if (!shared) {
    const apiKey = useLocalStorage('openai-api-key', '')

    const isConfigured = computed(() => {
      return Boolean(apiKey.value.trim())
    })

    function set(value: string) {
      apiKey.value = value.trim()
    }

    shared = {
      apiKey,
      isConfigured,
      set
    }
  }

  return shared
}
