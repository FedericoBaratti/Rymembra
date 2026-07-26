import { describe, it, expect, beforeAll, afterAll, beforeEach, vi } from 'vitest'
import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'

// Routing-level test: mounts the real handleRequest on a scratch http server
// and exercises the security-relevant branches (pairing rate limit, bearer
// auth) over real HTTP. Route HANDLERS are mocked — their logic has its own
// tests — so this file pins down exactly the gatekeeping in server.ts.
vi.mock('electron', () => ({
  app: { getPath: () => 'lan-routes-test-userdata-does-not-exist', getVersion: () => '0.0.0-test' }
}))
vi.mock('../jsonFile', () => ({
  createDebouncedJsonFile: () => ({ write: () => {}, flushSync: () => {} })
}))
vi.mock('./mediaRoutes', () => ({
  handleMediaRoute: (_req: unknown, res: { end: (s: string) => void }, _hash: string) => res.end('media'),
  handleArtRoute: (_req: unknown, res: { end: (s: string) => void }) => res.end('art')
}))
vi.mock('./routes', () => ({
  handleApiRoute: async () => false
}))
vi.mock('./ws', () => ({ attachLanWebSocket: () => {}, detachLanWebSocket: () => {} }))
vi.mock('./mdns', () => ({ startAdvertising: () => {}, stopAdvertising: () => {}, isAdvertising: () => false }))
vi.mock('./auth', () => ({
  authenticate: (req: { headers: Record<string, unknown> }) =>
    req.headers['authorization'] === 'Bearer good-token' ? { deviceName: 'phone' } : null
}))
vi.mock('./pairingStore', () => ({
  claimPairing: (token: string) => (token === 'valid-ticket' ? { deviceToken: 'dt', deviceName: 'phone' } : null)
}))

import { handleRequest } from './server'
import { resetPairRateLimit, PAIR_MAX_ATTEMPTS } from './pairRateLimit'

let srv: Server
let base: string

beforeAll(async () => {
  srv = createServer(handleRequest)
  await new Promise<void>((r) => srv.listen(0, '127.0.0.1', r))
  base = `http://127.0.0.1:${(srv.address() as AddressInfo).port}`
})

afterAll(async () => {
  srv.closeAllConnections()
  await new Promise((r) => srv.close(r))
})

beforeEach(() => {
  resetPairRateLimit()
})

function pair(body: unknown): Promise<Response> {
  return fetch(`${base}/api/pair`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body)
  })
}

describe('lan server routing', () => {
  it('serves /health without auth', async () => {
    const res = await fetch(`${base}/health`)
    expect(res.status).toBe(200)
    expect(await res.json()).toEqual({ ok: true, name: 'Aether', version: '0.0.0-test' })
  })

  it('rejects every authenticated route with 401 when the bearer token is missing/wrong', async () => {
    for (const path of ['/api/library', '/media/abc', '/art/abc']) {
      const res = await fetch(`${base}${path}`, { headers: { authorization: 'Bearer wrong' } })
      expect(res.status).toBe(401)
    }
  })

  it('lets a valid bearer token through to the route handlers', async () => {
    // handleApiRoute mock declines every route, so a 404 (not 401) proves the
    // request passed the auth gate and reached routing.
    const res = await fetch(`${base}/api/anything`, { headers: { authorization: 'Bearer good-token' } })
    expect(res.status).toBe(404)
  })

  it('exchanges a valid pairing ticket and 401s an invalid one', async () => {
    const ok = await pair({ pairingToken: 'valid-ticket', deviceName: 'phone' })
    expect(ok.status).toBe(200)
    expect(await ok.json()).toEqual({ deviceToken: 'dt', deviceName: 'phone' })

    const bad = await pair({ pairingToken: 'guess' })
    expect(bad.status).toBe(401)
  })

  it('rate-limits /api/pair after PAIR_MAX_ATTEMPTS tries from the same IP', async () => {
    for (let i = 0; i < PAIR_MAX_ATTEMPTS; i++) {
      expect((await pair({ pairingToken: 'guess' })).status).toBe(401)
    }
    const limited = await pair({ pairingToken: 'guess' })
    expect(limited.status).toBe(429)
    // Even a CORRECT ticket is refused while limited — the limiter runs first.
    const alsoLimited = await pair({ pairingToken: 'valid-ticket' })
    expect(alsoLimited.status).toBe(429)
  })
})
