import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ExternalLink, Loader2, Radio } from 'lucide-react'
import { useSettingsStore } from '@/store/useSettingsStore'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'

/**
 * Last.fm scrobbling block inside the external-services section: shared
 * secret input, the two-step desktop authorize flow and the on/off toggle.
 */
export default function ScrobbleSettings(): React.JSX.Element | null {
  const { t } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const [authStep, setAuthStep] = useState<'idle' | 'authorizing'>('idle')
  const [busy, setBusy] = useState(false)
  const [queued, setQueued] = useState(0)

  const connected = !!settings?.lastfmSessionKey

  useEffect(() => {
    if (!connected) return
    void window.aether.getScrobbleStatus().then((s) => setQueued(s.queued))
  }, [connected])

  if (!settings) return null
  const hasCreds = !!settings.lastfmApiKey && !!settings.lastfmApiSecret

  const startAuth = async (): Promise<void> => {
    setBusy(true)
    try {
      await window.aether.lastfmStartAuth()
      setAuthStep('authorizing')
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  const completeAuth = async (): Promise<void> => {
    setBusy(true)
    try {
      const { username } = await window.aether.lastfmCompleteAuth()
      setAuthStep('idle')
      toast.success(t('scrobble.connected_as', { username }))
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  const disconnect = async (): Promise<void> => {
    await window.aether.lastfmDisconnect()
    setAuthStep('idle')
  }

  return (
    <>
      <div className="flex items-center justify-between gap-6">
        <div>
          <div className="text-[13.5px] font-medium">{t('scrobble.shared_secret_label')}</div>
          <div className="mt-0.5 max-w-md text-[11.5px] leading-relaxed text-text-3">
            {t('scrobble.secret_hint')}
          </div>
        </div>
        <div className="shrink-0">
          <input
            className="field-input h-9"
            placeholder={t('scrobble.shared_secret_ph')}
            type="password"
            defaultValue={settings.lastfmApiSecret}
            onBlur={(e) => void update({ lastfmApiSecret: e.target.value.trim() })}
          />
        </div>
      </div>

      <div className="flex items-center justify-between gap-6">
        <div>
          <div className="flex items-center gap-2 text-[13.5px] font-medium">
            <Radio size={14} className={connected ? 'text-[var(--success)]' : 'text-text-3'} />
            {t('scrobble.title')}
          </div>
          <div className="mt-0.5 max-w-md text-[11.5px] leading-relaxed text-text-3">
            {connected
              ? t('scrobble.connected_as', { username: settings.lastfmUsername }) +
                (queued > 0 ? ` · ${t('scrobble.queued', { count: queued })}` : '')
              : hasCreds
                ? t('scrobble.disconnected_hint')
                : t('scrobble.needs_creds')}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {connected ? (
            <>
              <label className="switch" title={t('scrobble.toggle')}>
                <input
                  type="checkbox"
                  checked={settings.scrobblingEnabled}
                  aria-label={t('scrobble.toggle')}
                  onChange={(e) => void update({ scrobblingEnabled: e.target.checked })}
                />
                <span className="switch-track" />
              </label>
              <button
                className="btn-ghost rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2"
                onClick={() => void disconnect()}
              >
                {t('scrobble.disconnect')}
              </button>
            </>
          ) : authStep === 'authorizing' ? (
            <button
              className="btn-accent flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px]"
              onClick={() => void completeAuth()}
              disabled={busy}
            >
              {busy && <Loader2 size={13} className="animate-spin" />}
              {t('scrobble.complete_auth')}
            </button>
          ) : (
            <button
              className="btn-accent flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] disabled:opacity-50"
              onClick={() => void startAuth()}
              disabled={busy || !hasCreds}
            >
              {busy ? <Loader2 size={13} className="animate-spin" /> : <ExternalLink size={13} />}
              {t('scrobble.connect')}
            </button>
          )}
        </div>
      </div>
    </>
  )
}
