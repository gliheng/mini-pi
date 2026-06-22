<script setup lang="ts">
import type { PiWorkspace } from '~/composables/usePiRemote'

const input = ref('')
const loading = ref(false)
const workspaces = ref<PiWorkspace[]>([])
const loadingWorkspaces = ref(false)
const workspacesError = ref<Error | undefined>(undefined)
const selectedWorkspaceId = ref<string | null>(null)

const config = usePiRemoteConfig()
const remote = usePiRemote()
const voice = useVoiceInput()
const { model } = useModels()
const settings = usePiRemoteSettingsModal()
const toast = useToast()

const canUseVoice = computed(() =>
  voice.supported.value
  && !loading.value
)

async function toggleVoiceInput() {
  if (voice.isRecording.value) {
    try {
      const blob = await voice.stopRecording()
      const text = await voice.transcribe(blob)
      input.value = input.value.trimEnd()
      input.value = input.value ? `${input.value} ${text}` : text
    } catch (err) {
      toast.add({
        description: err instanceof Error ? err.message : String(err),
        icon: 'i-lucide-alert-circle',
        color: 'error'
      })
    }
    return
  }

  try {
    await voice.startRecording()
  } catch (err) {
    toast.add({
      description: err instanceof Error ? err.message : String(err),
      icon: 'i-lucide-alert-circle',
      color: 'error'
    })
  }
}

async function openSettings() {
  await settings.open()
}

async function loadWorkspaces() {
  if (!config.isConfigured.value) return
  loadingWorkspaces.value = true
  workspacesError.value = undefined
  try {
    const list = await remote.listWorkspaces()
    workspaces.value = list
    const first = list[0]
    if (first && !selectedWorkspaceId.value) {
      selectedWorkspaceId.value = first.id
    }
  } catch (err) {
    workspacesError.value = err instanceof Error ? err : new Error(String(err))
  } finally {
    loadingWorkspaces.value = false
  }
}

onMounted(async () => {
  if (!config.isConfigured.value) {
    await openSettings()
  }
  await loadWorkspaces()
})

watch(() => config.isConfigured.value, async (isConfigured) => {
  if (isConfigured) {
    await loadWorkspaces()
  } else {
    workspaces.value = []
    selectedWorkspaceId.value = null
  }
})

async function createChat(prompt: string) {
  if (!config.isConfigured.value) {
    await openSettings()
    if (!config.isConfigured.value) return
  }

  if (!selectedWorkspaceId.value) {
    toast.add({
      description: 'Please select a workspace first',
      icon: 'i-lucide-alert-circle',
      color: 'error'
    })
    return
  }

  input.value = prompt
  loading.value = true

  try {
    const { thread_id } = await remote.createThread(
      model.value || undefined,
      selectedWorkspaceId.value
    )
    const pendingPrompt = useState<string | null>(`pending-prompt-${thread_id}`, () => null)
    pendingPrompt.value = prompt
    await navigateTo(`/chat/${thread_id}`)
  } catch (err) {
    toast.add({
      description: err instanceof Error ? err.message : String(err),
      icon: 'i-lucide-alert-circle',
      color: 'error'
    })
  } finally {
    loading.value = false
  }
}

async function onSubmit() {
  await createChat(input.value)
}
</script>

<template>
  <UDashboardPanel
    id="home"
    class="min-h-0"
    :ui="{ body: 'p-0 sm:p-0' }"
  >
    <template #header>
      <Navbar />
    </template>

    <template #body>
      <UContainer class="flex-1 flex flex-col justify-center gap-4 sm:gap-6 py-8">
        <div class="flex flex-col gap-2">
          <h1 class="text-3xl sm:text-4xl text-highlighted font-bold">
            pi commander
          </h1>
          <p class="text-muted">
            Chat with your mini-pi agent through its Cloudflare tunnel.
          </p>
        </div>

        <div class="flex flex-col gap-3">
          <div class="flex items-center justify-between">
            <h2 class="text-sm font-semibold text-highlighted">
              Workspace
            </h2>
            <UButton
              v-if="!config.isConfigured.value"
              size="xs"
              color="neutral"
              variant="ghost"
              label="Configure"
              @click="openSettings"
            />
          </div>

          <div
            v-if="loadingWorkspaces"
            class="grid grid-cols-1 sm:grid-cols-2 gap-3"
          >
            <USkeleton class="h-20" />
            <USkeleton class="h-20" />
          </div>

          <div
            v-else-if="workspacesError"
            class="text-sm text-error"
          >
            {{ workspacesError.message }}
          </div>

          <div
            v-else-if="!workspaces.length"
            class="text-sm text-muted"
          >
            No workspaces found. Create one in mini-pi first.
          </div>

          <div
            v-else
            class="grid grid-cols-1 sm:grid-cols-2 gap-3"
          >
            <button
              v-for="workspace in workspaces"
              :key="workspace.id"
              type="button"
              class="text-left p-4 rounded-lg border transition-colors"
              :class="selectedWorkspaceId === workspace.id
                ? 'border-primary bg-primary/5'
                : 'border-default bg-default hover:bg-muted/50'"
              @click="selectedWorkspaceId = workspace.id"
            >
              <div class="flex items-start gap-3">
                <UIcon
                  name="i-lucide-folder"
                  class="size-5 mt-0.5 text-muted"
                />
                <div class="min-w-0 flex-1">
                  <div class="font-medium text-highlighted truncate">
                    {{ workspace.name }}
                  </div>
                  <div class="text-xs text-muted truncate">
                    {{ workspace.path }}
                  </div>
                </div>
                <UIcon
                  v-if="selectedWorkspaceId === workspace.id"
                  name="i-lucide-check"
                  class="size-5 text-primary"
                />
              </div>
            </button>
          </div>
        </div>

        <UChatPrompt
          v-model="input"
          :status="loading ? 'streaming' : 'ready'"
          class="[view-transition-name:chat-prompt]"
          variant="subtle"
          :ui="{ base: 'px-1.5' }"
          :disabled="!selectedWorkspaceId || loadingWorkspaces"
          @submit="onSubmit"
        >
          <template #footer>
            <div class="flex items-center gap-1">
              <ModelSelect />
            </div>

            <div class="flex items-center gap-1">
              <UButton
                :icon="voice.isRecording.value ? 'i-lucide-square' : 'i-lucide-mic'"
                :color="voice.isRecording.value ? 'error' : 'neutral'"
                :loading="voice.isTranscribing.value"
                :disabled="!voice.isRecording.value && !canUseVoice"
                variant="ghost"
                size="sm"
                :aria-label="voice.isRecording.value ? 'Stop recording' : 'Start voice input'"
                @click="toggleVoiceInput"
              />

              <UChatPromptSubmit color="neutral" size="sm" :disabled="loading || !selectedWorkspaceId" />
            </div>
          </template>
        </UChatPrompt>

        <p
          v-if="!selectedWorkspaceId && !loadingWorkspaces && workspaces.length"
          class="text-xs text-muted"
        >
          Select a workspace above to start chatting.
        </p>
      </UContainer>
    </template>
  </UDashboardPanel>
</template>
