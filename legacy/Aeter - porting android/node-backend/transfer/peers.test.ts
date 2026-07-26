import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createHash } from 'node:crypto'

// No electron main process in vitest: point userData at a path that doesn't
// exist (loadPeers then starts from []) and stub out the debounced JSON file
// so tests never touch the real filesystem — same recipe as pairingStore.test.ts
// on the desktop, whose store this module mirrors with the roles inverted.
vi.mock('electron', () => ({
  app: { getPath: () => 'transfer-peers-test-userdata-does-not-exist' }
}))
vi.mock('../../electron/modules/jsonFile', () => ({
  createDebouncedJsonFile: () => ({ write: () => {}, flushSync: () => {} })
}))

type Peers = typeof import('./peers')

let peers: Peers

beforeEach(async () => {
  // peersCache is module-level: fresh module per test.
  vi.resetModules()
  peers = await import('./peers')
})

afterEach(() => {
  vi.useRealTimers()
})

describe('mintDesktopToken', () => {
  it('mints a 32-byte hex token and registers the peer', () => {
    const { peerId, token } = peers.mintDesktopToken('Aether Desktop')
    expect(token).toMatch(/^[0-9a-f]{64}$/)

    const list = peers.listPeers()
    expect(list).toHaveLength(1)
    expect(list[0].peerId).toBe(peerId)
    expect(list[0].name).toBe('Aether Desktop')
    // The renderer-facing list must never leak the token hash.
    expect(list[0]).not.toHaveProperty('tokenHash')
  })

  it('falls back to a default name and trims/caps the given one', () => {
    peers.mintDesktopToken('')
    peers.mintDesktopToken(`  ${'x'.repeat(100)}  `)
    const [empty, long] = peers.listPeers()
    expect(empty.name).toBe('Desktop')
    expect(long.name.length).toBeLessThanOrEqual(62) // 60 + the trimmed-off spaces at most
    expect(long.name.startsWith('x')).toBe(true)
  })

  it('supports several paired desktops at once', () => {
    const a = peers.mintDesktopToken('A')
    const b = peers.mintDesktopToken('B')
    expect(a.peerId).not.toBe(b.peerId)
    expect(a.token).not.toBe(b.token)
    expect(peers.listPeers().map((p) => p.name)).toEqual(['A', 'B'])
  })
})

describe('verifyDesktopToken', () => {
  it('accepts the minted token and touches lastSeenAt', () => {
    vi.useFakeTimers()
    const { peerId, token } = peers.mintDesktopToken('Desktop')
    vi.advanceTimersByTime(5000)
    const peer = peers.verifyDesktopToken(token)
    expect(peer?.peerId).toBe(peerId)
    expect(peer?.lastSeenAt).toBe(Date.now())
  })

  it('rejects unknown or missing tokens', () => {
    peers.mintDesktopToken('Desktop')
    expect(peers.verifyDesktopToken('ab'.repeat(32))).toBeNull()
    expect(peers.verifyDesktopToken(null)).toBeNull()
    expect(peers.verifyDesktopToken(undefined)).toBeNull()
    expect(peers.verifyDesktopToken('')).toBeNull()
  })

  it('never accepts the stored hash itself as a token', () => {
    // A leaked transferPeers.json must not be enough to authenticate: the
    // stored value is the SHA-256 of the token, not the token.
    const { token } = peers.mintDesktopToken('Desktop')
    const hash = createHash('sha256').update(token).digest('hex')
    expect(peers.verifyDesktopToken(hash)).toBeNull()
  })
})

describe('revokePeer', () => {
  it('removes the peer and invalidates its token', () => {
    const { peerId, token } = peers.mintDesktopToken('Desktop')
    peers.revokePeer(peerId)
    expect(peers.listPeers()).toHaveLength(0)
    expect(peers.verifyDesktopToken(token)).toBeNull()
  })

  it('leaves other paired desktops untouched', () => {
    const a = peers.mintDesktopToken('A')
    const b = peers.mintDesktopToken('B')
    peers.revokePeer(a.peerId)
    expect(peers.listPeers().map((p) => p.peerId)).toEqual([b.peerId])
    expect(peers.verifyDesktopToken(b.token)?.peerId).toBe(b.peerId)
  })
})
