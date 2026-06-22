import { concatFloat32Arrays, float32ToWavBlob } from '~/utils/audio'

export interface VoiceInputState {
  isRecording: Ref<boolean>
  isTranscribing: Ref<boolean>
  isSpeaking: Ref<boolean>
  error: Ref<string | null>
  supported: boolean
  startRecording: () => Promise<void>
  stopRecording: () => Promise<Blob>
  transcribe: (blob: Blob) => Promise<string>
}

export function useVoiceInput(): VoiceInputState {
  const isRecording = ref(false)
  const isTranscribing = ref(false)
  const isSpeaking = ref(false)
  const error = ref<string | null>(null)

  const vad = useVad()

  // Change later
  const supported = true

  let voiceSegments: Float32Array[] = []

  async function startRecording(): Promise<void> {
    if (!supported) {
      throw new Error('Voice input is not supported in this browser.')
    }

    error.value = null
    voiceSegments = []

    try {
      isRecording.value = true
      isSpeaking.value = false

      await vad.start({
        onSpeechStart: () => {
          isSpeaking.value = true
        },
        onSpeechEnd: (audio: Float32Array) => {
          isSpeaking.value = false
          voiceSegments.push(audio)
        }
      })
    } catch (err) {
      isRecording.value = false
      isSpeaking.value = false
      const message = err instanceof Error ? err.message : String(err)
      if (message.includes('Permission denied') || message.includes('NotAllowedError')) {
        throw new Error('Microphone permission was denied.', { cause: err })
      }
      throw new Error(`Could not start voice recorder: ${message}`, { cause: err })
    }
  }

  function stopRecording(): Promise<Blob> {
    return new Promise((resolve, reject) => {
      if (!isRecording.value) {
        reject(new Error('No active recording.'))
        return
      }

      vad.pause()
        .then(() => {
          const combined = concatFloat32Arrays(voiceSegments)
          const blob = float32ToWavBlob(combined)
          voiceSegments = []
          isRecording.value = false
          isSpeaking.value = false
          resolve(blob)
        })
        .catch((err) => {
          voiceSegments = []
          isRecording.value = false
          isSpeaking.value = false
          reject(err)
        })
    })
  }

  async function blobToDataUrl(blob: Blob): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader()

      reader.onloadend = () => {
        if (typeof reader.result === 'string') {
          resolve(reader.result)
        } else {
          reject(new Error('Failed to read audio blob as data URL.'))
        }
      }

      reader.onerror = () => reject(reader.error || new Error('FileReader error'))
      reader.readAsDataURL(blob)
    })
  }

  async function transcribe(blob: Blob): Promise<string> {
    isTranscribing.value = true
    error.value = null

    try {
      const dataUrl = await blobToDataUrl(blob)

      const response = await $fetch('/api/transcribe', {
        method: 'POST',
        body: { dataUrl }
      })

      if (!response || typeof response.text !== 'string') {
        throw new Error('Invalid response from transcription server.')
      }

      return response.text
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      error.value = message
      throw new Error(`Transcription failed: ${message}`, { cause: err })
    } finally {
      isTranscribing.value = false
    }
  }

  return {
    isRecording,
    isTranscribing,
    isSpeaking,
    error,
    supported,
    startRecording,
    stopRecording,
    transcribe
  }
}
