import type { TransferState } from '@shared/types'
import { handle } from '../../electron/ipc/handle'
import { getSettings, setSettings } from '../../electron/modules/settings'
import { logWarn } from '../../electron/modules/logger'
import { mintDesktopToken, revokePeer } from './peers'
import {
  broadcastTransferState,
  getTransferState,
  startTransferServer,
  stopTransferServer
} from './server'

/**
 * Renderer-facing surface of the phone transfer server ("Riparazione da PC"
 * in the mobile settings): enable/disable toggle, QR pairing completion and
 * peer management. Phone-only — the desktop never registers these handlers.
 *
 * Pairing (roles inverted vs the LAN thin-client flow): the DESKTOP shows a QR
 * with a one-shot callback listener; the phone scans it, mints the desktop's
 * bearer token (peers.ts) and POSTs it back to that callback. The QR payload is
 * `{v:1, kind:'aether-phone-pair', host, port, pairingToken, name}`.
 */

interface PairPayload {
  v: number
  kind: string
  host: string
  port: number
  pairingToken: string
  /** Desktop's self-reported display name (shown in the peer list). */
  name?: string
}

function parsePairQr(qrText: string): PairPayload {
  let parsed: unknown
  try {
    parsed = JSON.parse(qrText)
  } catch {
    throw new Error('PAIR_BAD_QR')
  }
  const p = parsed as Partial<PairPayload> | null
  if (
    !p ||
    p.v !== 1 ||
    p.kind !== 'aether-phone-pair' ||
    typeof p.host !== 'string' ||
    !p.host ||
    typeof p.port !== 'number' ||
    !Number.isInteger(p.port) ||
    p.port <= 0 ||
    p.port > 65535 ||
    typeof p.pairingToken !== 'string' ||
    !/^[0-9a-f]{64}$/.test(p.pairingToken)
  ) {
    throw new Error('PAIR_BAD_QR')
  }
  return p as PairPayload
}

async function pairWithQr(qrText: string): Promise<TransferState> {
  const payload = parsePairQr(qrText)

  // Pairing implies the feature: persist the enable so the server comes back
  // after an app restart, then make sure it is actually listening now.
  if (!getSettings().transferServerEnabled) setSettings({ transferServerEnabled: true })
  const transferPort = await startTransferServer()

  const { peerId, token } = mintDesktopToken(payload.name ?? 'Desktop')
  const state = getTransferState()
  try {
    const res = await fetch(`http://${payload.host}:${payload.port}/pair`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        pairingToken: payload.pairingToken,
        deviceToken: token,
        deviceId: state.deviceId,
        deviceName: state.deviceName,
        transferPort
      }),
      signal: AbortSignal.timeout(10_000)
    })
    if (!res.ok) throw new Error(`PAIR_CALLBACK_${res.status}`)
  } catch (err) {
    // The desktop never received (or refused) the token: a peer entry without a
    // working counterpart is just confusing — roll it back.
    revokePeer(peerId)
    logWarn('transfer', 'Pairing callback verso il desktop fallito', err)
    throw err instanceof Error && err.message.startsWith('PAIR_')
      ? err
      : new Error('PAIR_UNREACHABLE')
  }

  broadcastTransferState()
  return getTransferState()
}

export function registerTransferIpc(): void {
  handle('transferGetState', (): TransferState => getTransferState())

  handle('transferSetEnabled', async (_e, enabled: boolean): Promise<TransferState> => {
    setSettings({ transferServerEnabled: enabled === true })
    if (enabled === true) await startTransferServer()
    else stopTransferServer()
    broadcastTransferState()
    return getTransferState()
  })

  handle('transferPairWithQr', (_e, qrText: string) => pairWithQr(String(qrText ?? '')))

  handle('transferRevokePeer', (_e, peerId: string): TransferState => {
    revokePeer(String(peerId ?? ''))
    broadcastTransferState()
    return getTransferState()
  })
}

/** Boot hook: bring the server up when the user left the feature enabled. */
export function maybeStartTransferServer(): void {
  if (!getSettings().transferServerEnabled) return
  void startTransferServer().catch((err) =>
    logWarn('transfer', 'Avvio transfer server fallito al boot', err)
  )
}
