import { useLocalStorage } from '@vueuse/core'

export interface PiRemoteConfig {
  baseUrl: string
  token: string
}

export function usePiRemoteConfig() {
  const baseUrl = useLocalStorage('pi-remote-base-url', '')
  const token = useLocalStorage('pi-remote-token', '')

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
    baseUrl: computed(() => baseUrl.value),
    token: computed(() => token.value),
    isConfigured,
    set,
    clear
  }
}
