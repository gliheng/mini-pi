import { useAsyncData } from '#app'
import { usePiRemote, type PiModel } from './usePiRemote'
import { toModelItems, type ModelItem } from '#shared/utils/models'

export function useModels() {
  const remote = usePiRemote()
  const model = useCookie<string>('model', { default: () => '' })
  const { data: apiModels, status, error } = useAsyncData<PiModel[]>(
    'pi-models',
    () => remote.listModels(),
    { server: false, default: () => [] }
  )

  const models = computed<ModelItem[]>(() =>
    toModelItems(apiModels.value ?? [])
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
    rawModels: apiModels,
    model,
    status,
    error
  }
}
