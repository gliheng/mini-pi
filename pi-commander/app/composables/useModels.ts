import { toModelItems, type ModelItem } from '#shared/utils/models'

export function useModels() {
  const init = usePiInit()
  const model = useCookie<string>('model', { default: () => '' })

  const models = computed<ModelItem[]>(() =>
    toModelItems(init.models.value)
  )

  const status = computed<'pending' | 'success' | 'error'>(() =>
    init.loading.value ? 'pending' : init.error.value ? 'error' : 'success'
  )

  // Fall back to the first available model when the cookie is missing or stale.
  watchEffect(() => {
    const items = models.value
    if (items.length === 0) return
    const current = model.value
    const first = items[0]
    if (first && (!current || !items.some(m => m.value === current))) {
      model.value = first.value
    }
  })

  return {
    models,
    rawModels: init.models,
    model,
    status,
    error: init.error,
    refresh: init.refresh
  }
}
