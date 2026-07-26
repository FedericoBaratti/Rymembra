import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { WifiOff, RefreshCw, Loader2, HardDrive, X } from 'lucide-react'
import { toast } from '@/store/useToastStore'
import {
  isLanModeActive,
  isLanFallbackLocal,
  getLanConnectionState,
  onLanConnectionChange,
  retryLanConnection,
  retryLanFromFallback,
  type LanConnectionState
} from '@/lib/lanClient'

/**
 * Two related strips around losing the paired desktop:
 *
 * - LAN mode, connection dropped (red): the paired desktop stopped answering
 *   (PC asleep/closed, phone left the Wi-Fi). Without it every page just
 *   decays into skeletons/errors with no explanation. The reconnect loop keeps
 *   running on its own backoff; "retry" skips the wait and re-discovers
 *   immediately, "use local" reloads — the boot probe fails and the app comes
 *   back on the local backend, pairing intact.
 *
 * - Local fallback while paired (amber, dismissible): the boot probe already
 *   failed and the app started on the local backend (installLanBridge()).
 *   Everything works standalone; the strip just explains why LAN mode is off
 *   and offers a manual reconnect (manual on purpose: an auto-reload would cut
 *   off local playback mid-track).
 */
export default function LanOfflineBanner(): React.JSX.Element | null {
  const { t } = useTranslation()
  const [state, setState] = useState<LanConnectionState>(getLanConnectionState())
  const [busy, setBusy] = useState(false)
  const [dismissed, setDismissed] = useState(false)

  useEffect(() => onLanConnectionChange(setState), [])

  if (isLanFallbackLocal()) {
    if (dismissed) return null
    const reconnect = async (): Promise<void> => {
      setBusy(true)
      try {
        // On success this reloads the page; the "still offline" toast is the
        // only path that returns.
        if (!(await retryLanFromFallback())) toast.error(t('lan_fallback.still_offline'))
      } finally {
        setBusy(false)
      }
    }
    return (
      <div className="relative z-10 flex items-center gap-2.5 bg-[var(--warning-soft)] px-[var(--content-x)] py-2">
        <HardDrive size={14} className="shrink-0 text-amber-400" />
        <div className="min-w-0 flex-1">
          <div className="text-[12px] font-medium leading-tight">{t('lan_fallback.title')}</div>
          <div className="truncate text-[10.5px] leading-tight text-text-3">{t('lan_fallback.hint')}</div>
        </div>
        <button
          className="btn-ghost flex shrink-0 items-center gap-1 rounded-lg px-2.5 py-1.5 text-[11.5px] font-medium disabled:opacity-50"
          onClick={() => void reconnect()}
          disabled={busy}
        >
          {busy ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}
          {busy ? t('lan_offline.retrying') : t('lan_fallback.reconnect')}
        </button>
        <button
          className="btn-ghost shrink-0 rounded-lg p-1.5"
          aria-label={t('common.close')}
          onClick={() => setDismissed(true)}
        >
          <X size={13} />
        </button>
      </div>
    )
  }

  if (!isLanModeActive() || state !== 'offline') return null

  const retry = async (): Promise<void> => {
    setBusy(true)
    try {
      await retryLanConnection()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="relative z-10 flex items-center gap-2.5 bg-red-500/15 px-[var(--content-x)] py-2">
      <WifiOff size={14} className="shrink-0 text-[var(--danger)]" />
      <div className="min-w-0 flex-1">
        <div className="text-[12px] font-medium leading-tight">{t('lan_offline.title')}</div>
        <div className="truncate text-[10.5px] leading-tight text-text-3">{t('lan_offline.hint')}</div>
      </div>
      <button
        className="btn-ghost flex shrink-0 items-center gap-1 rounded-lg px-2.5 py-1.5 text-[11.5px] font-medium disabled:opacity-50"
        onClick={() => void retry()}
        disabled={busy}
      >
        {busy ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}
        {busy ? t('lan_offline.retrying') : t('lan_offline.retry')}
      </button>
      <button
        className="btn-ghost flex shrink-0 items-center gap-1 rounded-lg px-2.5 py-1.5 text-[11.5px] font-medium"
        onClick={() => window.location.reload()}
      >
        <HardDrive size={12} />
        {t('lan_offline.use_local')}
      </button>
    </div>
  )
}
