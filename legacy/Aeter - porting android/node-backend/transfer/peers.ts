import { app } from 'electron'
import { randomBytes, randomUUID, createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { TransferPeer } from '@shared/types'
import { createDebouncedJsonFile } from '../../electron/modules/jsonFile'

/**
 * Paired-desktop store for the phone's transfer server — the mirror image of
 * the desktop's lan/pairingStore.ts, with the roles inverted: HERE the phone is
 * the server, so the phone mints the desktop's bearer token (at QR-pairing
 * time, see transfer/ipc.ts) and persists only its SHA-256 hash. The raw token
 * crosses the wire exactly once, in the POST to the desktop's one-shot pairing
 * callback, and then lives in the DESKTOP's encrypted secrets. A leaked hash
 * cannot be turned back into a valid token, so plain JSON needs no encryption.
 */
export interface StoredPeer extends TransferPeer {
  /** Never sent to the renderer — see listPeers(). */
  tokenHash: string
}

let peersCache: StoredPeer[] | null = null

function peersPath(): string {
  return join(app.getPath('userData'), 'transferPeers.json')
}

const peersFile = createDebouncedJsonFile<StoredPeer[]>(peersPath)

function loadPeers(): StoredPeer[] {
  if (peersCache) return peersCache
  try {
    const parsed = JSON.parse(readFileSync(peersPath(), 'utf-8'))
    peersCache = Array.isArray(parsed) ? parsed : []
  } catch {
    peersCache = []
  }
  return peersCache
}

function hashToken(token: string): string {
  return createHash('sha256').update(token).digest('hex')
}

/**
 * Mints a persistent bearer token for a desktop completing a QR pairing (each
 * pairing adds a peer — several desktops can be paired at once). Returns the
 * RAW token (to POST to the desktop's callback) — only its hash is stored here.
 */
export function mintDesktopToken(name: string): { peerId: string; token: string } {
  const token = randomBytes(32).toString('hex')
  const now = Date.now()
  const peer: StoredPeer = {
    peerId: randomUUID(),
    name: name?.trim().slice(0, 60) || 'Desktop',
    tokenHash: hashToken(token),
    pairedAt: now,
    lastSeenAt: now
  }
  const peers = loadPeers()
  peers.push(peer)
  peersFile.write(peers)
  return { peerId: peer.peerId, token }
}

/** Verifies a Bearer token on an incoming request; touches lastSeenAt on success. */
export function verifyDesktopToken(token: string | null | undefined): StoredPeer | null {
  if (!token) return null
  const hash = hashToken(token)
  const peers = loadPeers()
  const peer = peers.find((p) => p.tokenHash === hash)
  if (!peer) return null
  peer.lastSeenAt = Date.now()
  peersFile.write(peers)
  return peer
}

/** Renderer-facing peer list — never includes the token hash. */
export function listPeers(): TransferPeer[] {
  return loadPeers().map(({ peerId, name, pairedAt, lastSeenAt }) => ({
    peerId,
    name,
    pairedAt,
    lastSeenAt
  }))
}

export function revokePeer(peerId: string): void {
  peersCache = loadPeers().filter((p) => p.peerId !== peerId)
  peersFile.write(peersCache)
}

/** Persists any pending debounced write synchronously (fatal-handler flush). */
export function flushPeersSync(): void {
  peersFile.flushSync()
}
