import OpenAI from 'openai'

export interface VoiceInputState {
  isRecording: Ref<boolean>
  isTranscribing: Ref<boolean>
  error: Ref<string | null>
  supported: ComputedRef<boolean>
  startRecording: () => Promise<void>
  stopRecording: () => Promise<Blob>
  transcribe: (blob: Blob) => Promise<string>
}

export function useVoiceInput(): VoiceInputState {
  const { apiKey } = useOpenAIKey()

  const isRecording = ref(false)
  const isTranscribing = ref(false)
  const error = ref<string | null>(null)

  const supported = computed(() => {
    return typeof navigator !== 'undefined'
      && !!navigator.mediaDevices?.getUserMedia
      && typeof MediaRecorder !== 'undefined'
  })

  let mediaRecorder: MediaRecorder | null = null
  let recordedChunks: Blob[] = []
  let activeStream: MediaStream | null = null

  function getPreferredMimeType(): string | undefined {
    const candidates = [
      'audio/webm;codecs=opus',
      'audio/webm',
      'audio/mp4'
    ]
    return candidates.find(type => MediaRecorder.isTypeSupported(type))
  }

  async function startRecording(): Promise<void> {
    if (!supported.value) {
      throw new Error('Voice input is not supported in this browser.')
    }

    const key = apiKey.value.trim()
    if (!key) {
      throw new Error('OpenAI API key is not configured. Add it in Remote API settings.')
    }

    error.value = null
    recordedChunks = []

    try {
      activeStream = await navigator.mediaDevices.getUserMedia({ audio: true })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      if (message.includes('Permission denied') || message.includes('NotAllowedError')) {
        throw new Error('Microphone permission was denied.', { cause: err })
      }
      throw new Error(`Could not access microphone: ${message}`, { cause: err })
    }

    const mimeType = getPreferredMimeType()
    mediaRecorder = new MediaRecorder(activeStream, mimeType ? { mimeType } : undefined)

    mediaRecorder.ondataavailable = (event) => {
      if (event.data.size > 0) {
        recordedChunks.push(event.data)
      }
    }

    mediaRecorder.onerror = () => {
      error.value = 'Recording failed.'
      isRecording.value = false
      releaseStream()
    }

    mediaRecorder.start()
    isRecording.value = true
  }

  function stopRecording(): Promise<Blob> {
    return new Promise((resolve, reject) => {
      if (!mediaRecorder || !activeStream) {
        reject(new Error('No active recording.'))
        return
      }

      const recorder = mediaRecorder
      const stream = activeStream

      recorder.onstop = () => {
        const blob = new Blob(recordedChunks, {
          type: recorder.mimeType || 'audio/webm'
        })
        releaseStream()
        isRecording.value = false
        resolve(blob)
      }

      recorder.onerror = () => {
        releaseStream()
        isRecording.value = false
        reject(new Error('Recording failed.'))
      }

      recorder.stop()
      stream.getTracks().forEach(track => track.stop())
    })
  }

  function releaseStream() {
    if (activeStream) {
      activeStream.getTracks().forEach(track => track.stop())
      activeStream = null
    }
    mediaRecorder = null
  }

  async function transcribe(blob: Blob): Promise<string> {
    const key = apiKey.value.trim()
    if (!key) {
      throw new Error('OpenAI API key is not configured. Add it in Remote API settings.')
    }

    isTranscribing.value = true
    error.value = null

    try {
      const openai = new OpenAI({
        apiKey: key,
        dangerouslyAllowBrowser: true
      })

      const file = new File([blob], 'recording.webm', {
        type: blob.type || 'audio/webm'
      })

      const result = await openai.audio.transcriptions.create({
        file,
        model: 'whisper-1'
      })

      return result.text
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
    error,
    supported,
    startRecording,
    stopRecording,
    transcribe
  }
}
