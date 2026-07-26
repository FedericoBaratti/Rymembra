import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AlertTriangle, Loader2, RotateCcw } from 'lucide-react'
import { isBackendDown, onBackendHealthChange } from '@/lib/bridge'
import { FileAccessNative } from '@/lib/nativeRpc'
import { isMobile } from '@/lib/platform'

/**
 * Persistent strip shown when the nodejs-mobile backend has been unreachable
 * for over a minute (src/lib/bridge.ts isBackendDown). At that point the app is
 * de-facto bricked — every IPC call rejects, spinners multiply — and the engine
 * cannot be restarted in-process, so the only honest offer is a full app
 * restart (FileAccess.restartApp → RestartActivity trampoline).
 *
 * Not dismissible on purpose: unlike the LAN fallback, nothing works while the
 * backend is down, so hiding the strip would just hide the explanation. It
 * removes itself if the backend ever answers again (late reply/event).
 */
export default function BackendDownBanner(): React.JSX.Element | null {
  const { t } = useTranslation()
  const [down, setDown] = useState(isBackendDown())
  const [busy, setBusy] = useState(false)

  useEffect(() => onBackendHealthChange(setDown), [])

  if (!isMobile || !down) return null

  const restart = async (): Promise<void> => {
    setBusy(true)
    try {
      await FileAccessNative.restartApp()
      // The process dies within milliseconds of the ack; nothing to reset.
    } catch {
      setBusy(false)
    }
  }

  return (
    <div className="relative z-10 flex items-center gap-2.5 bg-red-500/15 px-[var(--content-x)] py-2">
      <AlertTriangle size={14} className="shrink-0 text-[var(--danger)]" />
      <div className="min-w-0 flex-1">
        <div className="text-[12px] font-medium leading-tight">{t('backend_down.title')}</div>
        <div className="truncate text-[10.5px] leading-tight text-text-3">
          {t('backend_down.hint')}
        </div>
      </div>
      <button
        className="btn-ghost flex shrink-0 items-center gap-1 rounded-lg px-2.5 py-1.5 text-[11.5px] font-medium disabled:opacity-50"
        onClick={() => void restart()}
        disabled={busy}
      >
        {busy ? <Loader2 size={12} className="animate-spin" /> : <RotateCcw size={12} />}
        {t('backend_down.restart')}
      </button>
    </div>
  )
}
