<script setup lang="ts">
import QrScanner from 'qr-scanner'

const emit = defineEmits<{
  scan: [value: string]
  error: [message: string]
  cancel: []
}>()

const videoRef = ref<HTMLVideoElement | null>(null)
let scanner: QrScanner | null = null
const starting = ref(true)
const hasCamera = ref(true)

onMounted(async () => {
  if (!videoRef.value) return

  try {
    const devices = await QrScanner.listCameras(true)
    if (!devices.length) {
      hasCamera.value = false
      starting.value = false
      emit('error', 'No camera found on this device.')
      return
    }

    scanner = new QrScanner(
      videoRef.value,
      (result) => {
        emit('scan', result.data)
      },
      {
        onDecodeError: () => {
          // Frame didn't contain a QR code; ignore and keep scanning.
        },
        highlightScanRegion: true,
        highlightCodeOutline: true
      }
    )

    await scanner.start()
    starting.value = false
  } catch (err) {
    starting.value = false
    hasCamera.value = false
    emit('error', err instanceof Error ? err.message : String(err))
  }
})

onUnmounted(() => {
  scanner?.stop()
  scanner?.destroy()
})

function stop() {
  scanner?.stop()
  emit('cancel')
}
</script>

<template>
  <div class="flex flex-col gap-4">
    <div class="relative aspect-square max-h-72 overflow-hidden rounded-lg bg-black">
      <video
        ref="videoRef"
        class="h-full w-full object-cover"
        autoplay
        muted
        playsinline
      />

      <div
        v-if="starting"
        class="absolute inset-0 flex items-center justify-center bg-black/60 text-white"
      >
        <UIcon name="i-lucide-loader-2" class="animate-spin size-8" />
      </div>

      <div
        v-if="!hasCamera && !starting"
        class="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-black/80 p-6 text-center text-white"
      >
        <UIcon name="i-lucide-camera-off" class="size-8" />
        <p>Camera access is required to scan QR codes.</p>
      </div>
    </div>

    <UButton
      color="neutral"
      variant="outline"
      label="Cancel"
      block
      @click="stop"
    />
  </div>
</template>
