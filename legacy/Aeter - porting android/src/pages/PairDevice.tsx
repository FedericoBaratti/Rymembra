import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { BarcodeScanner } from '@capacitor-mlkit/barcode-scanning'
import { QrCode, Wifi, Loader2, ArrowLeft } from 'lucide-react'
import PageHeader from '@/components/ui/PageHeader'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { getStoredPairing, claimPairing, clearPairing, type PairingQrPayload } from '@/lib/lanClient'

/**
 * Pairing flow for LAN thin-client mode: scan the QR the desktop shows in its
 * "Connetti telefono" settings, POST it to the desktop's /api/pair, and store
 * the returned device token. Reachable from a normal Settings entry point;
 * doesn't require an existing pairing (or even window.aether) to render —
 * this screen IS how one gets created.
 */
export default function PairDevice(): React.JSX.Element {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [host, setHost] = useState<string | null>(null)
  const [scanning, setScanning] = useState(false)

  useEffect(() => {
    void getStoredPairing().then((p) => setHost(p?.host ?? null))
  }, [])

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
      let payload: PairingQrPayload
      try {
        payload = JSON.parse(raw) as PairingQrPayload
        if (payload.v !== 1 || !payload.host || !payload.port || !payload.pairingToken) {
          throw new Error('invalid payload')
        }
      } catch {
        toast.error(t('pair_device.invalid_qr'))
        return
      }
      await claimPairing(payload, 'Android')
      setHost(payload.host)
      toast.success(t('pair_device.paired_toast', { host: payload.host }))
      setTimeout(() => window.location.reload(), 1200)
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setScanning(false)
    }
  }

  const forget = async (): Promise<void> => {
    await clearPairing()
    setHost(null)
    toast.success(t('pair_device.forgotten_toast'))
    setTimeout(() => window.location.reload(), 800)
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t('pair_device.title')}
        actions={
          <button className="btn-ghost flex items-center gap-1 rounded-lg px-2.5 py-1.5" onClick={() => navigate(-1)}>
            <ArrowLeft size={14} />
            {t('common.back')}
          </button>
        }
      />
      <div className="flex min-h-0 flex-1 flex-col items-center gap-5 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)] pt-6 text-center">
        <p className="max-w-xs text-[12.5px] leading-relaxed text-text-3">{t('pair_device.subtitle')}</p>

        {host ? (
          <>
            <div className="flex items-center gap-2 rounded-lg bg-white/5 px-4 py-3">
              <Wifi size={16} className="text-[var(--success)]" />
              <span className="text-[13px] font-medium">{t('pair_device.connected_to', { host })}</span>
            </div>
            <button className="btn-ghost rounded-lg px-4 py-2 text-[12.5px] font-medium text-text-2" onClick={() => void forget()}>
              {t('pair_device.forget')}
            </button>
          </>
        ) : (
          <button
            className="btn-accent flex items-center gap-2 rounded-lg px-5 py-3 text-[13px] disabled:opacity-50"
            onClick={() => void scan()}
            disabled={scanning}
          >
            {scanning ? <Loader2 size={16} className="animate-spin" /> : <QrCode size={16} />}
            {scanning ? t('pair_device.scanning') : t('pair_device.scan_button')}
          </button>
        )}
      </div>
    </div>
  )
}
