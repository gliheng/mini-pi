import { usePiRemote, type PiModel } from './usePiRemote'

export interface ThinkingLevelItem {
  label: string
  value: string
}

export const DEFAULT_THINKING_LEVELS: ThinkingLevelItem[] = [
  { label: 'Off', value: 'off' },
  { label: 'Minimal', value: 'minimal' },
  { label: 'Low', value: 'low' },
  { label: 'Medium', value: 'medium' },
  { label: 'High', value: 'high' },
  { label: 'Extra High', value: 'xhigh' }
]

export function thinkingLevelLabel(level: string | null | undefined): string {
  if (!level) return 'Default'
  const item = DEFAULT_THINKING_LEVELS.find(l => l.value === level)
  return item?.label ?? level
}

export function thinkingLevelItemsForModel(
  model: PiModel | null | undefined
): ThinkingLevelItem[] {
  const map = model?.thinking_level_map
  if (!map) return DEFAULT_THINKING_LEVELS

  return DEFAULT_THINKING_LEVELS.filter((item) => {
    const mapped = map[item.value]
    // A value of `null` in the map means the level is unsupported.
    return mapped !== null
  })
}

export function useThinkingLevel() {
  const remote = usePiRemote()
  const { rawModels, model } = useModels()

  const selectedModel = computed<PiModel | undefined>(() =>
    rawModels.value.find(m => `${m.provider}:${m.id}` === model.value)
  )

  const items = computed<ThinkingLevelItem[]>(() =>
    thinkingLevelItemsForModel(selectedModel.value)
  )

  const level = useCookie<string>('thinking-level', { default: () => '' })

  // Fall back to a valid level when the cookie is missing, stale, or incompatible
  // with the current model's thinking level map.
  watchEffect(() => {
    const available = items.value
    if (available.length === 0) return
    const current = level.value
    const first = available[0]
    if (first && (!current || !available.some(l => l.value === current))) {
      level.value = first.value
    }
  })

  async function setLevel(threadId: string, value: string) {
    level.value = value
    await remote.setThinkingLevel(threadId, value)
  }

  return {
    level,
    items,
    setLevel
  }
}
