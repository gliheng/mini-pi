import type { PiModel, PiWorkspace } from './usePiRemote'

interface SharedInit {
  models: Ref<PiModel[]>
  workspaces: Ref<PiWorkspace[]>
  loading: Ref<boolean>
  error: Ref<Error | undefined>
  initialized: Ref<boolean>
  refresh: () => Promise<void>
}

let shared: SharedInit | null = null

export function usePiInit() {
  const config = usePiRemoteConfig()
  const remote = usePiRemote()

  if (!shared) {
    const models = ref<PiModel[]>([])
    const workspaces = ref<PiWorkspace[]>([])
    const loading = ref(false)
    const error = ref<Error | undefined>(undefined)
    const initialized = ref(false)

    async function initialize() {
      if (!config.isConfigured.value) {
        models.value = []
        workspaces.value = []
        initialized.value = true
        return
      }

      loading.value = true
      error.value = undefined

      try {
        const [modelList, workspaceList] = await Promise.all([
          remote.listModels(),
          remote.listWorkspaces()
        ])
        models.value = modelList
        workspaces.value = workspaceList
        initialized.value = true
      } catch (err) {
        const normalized = err instanceof Error ? err : new Error(String(err))
        error.value = normalized
        models.value = []
        workspaces.value = []
      } finally {
        loading.value = false
      }
    }

    // Re-initialize when the tunnel endpoint changes.
    watch(() => config.baseUrl.value, () => {
      initialized.value = false
      initialize()
    }, { immediate: true })

    shared = {
      models,
      workspaces,
      loading,
      error,
      initialized,
      refresh: initialize
    }
  }

  return {
    models: shared.models,
    workspaces: shared.workspaces,
    loading: shared.loading,
    error: shared.error,
    initialized: shared.initialized,
    refresh: shared.refresh
  }
}
