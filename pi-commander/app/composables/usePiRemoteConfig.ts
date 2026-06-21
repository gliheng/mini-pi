import { useLocalStorage } from '@vueuse/core'
import type { Ref } from 'vue'

export interface PiRemoteConfig {
  baseUrl: string
  token: string
}

interface SharedConfig {
  baseUrl: Ref<string>
  token: Ref<string>
}

let shared: SharedConfig | null = null

export function usePiRemoteConfig() {
  if (!shared) {
    shared = {
      baseUrl: useLocalStorage('pi-remote-base-url', ''),
      token: useLocalStorage('pi-remote-token', '')
    }
  }

  const { baseUrl, token } = shared

  const isConfigured = computed(() => {
    return Boolean(baseUrl.value.trim())
  })

  function set(config: Partial<PiRemoteConfig>) {
    if (config.baseUrl !== undefined) baseUrl.value = config.baseUrl.trim().replace(/\/$/, '')
    if (config.token !== undefined) token.value = config.token.trim()
  }

  function clear() {
    baseUrl.value = ''
    token.value = ''
  }

  return {
    baseUrl,
    token,
    isConfigured,
    set,
    clear
  }
}
