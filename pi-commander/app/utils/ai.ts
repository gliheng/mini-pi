import type { UIMessage } from 'ai'
import { isTextUIPart } from 'ai'

function isStreamingState(state: string | undefined): boolean {
  return state === 'streaming'
}

export function getMergedParts(parts: UIMessage['parts']): UIMessage['parts'] {
  const result: UIMessage['parts'] = []
  for (const part of parts) {
    const prev = result[result.length - 1]
    if (part.type === 'source-url') {
      if (prev && isTextUIPart(prev)) {
        const mergedText = prev.text + sourceToInlineMdc(part.url)
        const state = isStreamingState(prev.state) ? 'streaming' : undefined
        result[result.length - 1] = { type: 'text', text: mergedText, state }
      }
      continue
    }
    if (isTextUIPart(part) && prev && isTextUIPart(prev)) {
      const mergedText = prev.text + part.text
      const state = isStreamingState(prev.state) || isStreamingState(part.state) ? 'streaming' : undefined
      result[result.length - 1] = { type: 'text', text: mergedText, state }
    } else {
      result.push(part)
    }
  }
  return result
}
