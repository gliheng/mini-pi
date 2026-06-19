import PiRemoteSettings from '~/components/PiRemoteSettings.vue'

let modal: ReturnType<ReturnType<typeof useOverlay>['create']> | null = null

export function usePiRemoteSettingsModal() {
  const overlay = useOverlay()

  if (!modal) {
    modal = overlay.create(PiRemoteSettings)
  }

  async function open(blocking = false) {
    return await modal!.open({ blocking }).result
  }

  return {
    modal,
    open
  }
}
