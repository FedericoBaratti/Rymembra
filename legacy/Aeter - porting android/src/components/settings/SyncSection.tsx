import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Cloud, Loader2, RefreshCw, Download } from 'lucide-react'
import type { SyncStatus, MissingFetchStatus } from '@shared/types'
import { Section, FieldRow, Switch, inputCls } from './controls'
import Select from '@/components/ui/Select'
import { useSettingsStore } from '@/store/useSettingsStore'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'

/** The four-colour Google "G", inlined so it needs no external asset (the CSP
 *  blocks remote images) and renders sharply at any size. */
function GoogleGIcon({ size = 16 }: { size?: number }): React.JSX.Element {
  return (
    <svg width={size} height={size} viewBox="0 0 48 48" aria-hidden="true">
      <path
        fill="#EA4335"
        d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"
      />
      <path
        fill="#4285F4"
        d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"
      />
      <path
        fill="#FBBC05"
        d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z"
      />
      <path
        fill="#34A853"
        d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"
      />
    </svg>
  )
}

/**
 * Google Drive library-sync block. Setup mirrors the Spotify integration: paste
 * the OAuth "Desktop app" client id + secret (from your Google Cloud project),
 * then use the "Sign in with Google" button to authorize the backup. Once
 * connected, toggle automatic sync or run a manual one. Live status arrives via
 * the 'sync:status' event.
 */
export default function SyncSection({ index }: { index?: number }): React.JSX.Element | null {
  const { t, i18n } = useTranslation()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const [status, setStatus] = useState<SyncStatus | null>(null)
  const [busy, setBusy] = useState<'connect' | 'sync' | 'disconnect' | null>(null)
  const [missing, setMissing] = useState<MissingFetchStatus | null>(null)
  const [retrying, setRetrying] = useState(false)

  useEffect(() => {
    void window.aether.driveSyncStatus().then(setStatus)
    return window.aether.on('sync:status', setStatus)
  }, [])

  useEffect(() => {
    const refresh = (): void => void window.aether.syncMissingStatus().then(setMissing)
    refresh()
    const off = window.aether.on('library:changed', refresh)
    const id = window.setInterval(refresh, 15_000)
    return () => {
      off()
      window.clearInterval(id)
    }
  }, [])

  if (!settings) return null

  const connected = status?.connected ?? false
  const syncing = status?.syncing ?? false
  // Configured = an OAuth client is available (bundled in source or entered in
  // the UI). When bundled, the credential fields are hidden and the button is
  // ready immediately; otherwise the user must enter the client id + secret.
  const configured = status?.configured ?? false
  const hasCreds = configured || (!!settings.googleClientId && !!settings.googleClientSecret)

  const connect = async (): Promise<void> => {
    setBusy('connect')
    try {
      const s = await window.aether.driveConnect()
      setStatus(s)
      if (s.connected) toast.success(t('sync.connected_toast', { email: s.email }))
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setBusy(null)
    }
  }

  const syncNow = async (): Promise<void> => {
    setBusy('sync')
    try {
      const s = await window.aether.driveSyncNow()
      setStatus(s)
      if (s.lastError) toast.error(t('sync.error', { message: s.lastError }))
      else toast.success(t('sync.synced_toast'))
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setBusy(null)
    }
  }

  const disconnect = async (): Promise<void> => {
    setBusy('disconnect')
    try {
      const s = await window.aether.driveDisconnect()
      setStatus(s)
      toast.success(t('sync.disconnected_toast'))
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setBusy(null)
    }
  }

  const lastWhen = status?.lastSyncAt
    ? new Date(status.lastSyncAt).toLocaleString(i18n.language)
    : t('sync.never')

  return (
    <Section title={t('sync.title')} icon={Cloud} index={index}>
      <p className="max-w-xl text-[11.5px] leading-relaxed text-text-3">{t('sync.subtitle')}</p>

      {!connected ? (
        <>
          {!configured && (
            <FieldRow label={t('sync.credentials_label')} hint={t('sync.setup_hint')}>
              <div className="flex flex-col gap-1.5">
                <input
                  className={inputCls}
                  placeholder={t('settings.client_id_ph')}
                  defaultValue={settings.googleClientId}
                  onBlur={(e) => void update({ googleClientId: e.target.value.trim() })}
                />
                <input
                  className={inputCls}
                  placeholder={t('settings.client_secret_ph')}
                  type="password"
                  defaultValue={settings.googleClientSecret}
                  onBlur={(e) => void update({ googleClientSecret: e.target.value.trim() })}
                />
              </div>
            </FieldRow>
          )}

          <FieldRow
            label={t('sync.sign_in_google')}
            hint={hasCreds ? undefined : t('sync.need_creds')}
          >
            <button
              className="flex items-center gap-2.5 rounded-lg bg-white px-4 py-2 text-[13px] font-medium text-[#3c4043] shadow-sm transition-colors hover:bg-[#f6f7f8] disabled:cursor-not-allowed disabled:opacity-50"
              onClick={() => void connect()}
              disabled={!hasCreds || busy !== null}
            >
              {busy === 'connect' ? (
                <Loader2 size={16} className="animate-spin text-[#5f6368]" />
              ) : (
                <GoogleGIcon size={16} />
              )}
              {busy === 'connect' ? t('sync.connecting') : t('sync.sign_in_google')}
            </button>
          </FieldRow>
        </>
      ) : (
        <>
          <FieldRow
            label={t('sync.connected_as', { email: status?.email ?? '' })}
            hint={t('sync.last_sync', { when: lastWhen })}
          >
            <button
              className="btn-ghost rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2"
              onClick={() => void disconnect()}
              disabled={busy !== null}
            >
              {t('sync.disconnect')}
            </button>
          </FieldRow>

          <FieldRow label={t('sync.auto_toggle')} hint={t('sync.auto_toggle_hint')}>
            <Switch
              checked={settings.driveSyncEnabled}
              label={t('sync.auto_toggle')}
              onChange={(c) => {
                void update({ driveSyncEnabled: c })
                setStatus((s) => (s ? { ...s, enabled: c } : s))
              }}
            />
          </FieldRow>

          <FieldRow label={t('sync.autofetch_toggle')} hint={t('sync.autofetch_hint')}>
            <Switch
              checked={settings.autoFetchMissing}
              label={t('sync.autofetch_toggle')}
              onChange={(c) => void update({ autoFetchMissing: c })}
            />
          </FieldRow>

          {settings.autoFetchMissing && (
            <FieldRow label={t('sync.autofetch_network')} hint={t('sync.autofetch_network_hint')}>
              <Select<typeof settings.autoFetchNetwork>
                title={t('sync.autofetch_network')}
                ariaLabel={t('sync.autofetch_network')}
                value={settings.autoFetchNetwork}
                options={[
                  { value: 'wifi', label: t('sync.autofetch_network_wifi') },
                  { value: 'any', label: t('sync.autofetch_network_any') }
                ]}
                onChange={(autoFetchNetwork) => void update({ autoFetchNetwork })}
              />
            </FieldRow>
          )}

          {settings.autoFetchMissing && missing && missing.total > 0 && (
            <FieldRow
              label={t('sync.autofetch_status', {
                pending: missing.pending,
                active: missing.active
              })}
              hint={missing.failed > 0 ? t('sync.autofetch_failed', { count: missing.failed }) : undefined}
            >
              <button
                className="btn-ghost flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2 disabled:opacity-50"
                onClick={() => {
                  setRetrying(true)
                  void window.aether
                    .syncRetryMissing()
                    .then(setMissing)
                    .finally(() => setRetrying(false))
                }}
                disabled={retrying || missing.failed === 0}
              >
                {retrying ? <Loader2 size={13} className="animate-spin" /> : <Download size={13} />}
                {t('sync.autofetch_retry')}
              </button>
            </FieldRow>
          )}

          <FieldRow label={t('sync.sync_now')}>
            <button
              className="btn-accent flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] disabled:opacity-50"
              onClick={() => void syncNow()}
              disabled={busy !== null || syncing}
            >
              {busy === 'sync' || syncing ? (
                <Loader2 size={13} className="animate-spin" />
              ) : (
                <RefreshCw size={13} />
              )}
              {busy === 'sync' || syncing ? t('sync.syncing') : t('sync.sync_now')}
            </button>
          </FieldRow>

          {status?.lastError && (
            <p className="text-[11.5px] leading-relaxed text-[var(--danger)]">
              {t('sync.error', { message: status.lastError })}
            </p>
          )}
        </>
      )}
    </Section>
  )
}
