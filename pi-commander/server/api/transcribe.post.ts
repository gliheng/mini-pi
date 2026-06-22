import OpenAI from 'openai'

const MODEL = 'custom-xiaomi/mimo-v2.5-asr'

function parseDataUrl(dataUrl: string): { mimeType: string, base64: string, format: string } {
  const splitIndex = dataUrl.indexOf(';base64,')
  if (!dataUrl.startsWith('data:') || splitIndex === -1) {
    throw createError({
      statusCode: 400,
      statusMessage: 'Invalid dataUrl format.'
    })
  }

  const mimeType = dataUrl.slice('data:'.length, splitIndex)
  const base64 = dataUrl.slice(splitIndex + ';base64,'.length)

  if (!mimeType || !base64) {
    throw createError({
      statusCode: 400,
      statusMessage: 'Invalid dataUrl: missing MIME type or base64 data.'
    })
  }

  let format = 'wav'
  if (mimeType.includes('webm')) {
    format = 'webm'
  } else if (mimeType.includes('mp4') || mimeType.includes('m4a') || mimeType.includes('mp3')) {
    format = 'mp3'
  } else if (mimeType.includes('wav')) {
    format = 'wav'
  }

  return { mimeType, base64, format }
}

export default defineEventHandler(async (event) => {
  const body = await readBody(event)
  const dataUrl = body?.dataUrl

  if (typeof dataUrl !== 'string' || !dataUrl.startsWith('data:')) {
    throw createError({
      statusCode: 400,
      statusMessage: 'Invalid or missing dataUrl.'
    })
  }

  const CLOUDFLARE_API_KEY = process.env.CLOUDFLARE_API_KEY
  const CLOUDFLARE_ACCOUNT_ID = process.env.CLOUDFLARE_ACCOUNT_ID
  const CLOUDFLARE_GATEWAY_ID = process.env.CLOUDFLARE_GATEWAY_ID

  if (!CLOUDFLARE_API_KEY || !CLOUDFLARE_ACCOUNT_ID || !CLOUDFLARE_GATEWAY_ID) {
    throw createError({
      statusCode: 500,
      statusMessage: 'Missing Cloudflare gateway configuration.'
    })
  }

  // const { base64, format } = parseDataUrl(dataUrl)

  const openai = new OpenAI({
    apiKey: CLOUDFLARE_API_KEY,
    baseURL: `https://gateway.ai.cloudflare.com/v1/${CLOUDFLARE_ACCOUNT_ID}/${CLOUDFLARE_GATEWAY_ID}/compat`
  })

  try {
    const result = await openai.chat.completions.create({
      model: MODEL,
      messages: [
        {
          role: 'user',
          content: [
            {
              type: 'input_audio',
              input_audio: {
                data: dataUrl
              }
            }
          ]
        }
      ],
      asr_options: {
        language: 'auto'
      },
      stream: false
    } as any)

    const content = result.choices[0]?.message?.content
    if (typeof content !== 'string' || !content.trim()) {
      throw createError({
        statusCode: 502,
        statusMessage: 'Transcription returned empty content.'
      })
    }

    return { text: content.trim() }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    throw createError({
      statusCode: 502,
      statusMessage: `Transcription failed: ${message}`
    })
  }
})
