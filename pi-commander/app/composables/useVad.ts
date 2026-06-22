import { MicVAD } from '@ricky0123/vad-web'
import ortWasmMjsUrl from 'onnxruntime-web/ort-wasm-simd-threaded.mjs?url'
import ortWasmUrl from 'onnxruntime-web/ort-wasm-simd-threaded.wasm?url'

export interface UseVadOptions {
  onSpeechStart?: () => void
  onSpeechEnd?: (audio: Float32Array) => void
}

let vad: MicVAD | null = null
let options: UseVadOptions = {}
let isInitializing = false
let initPromise: Promise<void> | null = null

export function useVad() {
  const isSpeaking = ref(false)
  const isReady = ref(false)
  const error = ref<string | null>(null)

  async function init(vadOptions: UseVadOptions = {}) {
    options = vadOptions

    if (vad) {
      isReady.value = true
      return
    }

    if (isInitializing) {
      await initPromise
      return
    }

    isInitializing = true
    error.value = null

    initPromise = (async () => {
      try {
        vad = await MicVAD.new({
          baseAssetPath: '/vendor/vad/',
          onnxWASMBasePath: { mjs: ortWasmMjsUrl, wasm: ortWasmUrl } as unknown as string,
          submitUserSpeechOnPause: true,
          startOnLoad: false,
          onSpeechStart: () => {
            isSpeaking.value = true
            options.onSpeechStart?.()
          },
          onSpeechEnd: (audio: Float32Array) => {
            isSpeaking.value = false
            options.onSpeechEnd?.(audio)
          }
        })
        isReady.value = true
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        error.value = message
        throw new Error(`Failed to initialize voice activity detection: ${message}`, { cause: err })
      } finally {
        isInitializing = false
      }
    })()

    await initPromise
  }

  async function start(vadOptions: UseVadOptions = {}) {
    await init(vadOptions)
    if (!vad) {
      throw new Error('VAD is not initialized.')
    }
    await vad.start()
  }

  async function pause() {
    if (!vad) return
    await vad.pause()
  }

  async function destroy() {
    if (!vad) return
    try {
      await vad.destroy()
    } catch {
      // ignore
    }
    vad = null
    isReady.value = false
  }

  return {
    isSpeaking,
    isReady,
    error,
    init,
    start,
    pause,
    destroy
  }
}
