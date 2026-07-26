import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Smartphone, QrCode, Wifi, Loader2, X } from 'lucide-react'
import type { LanStatus, PairedDevice, PairingCode } from '@shared/types'
import { Section, FieldRow, Switch } from './controls'
import { useSettingsStore } from '@/store/useSettingsStore'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'

/**
 * "Connetti telefono" — the desktop side of the LAN thin-client pairing flow.
 * The phone streams/browses/searches the desktop library directly over the
 * local network (or the Windows hotspot as a fallback); nothing here talks to
 * the cloud (contrast with SyncSection's Google Drive backup).
 */
export default function RemoteAccessSection({ index }: { index?: number }): React.JSX.Element | null {
  const { t, i18n } = useTranslation()
  const navigate = useNavigate()
  const settings = useSettingsStore((s) => s.settings)
  const update = useSettingsStore((s) => s.update)
  const [status, setStatus] = useState<LanStatus | null>(null)
  const [devices, setDevices] = useState<PairedDevice[]>([])
  const [pairing, setPairing] = useState<PairingCode | null>(null)
  const [generating, setGenerating] = useState(false)

  const refresh = (): void => {
    void window.aether.getLanStatus().then(setStatus)
    void window.aether.getPairedDevices().then(setDevices)
  }

  useEffect(() => {
    if (!settings?.lanServerEnabled) return
    refresh()
    const id = window.setInterval(refresh, 10_000)
    return () => window.clearInterval(id)
  }, [settings?.lanServerEnabled])

  if (!settings) return null

  const generateCode = async (): Promise<void> => {
    setGenerating(true)
    try {
      const code = await window.aether.generatePairingCode()
      setPairing(code)
      if (code.error) toast.error(t('remote_access.no_lan_address'))
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setGenerating(false)
    }
  }

  const revoke = async (deviceId: string): Promise<void> => {
    try {
      await window.aether.revokeDevice(deviceId)
      setDevices((prev) => prev.filter((d) => d.deviceId !== deviceId))
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    }
  }

  const openHotspot = async (): Promise<void> => {
    try {
      await window.aether.openHotspotSettings()
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    }
  }

  return (
    <Section title={t('remote_access.title')} icon={Smartphone} index={index}>
      <p className="max-w-xl text-[11.5px] leading-relaxed text-text-3">{t('remote_access.subtitle')}</p>

      <FieldRow label={t('remote_access.enable_toggle')} hint={t('remote_access.enable_hint')}>
        <Switch
          checked={settings.lanServerEnabled}
          label={t('remote_access.enable_toggle')}
          onChange={(c) => {
            void update({ lanServerEnabled: c })
            if (!c) setPairing(null)
          }}
        />
      </FieldRow>

      {settings.lanServerEnabled && (
        <>
          <FieldRow
            label={
              status?.lastError
                ? t('remote_access.server_error', { error: status.lastError })
                : status?.running
                  ? t('remote_access.running', { addresses: status.addresses.join(', ') || '—', port: status.port })
                  : t('remote_access.starting')
            }
            hint={
              status?.lastError
                ? t('remote_access.server_error_hint')
                : status?.advertising
                  ? t('remote_access.advertising')
                  : undefined
            }
          >
            <Wifi
              size={16}
              className={status?.lastError ? 'text-[var(--danger)]' : status?.running ? 'text-[var(--success)]' : 'text-text-3'}
            />
          </FieldRow>

          <FieldRow label={t('remote_access.pair_new')} hint={t('remote_access.pair_hint')}>
            <button
              className="btn-accent flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] disabled:opacity-50"
              onClick={() => void generateCode()}
              disabled={generating}
            >
              {generating ? <Loader2 size={13} className="animate-spin" /> : <QrCode size={13} />}
              {t('remote_access.generate_qr')}
            </button>
          </FieldRow>

          {pairing?.qrDataUrl && (
            <div className="flex flex-col items-center gap-2 rounded-lg bg-white/5 p-4">
              <img src={pairing.qrDataUrl} alt={t('remote_access.generate_qr')} width={200} height={200} />
              <p className="text-[11px] text-text-3">
                {t('remote_access.qr_hint', { host: pairing.host, port: pairing.port })}
              </p>
            </div>
          )}

          {status?.needsHotspot && (
            <FieldRow label={t('remote_access.no_lan')} hint={t('remote_access.hotspot_hint')}>
              <button
                className="btn-ghost rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2"
                onClick={() => void openHotspot()}
              >
                {t('remote_access.open_hotspot_settings')}
              </button>
            </FieldRow>
          )}

          {devices.length > 0 && (
            <div className="flex flex-col gap-2">
              <div className="text-[13.5px] font-medium">{t('remote_access.paired_devices')}</div>
              {devices.map((d) => (
                <div
                  key={d.deviceId}
                  className="flex items-center justify-between gap-4 rounded-lg bg-white/5 px-3 py-2"
                >
                  <div>
                    <div className="text-[12.5px] font-medium">{d.deviceName}</div>
                    <div className="text-[11px] text-text-3">
                      {t('remote_access.last_seen', {
                        when: new Date(d.lastSeenAt).toLocaleString(i18n.language)
                      })}
                    </div>
                  </div>
                  <button
                    className="btn-ghost flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-[11.5px] text-text-2"
                    onClick={() => void revoke(d.deviceId)}
                  >
                    <X size={12} />
                    {t('remote_access.revoke')}
                  </button>
                </div>
              ))}
            </div>
          )}
        </>
      )}

      <FieldRow label={t('phone_sync.shortcut_label')} hint={t('phone_sync.shortcut_hint')}>
        <button
          className="btn-ghost rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2"
          onClick={() => navigate('/phone')}
        >
          {t('phone_sync.shortcut_button')}
        </button>
      </FieldRow>
    </Section>
  )
}
