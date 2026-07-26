import { app } from 'electron'
import { randomBytes, randomUUID, createHash, timingSafeEqual } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { PairedDevice } from '@shared/types'
import { createDebouncedJsonFile } from '../jsonFile'

/**
 * Paired-device store for the LAN thin-client protocol. Only a SHA-256 hash of
 * each device's bearer token is ever persisted — the raw token crosses the wire
 * exactly once, at pairing time (claimPairing's return value) — so this file
 * needs no encryption-at-rest: a leaked hash cannot be turned back into a valid
 * token. Kept as plain JSON like settings.json, not the encrypted secrets.json
 * pattern (which exists for material that IS reversible, e.g. OAuth tokens).
 */
export interface StoredDevice extends PairedDevice {
  /** Never sent to the renderer — see listPairedDevices(). */
  tokenHash: string
}

const CLAIM_TTL_MS = 2 * 60 * 1000

interface PendingClaim {
  pairingToken: string
  expiresAt: number
}

let pendingClaim: PendingClaim | null = null
let devicesCache: StoredDevice[] | null = null

function devicesPath(): string {
  return join(app.getPath('userData'), 'lanDevices.json')
}

const devicesFile = createDebouncedJsonFile<StoredDevice[]>(devicesPath)

function loadDevices(): StoredDevice[] {
  if (devicesCache) return devicesCache
  try {
    const parsed = JSON.parse(readFileSync(devicesPath(), 'utf-8'))
    devicesCache = Array.isArray(parsed) ? parsed : []
  } catch {
    devicesCache = []
  }
  return devicesCache
}

function hashToken(token: string): string {
  return createHash('sha256').update(token).digest('hex')
}

/** Mints a short-TTL, single-use claim ticket to embed in the pairing QR code. */
export function createPairingClaim(): { pairingToken: string; expiresAt: number } {
  const pairingToken = randomBytes(32).toString('hex')
  pendingClaim = { pairingToken, expiresAt: Date.now() + CLAIM_TTL_MS }
  return pendingClaim
}

/**
 * Consumes a claim ticket and mints a persistent device token. Returns null if
 * the ticket is missing, expired, already used, or doesn't match.
 */
export function claimPairing(
  pairingToken: string,
  deviceName: string
): { deviceId: string; deviceToken: string } | null {
  if (!pendingClaim || pendingClaim.expiresAt < Date.now()) return null
  const presented = Buffer.from(pairingToken, 'hex')
  const expected = Buffer.from(pendingClaim.pairingToken, 'hex')
  if (presented.length !== expected.length || !timingSafeEqual(presented, expected)) return null
  pendingClaim = null

  const deviceToken = randomBytes(32).toString('hex')
  const now = Date.now()
  const device: StoredDevice = {
    deviceId: randomUUID(),
    deviceName: deviceName?.trim().slice(0, 60) || 'Telefono',
    tokenHash: hashToken(deviceToken),
    pairedAt: now,
    lastSeenAt: now
  }
  const devices = loadDevices()
  devices.push(device)
  devicesFile.write(devices)
  return { deviceId: device.deviceId, deviceToken }
}

/** Verifies a bearer token from an incoming LAN request; touches lastSeenAt on success. */
export function verifyDeviceToken(token: string | null | undefined): StoredDevice | null {
  if (!token) return null
  const hash = hashToken(token)
  const devices = loadDevices()
  const device = devices.find((d) => d.tokenHash === hash)
  if (!device) return null
  device.lastSeenAt = Date.now()
  devicesFile.write(devices)
  return device
}

/** Renderer-facing device list — never includes the token hash. */
export function listPairedDevices(): PairedDevice[] {
  return loadDevices().map(({ deviceId, deviceName, pairedAt, lastSeenAt }) => ({
    deviceId,
    deviceName,
    pairedAt,
    lastSeenAt
  }))
}

export function revokeDevice(deviceId: string): void {
  devicesCache = loadDevices().filter((d) => d.deviceId !== deviceId)
  devicesFile.write(devicesCache)
}

/** Persists any pending debounced write synchronously (call on quit). */
export function flushPairingStoreSync(): void {
  devicesFile.flushSync()
}
