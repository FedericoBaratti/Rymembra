import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

// No electron main process in vitest: point userData at a path that doesn't
// exist (loadDevices then starts from []) and stub out the debounced JSON file
// so tests never touch the real filesystem.
vi.mock('electron', () => ({
  app: { getPath: () => 'pairing-store-test-userdata-does-not-exist' }
}))
vi.mock('../jsonFile', () => ({
  createDebouncedJsonFile: () => ({ write: () => {}, flushSync: () => {} })
}))

type PairingStore = typeof import('./pairingStore')

const CLAIM_TTL_MS = 2 * 60 * 1000

let store: PairingStore

beforeEach(async () => {
  // pendingClaim/devicesCache are module-level: fresh module per test.
  vi.resetModules()
  store = await import('./pairingStore')
})

afterEach(() => {
  vi.useRealTimers()
})

describe('createPairingClaim', () => {
  it('mints a 32-byte hex ticket with the 2-minute TTL', () => {
    vi.useFakeTimers()
    const claim = store.createPairingClaim()
    expect(claim.pairingToken).toMatch(/^[0-9a-f]{64}$/)
    expect(claim.expiresAt).toBe(Date.now() + CLAIM_TTL_MS)
  })
})

describe('claimPairing', () => {
  it('exchanges a valid ticket for a device token and registers the device', () => {
    const { pairingToken } = store.createPairingClaim()
    const result = store.claimPairing(pairingToken, 'Pixel di test')
    expect(result).not.toBeNull()
    expect(result!.deviceToken).toMatch(/^[0-9a-f]{64}$/)

    const devices = store.listPairedDevices()
    expect(devices).toHaveLength(1)
    expect(devices[0].deviceId).toBe(result!.deviceId)
    expect(devices[0].deviceName).toBe('Pixel di test')
    // The renderer-facing list must never leak the token hash.
    expect(devices[0]).not.toHaveProperty('tokenHash')
  })

  it('is single-use: the same ticket cannot be claimed twice', () => {
    const { pairingToken } = store.createPairingClaim()
    expect(store.claimPairing(pairingToken, 'A')).not.toBeNull()
    expect(store.claimPairing(pairingToken, 'B')).toBeNull()
    expect(store.listPairedDevices()).toHaveLength(1)
  })

  it('rejects a wrong token without consuming the pending claim', () => {
    const { pairingToken } = store.createPairingClaim()
    expect(store.claimPairing('ab'.repeat(32), 'intruso')).toBeNull()
    expect(store.claimPairing('troppo-corto', 'intruso')).toBeNull()
    // The legitimate phone can still complete the pairing afterwards.
    expect(store.claimPairing(pairingToken, 'legittimo')).not.toBeNull()
  })

  it('rejects an expired ticket', () => {
    vi.useFakeTimers()
    const { pairingToken } = store.createPairingClaim()
    vi.advanceTimersByTime(CLAIM_TTL_MS + 1)
    expect(store.claimPairing(pairingToken, 'in ritardo')).toBeNull()
  })

  it('rejects when no claim is pending', () => {
    expect(store.claimPairing('ab'.repeat(32), 'nessun claim')).toBeNull()
  })

  it('a new claim replaces the previous one', () => {
    const first = store.createPairingClaim()
    const second = store.createPairingClaim()
    expect(store.claimPairing(first.pairingToken, 'vecchio QR')).toBeNull()
    expect(store.claimPairing(second.pairingToken, 'QR corrente')).not.toBeNull()
  })
})

describe('verifyDeviceToken', () => {
  it('accepts the minted token and touches lastSeenAt', () => {
    vi.useFakeTimers()
    const { pairingToken } = store.createPairingClaim()
    const { deviceId, deviceToken } = store.claimPairing(pairingToken, 'Pixel')!

    vi.advanceTimersByTime(5000)
    const device = store.verifyDeviceToken(deviceToken)
    expect(device?.deviceId).toBe(deviceId)
    expect(device?.lastSeenAt).toBe(Date.now())
  })

  it('rejects unknown or missing tokens', () => {
    const { pairingToken } = store.createPairingClaim()
    store.claimPairing(pairingToken, 'Pixel')
    expect(store.verifyDeviceToken('ab'.repeat(32))).toBeNull()
    expect(store.verifyDeviceToken(null)).toBeNull()
    expect(store.verifyDeviceToken('')).toBeNull()
  })

  it('never accepts the pairing ticket itself as a device token', () => {
    const { pairingToken } = store.createPairingClaim()
    store.claimPairing(pairingToken, 'Pixel')
    expect(store.verifyDeviceToken(pairingToken)).toBeNull()
  })
})

describe('revokeDevice', () => {
  it('removes the device and invalidates its token', () => {
    const { pairingToken } = store.createPairingClaim()
    const { deviceId, deviceToken } = store.claimPairing(pairingToken, 'Pixel')!

    store.revokeDevice(deviceId)
    expect(store.listPairedDevices()).toHaveLength(0)
    expect(store.verifyDeviceToken(deviceToken)).toBeNull()
  })

  it('leaves other paired devices untouched', () => {
    const a = store.claimPairing(store.createPairingClaim().pairingToken, 'A')!
    const b = store.claimPairing(store.createPairingClaim().pairingToken, 'B')!

    store.revokeDevice(a.deviceId)
    expect(store.listPairedDevices().map((d) => d.deviceId)).toEqual([b.deviceId])
    expect(store.verifyDeviceToken(b.deviceToken)?.deviceId).toBe(b.deviceId)
  })
})
