import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { BarcodeScanner } from '@capacitor-mlkit/barcode-scanning'
import { Loader2, MonitorSmartphone, QrCode, Trash2 } from 'lucide-react'
import type { TransferState } from '@shared/types'
import { Section, FieldRow, Switch } from './controls'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'

/**
 * "Riparazione da PC": phone side of the WiFi repair feature. Enables the
 * authenticated LAN transfer server (node-backend/transfer/), pairs with a
 * desktop by scanning the QR its PhoneSync page shows (the mirror image of
 * PairDevice.tsx — here the DESKTOP shows the code), and lists/revokes the
 * paired desktops. Mobile-standalone only: in LAN thin-client mode there is
 * no on-device library to repair.
 */
export default function PhoneRepairSection({ index }: { index?: number }): React.JSX.Element {
  const { t } = useTranslation()
  const [state, setState] = useState<TransferState | null>(null)
  const [busy, setBusy] = useState(false)
  const [scanning, setScanning] = useState(false)

  useEffect(() => {
    let mounted = true
    void window.aether
      .transferGetState()
      .then((s) => {
        if (mounted) setState(s)
      })
      .catch(() => {})
    const off = window.aether.on('transfer:state', setState)
    return () => {
      mounted = false
      off()
    }
  }, [])

  const setEnabled = async (enabled: boolean): Promise<void> => {
    setBusy(true)
    try {
      setState(await window.aether.transferSetEnabled(enabled))
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  const scan = async (): Promise<void> => {
    setScanning(true)
    try {
      const { supported } = await BarcodeScanner.isSupported()
      if (!supported) {
        toast.error(t('pair_device.not_supported'))
        return
      }
      const { camera } = await BarcodeScanner.requestPermissions()
      if (camera !== 'granted' && camera !== 'limited') {
        toast.error(t('pair_device.camera_denied'))
        return
      }
      const { barcodes } = await BarcodeScanner.scan()
      const raw = barcodes[0]?.rawValue
      if (!raw) return
      const next = await window.aether.transferPairWithQr(raw)
      setState(next)
      toast.success(t('phone_repair.paired_toast'))
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      if (msg.includes('PAIR_BAD_QR')) toast.error(t('pair_device.invalid_qr'))
      else if (msg.includes('PAIR_UNREACHABLE')) toast.error(t('phone_repair.unreachable'))
      else toast.error(ipcErrorMessage(err))
    } finally {
      setScanning(false)
    }
  }

  const revoke = async (peerId: string): Promise<void> => {
    try {
      setState(await window.aether.transferRevokePeer(peerId))
      toast.success(t('phone_repair.revoked_toast'))
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    }
  }

  return (
    <Section title={t('phone_repair.section_title')} icon={MonitorSmartphone} index={index}>
      <p className="max-w-xl text-[11.5px] leading-relaxed text-text-3">
        {t('phone_repair.section_subtitle')}
      </p>

      <FieldRow
        label={t('phone_repair.enable_label')}
        hint={
          state?.running && state.port !== null
            ? t('phone_repair.running_hint', { port: state.port })
            : t('phone_repair.enable_hint')
        }
      >
        <Switch
          checked={state?.enabled ?? false}
          onChange={(v) => void setEnabled(v)}
          label={t('phone_repair.enable_label')}
        />
      </FieldRow>

      {state?.sessionActive && (
        <div className="flex items-center gap-2 rounded-lg bg-white/5 px-3 py-2 text-[12.5px]">
          <Loader2 size={14} className="animate-spin text-[var(--accent)]" />
          {t('phone_repair.session_active')}
        </div>
      )}

      <FieldRow label={t('phone_repair.pair_label')} hint={t('phone_repair.pair_hint')}>
        <button
          className="btn-accent flex items-center gap-2 rounded-lg px-3 py-2 text-[12.5px] disabled:opacity-50"
          onClick={() => void scan()}
          disabled={scanning || busy}
        >
          {scanning ? <Loader2 size={14} className="animate-spin" /> : <QrCode size={14} />}
          {scanning ? t('pair_device.scanning') : t('pair_device.scan_button')}
        </button>
      </FieldRow>

      {state && state.peers.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="text-[11.5px] font-medium uppercase tracking-wide text-text-3">
            {t('phone_repair.peers_title')}
          </div>
          {state.peers.map((peer) => (
            <div
              key={peer.peerId}
              className="flex items-center justify-between gap-3 rounded-lg bg-white/5 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="truncate text-[13px] font-medium">{peer.name}</div>
                <div className="text-[11px] text-text-3">
                  {t('phone_repair.paired_at', {
                    date: new Date(peer.pairedAt).toLocaleDateString()
                  })}
                </div>
              </div>
              <button
                className="btn-ghost rounded-lg p-2 text-text-3"
                aria-label={t('phone_repair.revoke')}
                onClick={() => void revoke(peer.peerId)}
              >
                <Trash2 size={14} />
              </button>
            </div>
          ))}
        </div>
      )}
    </Section>
  )
}
