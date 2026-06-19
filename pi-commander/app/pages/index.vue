<script setup lang="ts">
const input = ref('')
const loading = ref(false)

const config = usePiRemoteConfig()
const remote = usePiRemote()
const { model } = useModels()
const settings = usePiRemoteSettingsModal()

async function openSettings() {
  await settings.open()
}

async function createChat(prompt: string) {
  if (!config.isConfigured.value) {
    await openSettings()
    if (!config.isConfigured.value) return
  }

  input.value = prompt
  loading.value = true

  try {
    const { thread_id } = await remote.createThread(model.value || undefined)
    const pendingPrompt = useState<string | null>(`pending-prompt-${thread_id}`, () => null)
    pendingPrompt.value = prompt
    await navigateTo(`/chat/${thread_id}`)
  } catch (err) {
    useToast().add({
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

const quickChats = [
  { label: 'Explain Rust lifetimes', icon: 'i-lucide-code' },
  { label: 'Refactor this function', icon: 'i-lucide-wrench' },
  { label: 'Plan my week', icon: 'i-lucide-calendar' },
  { label: 'Summarize this project', icon: 'i-lucide-folder' }
]
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

        <UChatPrompt
          v-model="input"
          :status="loading ? 'streaming' : 'ready'"
          class="[view-transition-name:chat-prompt]"
          variant="subtle"
          :ui="{ base: 'px-1.5' }"
          @submit="onSubmit"
        >
          <template #footer>
            <div class="flex items-center gap-1">
              <ModelSelect />
            </div>

            <UChatPromptSubmit color="neutral" size="sm" :disabled="loading" />
          </template>
        </UChatPrompt>

        <div class="flex flex-wrap gap-2">
          <UButton
            v-for="quickChat in quickChats"
            :key="quickChat.label"
            :icon="quickChat.icon"
            :label="quickChat.label"
            size="sm"
            color="neutral"
            variant="outline"
            class="rounded-full"
            @click="createChat(quickChat.label)"
          />
        </div>
      </UContainer>
    </template>
  </UDashboardPanel>
</template>
