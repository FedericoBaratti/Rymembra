import { app } from 'electron'
import { createServer, type Server } from 'node:http'
import { randomBytes, timingSafeEqual } from 'node:crypto'
import { mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { hostname } from 'node:os'
import type { PhonePairingCode, PhonePeer, PhoneSyncState } from '@shared/types'
import { broadcast } from '../events'
import { logWarn } from '../logger'
import { loadSecrets, setSecrets } from '../secrets'
import { getLocalIPv4Addresses } from '../lan/network'
import { generateQrDataUrl } from '../lan/qr'

/**
 * Desktop half of the phone pairing — the LAN pairing flow with the roles
 * INVERTED vs lan/pairingStore.ts: the phone is the transfer server, so the
 * PHONE mints the bearer token. The desktop shows a QR containing a one-shot
 * ephemeral HTTP callback (`{v:1, kind:'aether-phone-pair', host, port,
 * pairingToken, name}`); the phone scans it and POSTs `/pair` with the freshly
 * minted token. The raw token is REVERSIBLE material → encrypted secrets store
 * (`phoneTransferToken`), while the peer metadata lives in phonePeer.json.
 */

const CLAIM_TTL_MS = 2 * 60 * 1000
const MAX_BODY_BYTES = 8 * 1024
// A hostile LAN peer hammering the one-shot listener gets it closed, not a
// brute-force window (64-hex tokens make guessing moot anyway).
const MAX_ATTEMPTS = 10

// ---- peer persistence ----------------------------------------------------

let peerCache: PhonePeer | null | undefined

function peerPath(): string {
  return join(app.getPath('userData'), 'phonePeer.json')
}

export function getPhonePeer(): PhonePeer | null {
  if (peerCache !== undefined) return peerCache
  try {
    const parsed = JSON.parse(readFileSync(peerPath(), 'utf-8')) as Partial<PhonePeer> | null
    peerCache =
      parsed &&
      typeof parsed.deviceId === 'string' &&
      parsed.deviceId &&
      typeof parsed.lastHost === 'string' &&
      typeof parsed.lastPort === 'number'
        ? {
            deviceId: parsed.deviceId,
            deviceName: typeof parsed.deviceName === 'string' ? parsed.deviceName : 'Telefono',
            lastHost: parsed.lastHost,
            lastPort: parsed.lastPort,
            pairedAt: typeof parsed.pairedAt === 'number' ? parsed.pairedAt : 0
          }
        : null
  } catch {
    peerCache = null
  }
  return peerCache
}

function writePeer(peer: PhonePeer | null): void {
  peerCache = peer
  const path = peerPath()
  try {
    if (peer === null) {
      unlinkSync(path)
      return
    }
    mkdirSync(dirname(path), { recursive: true })
    const tmp = `${path}.tmp`
    writeFileSync(tmp, JSON.stringify(peer, null, 2), 'utf-8')
    renameSync(tmp, path)
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      logWarn('phoneSync', 'Scrittura phonePeer.json fallita', err)
    }
  }
}

/** Called by the client when the phone answers from a new address (mDNS). */
export function savePhonePeerEndpoint(host: string, port: number): void {
  const peer = getPhonePeer()
  if (!peer || (peer.lastHost === host && peer.lastPort === port)) return
  writePeer({ ...peer, lastHost: host, lastPort: port })
}

/** Raw bearer token for the phone's transfer server ('' when not paired). */
export function getPhoneToken(): string {
  return loadSecrets().phoneTransferToken
}

// ---- connection status (fed by phoneSync/client.ts) ----------------------

let lastOnline: boolean | null = null

export function setPhoneOnline(online: boolean): void {
  if (lastOnline === online) return
  lastOnline = online
  broadcast('phone:state', getPhoneSyncState())
}

// ---- pairing listener ----------------------------------------------------

interface PairingSession {
  server: Server
  pairingToken: string
  expiresAt: number
  timer: ReturnType<typeof setTimeout>
  attempts: number
}

let pairingSession: PairingSession | null = null

export function getPhoneSyncState(): PhoneSyncState {
  const peer = getPhonePeer()
  return {
    paired: peer !== null && getPhoneToken() !== '',
    peer,
    pairing: pairingSession !== null,
    online: lastOnline
  }
}

function closePairing(): void {
  if (!pairingSession) return
  clearTimeout(pairingSession.timer)
  pairingSession.server.close()
  pairingSession = null
  broadcast('phone:state', getPhoneSyncState())
}

interface PairCallbackBody {
  pairingToken?: unknown
  deviceToken?: unknown
  deviceId?: unknown
  deviceName?: unknown
  transferPort?: unknown
}

function validClaim(session: PairingSession, presentedToken: string): boolean {
  if (Date.now() > session.expiresAt) return false
  if (!/^[0-9a-f]{64}$/.test(presentedToken)) return false
  const presented = Buffer.from(presentedToken, 'hex')
  const expected = Buffer.from(session.pairingToken, 'hex')
  return presented.length === expected.length && timingSafeEqual(presented, expected)
}

/**
 * Shows the phone a way in: starts a one-shot HTTP listener on an ephemeral
 * port and returns the QR that points at it. TTL 2 minutes, single use.
 */
export async function startPhonePairing(): Promise<PhonePairingCode> {
  cancelPhonePairing()

  const host = getLocalIPv4Addresses()[0] ?? null
  if (!host) return { qrDataUrl: null, expiresAt: null, error: 'no-lan-address' }

  const pairingToken = randomBytes(32).toString('hex')
  const expiresAt = Date.now() + CLAIM_TTL_MS

  const port = await new Promise<number>((resolvePort, reject) => {
    const server = createServer((req, res) => {
      const session = pairingSession
      const deny = (status: number): void => {
        res.writeHead(status, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ ok: false }))
        if (session && ++session.attempts >= MAX_ATTEMPTS) closePairing()
      }
      if (!session || req.method !== 'POST' || req.url !== '/pair') return deny(404)

      let size = 0
      const chunks: Buffer[] = []
      req.on('data', (chunk: Buffer) => {
        size += chunk.length
        if (size > MAX_BODY_BYTES) req.destroy()
        else chunks.push(chunk)
      })
      req.on('end', () => {
        let body: PairCallbackBody
        try {
          body = JSON.parse(Buffer.concat(chunks).toString('utf-8')) as PairCallbackBody
        } catch {
          return deny(400)
        }
        const deviceToken = typeof body.deviceToken === 'string' ? body.deviceToken : ''
        const deviceId = typeof body.deviceId === 'string' ? body.deviceId.slice(0, 80) : ''
        const deviceName =
          typeof body.deviceName === 'string' && body.deviceName.trim()
            ? body.deviceName.trim().slice(0, 60)
            : 'Telefono'
        const transferPort = Number(body.transferPort)
        if (
          !validClaim(session, typeof body.pairingToken === 'string' ? body.pairingToken : '') ||
          !/^[0-9a-f]{64}$/.test(deviceToken) ||
          !deviceId ||
          !Number.isInteger(transferPort) ||
          transferPort <= 0 ||
          transferPort > 65535
        ) {
          return deny(403)
        }

        // The phone's REAL address is the socket's, not whatever the payload
        // could claim — it is also the address future /health probes must use.
        const remote = (req.socket.remoteAddress ?? '').replace(/^::ffff:/, '')
        writePeer({
          deviceId,
          deviceName,
          lastHost: remote || host,
          lastPort: transferPort,
          pairedAt: Date.now()
        })
        setSecrets({ phoneTransferToken: deviceToken })
        lastOnline = true

        res.writeHead(200, { 'Content-Type': 'application/json' })
        res.end(JSON.stringify({ ok: true, name: hostname() }))
        closePairing()
      })
      req.on('error', () => deny(400))
    })
    server.on('error', reject)
    server.listen(0, '0.0.0.0', () => {
      const addr = server.address()
      if (!addr || typeof addr !== 'object') {
        reject(new Error('failed to bind pairing listener'))
        return
      }
      pairingSession = {
        server,
        pairingToken,
        expiresAt,
        attempts: 0,
        timer: setTimeout(closePairing, CLAIM_TTL_MS)
      }
      resolvePort(addr.port)
    })
  })

  const qrDataUrl = await generateQrDataUrl({
    v: 1,
    kind: 'aether-phone-pair',
    host,
    port,
    pairingToken,
    name: hostname().slice(0, 40)
  })
  broadcast('phone:state', getPhoneSyncState())
  return { qrDataUrl, expiresAt }
}

export function cancelPhonePairing(): void {
  closePairing()
}

/** Unpairs: forgets the peer metadata and blanks the stored token. */
export function forgetPhone(): void {
  writePeer(null)
  setSecrets({ phoneTransferToken: '' })
  lastOnline = null
  broadcast('phone:state', getPhoneSyncState())
}
