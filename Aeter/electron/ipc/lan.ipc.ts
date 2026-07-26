import { handle } from './handle'
import type { LanStatus, PairedDevice, PairingCode } from '@shared/types'
import { getLanServerState } from '../modules/lan/server'
import { closeLanSocketsForDevice } from '../modules/lan/ws'
import { createPairingClaim, listPairedDevices, revokeDevice } from '../modules/lan/pairingStore'
import { generateQrDataUrl } from '../modules/lan/qr'
import { needsHotspot, openHotspotSettings } from '../modules/lan/hotspot'
import { getSettings } from '../modules/settings'

export function registerLanIpc(): void {
  handle('getLanStatus', (): LanStatus => ({ ...getLanServerState(), needsHotspot: needsHotspot() }))

  handle('generatePairingCode', async (): Promise<PairingCode> => {
    const settings = getSettings()
    const { addresses } = getLanServerState()
    const host = addresses[0] ?? null
    if (!host) {
      return { qrDataUrl: null, expiresAt: null, host: null, port: settings.lanServerPort, error: 'no-lan-address' }
    }
    const claim = createPairingClaim()
    const qrDataUrl = await generateQrDataUrl({
      v: 1,
      host,
      port: settings.lanServerPort,
      pairingToken: claim.pairingToken
    })
    return { qrDataUrl, expiresAt: claim.expiresAt, host, port: settings.lanServerPort }
  })

  handle('getPairedDevices', (): PairedDevice[] => listPairedDevices())

  handle('revokeDevice', (_e, deviceId: string): void => {
    revokeDevice(deviceId)
    closeLanSocketsForDevice(deviceId)
  })

  handle('openHotspotSettings', (): Promise<void> => openHotspotSettings())
}
