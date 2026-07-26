import { describe, it, expect, beforeAll, afterAll, vi } from 'vitest'
import { createServer, request as httpRequest, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { createHash } from 'node:crypto'
import { mkdtempSync, writeFileSync, existsSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// Routing-level test: mounts the real handleRequest on a scratch http server
// and exercises the gatekeeping over real HTTP — bearer auth, Range serving,
// upload hash/size validation, and the single-session policy. The commit logic
// is mocked (transfer/commit.test.ts owns it); the DB is a fixture row store.
// Pattern: the desktop tree's lan/server.routes.test.ts.

interface FixtureRow {
  id: number
  path: string
  title: string
  artist: string
  album: string
  year: number | null
  genre: string | null
  duration: number
  codec: string | null
  bitrate: number | null
  sample_rate: number | null
  file_size: number
  date_modified: number
  cover_art_hash: string | null
  enrich_status: string | null
}

const state = vi.hoisted(() => ({ tempRoot: '', rows: [] as { id: number }[] }))

vi.mock('electron', () => ({
  app: { getPath: () => state.tempRoot }
}))
vi.mock('../../electron/modules/db', () => ({
  getDb: () => ({
    prepare: () => ({
      get: (id: number) => state.rows.find((r) => r.id === id),
      all: () => state.rows
    })
  })
}))
vi.mock('../../electron/modules/settings', () => ({
  getSettings: () => ({ transferServerEnabled: true, syncDeviceId: 'phone-dev-1' })
}))
vi.mock('../../electron/modules/events', () => ({ broadcast: vi.fn() }))
vi.mock('../../electron/modules/logger', () => ({ logWarn: vi.fn() }))
vi.mock('../runtime', () => ({ callNative: vi.fn(async () => ({})) }))
vi.mock('./peers', () => ({
  listPeers: () => [],
  verifyDesktopToken: (token: string | null) =>
    token === 'token-a'
      ? { peerId: 'peer-a', name: 'Desktop A', tokenHash: 'h', pairedAt: 0, lastSeenAt: 0 }
      : token === 'token-b'
        ? { peerId: 'peer-b', name: 'Desktop B', tokenHash: 'h', pairedAt: 0, lastSeenAt: 0 }
        : null
}))
vi.mock('./commit', () => ({
  commitReplace: vi.fn(),
  sha256File: vi.fn(async () => 'f'.repeat(64))
}))

import { handleRequest } from './server'
import { commitReplace } from './commit'

const FILE_BYTES = '0123456789'

let srv: Server
let base: string

beforeAll(async () => {
  state.tempRoot = mkdtempSync(join(tmpdir(), 'aether-transfer-routes-'))
  const musicPath = join(state.tempRoot, 'song.mp3')
  writeFileSync(musicPath, FILE_BYTES)
  const row: FixtureRow = {
    id: 1,
    path: musicPath,
    title: 'Title',
    artist: 'Artist',
    album: 'Album',
    year: 2020,
    genre: 'Rock',
    duration: 10,
    codec: 'MPEG 1 Layer 3',
    bitrate: 320_000,
    sample_rate: 44_100,
    file_size: FILE_BYTES.length,
    date_modified: 1234,
    cover_art_hash: null,
    enrich_status: 'ok'
  }
  state.rows.push(row)

  srv = createServer((req, res) => {
    handleRequest(req, res).catch(() => res.destroy())
  })
  await new Promise<void>((r) => srv.listen(0, '127.0.0.1', r))
  base = `http://127.0.0.1:${(srv.address() as AddressInfo).port}`
})

afterAll(async () => {
  srv.closeAllConnections()
  await new Promise((r) => srv.close(r))
  rmSync(state.tempRoot, { recursive: true, force: true })
})

function get(path: string, token = 'token-a', headers: Record<string, string> = {}): Promise<Response> {
  return fetch(`${base}${path}`, { headers: { authorization: `Bearer ${token}`, ...headers } })
}

function post(path: string, token: string, body?: unknown): Promise<Response> {
  return fetch(`${base}${path}`, {
    method: 'POST',
    headers: { authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body)
  })
}

function upload(body: string, sha256: string, ext = '.mp3'): Promise<Response> {
  return fetch(`${base}/api/upload/1`, {
    method: 'POST',
    headers: {
      authorization: 'Bearer token-a',
      'X-Aether-Sha256': sha256,
      'X-Aether-Ext': ext
    },
    body
  })
}

describe('transfer server routing', () => {
  it('serves /health without auth', async () => {
    const res = await fetch(`${base}/health`)
    expect(res.status).toBe(200)
    const body = await res.json()
    expect(body.ok).toBe(true)
    expect(body.name).toBe('AetherPhone')
    expect(body.deviceId).toBe('phone-dev-1')
    expect(body.protocol).toBe(1)
  })

  it('rejects every /api route with 401 when the bearer token is missing or wrong', async () => {
    for (const path of ['/api/tracks', '/api/file/1', '/api/hash/1']) {
      expect((await fetch(`${base}${path}`)).status).toBe(401)
      expect((await get(path, 'wrong-token')).status).toBe(401)
    }
    expect((await post('/api/session/start', 'wrong-token')).status).toBe(401)
  })

  it('lists tracks with the wire mapping', async () => {
    const res = await get('/api/tracks')
    expect(res.status).toBe(200)
    const infos = await res.json()
    expect(infos).toHaveLength(1)
    expect(infos[0]).toMatchObject({
      id: 1,
      title: 'Title',
      ext: '.mp3',
      basename: 'song.mp3',
      hasCover: false,
      enrichStatus: 'ok',
      fileSize: FILE_BYTES.length
    })
    expect(infos[0].trackKey).toBeTruthy()
  })

  it('serves the whole file with an ETag, and honors Range requests', async () => {
    const full = await get('/api/file/1')
    expect(full.status).toBe(200)
    expect(await full.text()).toBe(FILE_BYTES)
    expect(full.headers.get('etag')).toMatch(/^"10-\d+"$/)
    expect(full.headers.get('accept-ranges')).toBe('bytes')

    const part = await get('/api/file/1', 'token-a', { range: 'bytes=2-5' })
    expect(part.status).toBe(206)
    expect(part.headers.get('content-range')).toBe('bytes 2-5/10')
    expect(await part.text()).toBe('2345')

    const bad = await get('/api/file/1', 'token-a', { range: 'bytes=99-' })
    expect(bad.status).toBe(416)
  })

  it('404s a file for an unknown track id', async () => {
    expect((await get('/api/file/99')).status).toBe(404)
  })

  it('serves the source hash with size and mtime', async () => {
    const res = await get('/api/hash/1')
    expect(res.status).toBe(200)
    const body = await res.json()
    expect(body.sha256).toBe('f'.repeat(64))
    expect(body.size).toBe(FILE_BYTES.length)
    expect(body.mtimeMs).toBeGreaterThan(0)
  })

  it('accepts an upload whose hash matches and stages it as .bin', async () => {
    const bytes = 'repaired-audio-bytes'
    const sha = createHash('sha256').update(bytes).digest('hex')
    const res = await upload(bytes, sha)
    expect(res.status).toBe(200)
    const { uploadId } = await res.json()
    expect(uploadId).toMatch(/^upl-1-\d+$/)
    const staged = join(state.tempRoot, 'transfer', `${uploadId}.bin`)
    expect(existsSync(staged)).toBe(true)
    expect(readFileSync(staged, 'utf-8')).toBe(bytes)
  })

  it('422s an upload whose bytes do not match the declared hash and keeps nothing', async () => {
    const res = await upload('corrupted-in-flight', 'ab'.repeat(32))
    expect(res.status).toBe(422)
    expect((await res.json()).error).toBe('hash-mismatch')
  })

  it('400s malformed upload headers', async () => {
    expect((await upload('x', 'not-a-hash')).status).toBe(400)
    expect((await upload('x', 'ab'.repeat(32), 'mp3')).status).toBe(400) // missing dot
  })

  it('413s an upload whose declared size exceeds the 2GB cap before reading the body', async () => {
    const status = await new Promise<number>((resolve, reject) => {
      const req = httpRequest(
        `${base}/api/upload/1`,
        {
          method: 'POST',
          headers: {
            authorization: 'Bearer token-a',
            'X-Aether-Sha256': 'ab'.repeat(32),
            'X-Aether-Ext': '.mp3',
            'Content-Length': String(3 * 1024 * 1024 * 1024)
          }
        },
        (res) => {
          resolve(res.statusCode ?? 0)
          res.resume()
          req.destroy()
        }
      )
      req.on('error', reject)
      req.flushHeaders()
    })
    expect(status).toBe(413)
  })

  it('routes a well-formed commit to commitReplace and rejects a malformed uploadId', async () => {
    vi.mocked(commitReplace).mockResolvedValueOnce({
      trackId: 1,
      path: '/x/song.mp3',
      changedExt: false
    })
    const ok = await post('/api/commit/1', 'token-a', {
      uploadId: 'upl-1-123',
      sha256: 'ab'.repeat(32),
      ext: '.mp3'
    })
    expect(ok.status).toBe(200)
    expect((await ok.json()).ok).toBe(true)
    expect(commitReplace).toHaveBeenCalledWith(
      1,
      join(state.tempRoot, 'transfer', 'upl-1-123.bin'),
      '.mp3',
      'ab'.repeat(32)
    )

    // uploadId feeds a path join: only the minted shape may pass.
    const evil = await post('/api/commit/1', 'token-a', {
      uploadId: '../../etc/passwd',
      sha256: 'ab'.repeat(32),
      ext: '.mp3'
    })
    expect(evil.status).toBe(400)
  })

  it('maps commit errors to statuses (404 missing, 422 hash, 500 rest)', async () => {
    const cases: [string, number][] = [
      ['TRACK_NOT_FOUND', 404],
      ['UPLOAD_NOT_FOUND', 404],
      ['HASH_MISMATCH', 422],
      ['EIO', 500]
    ]
    for (const [message, status] of cases) {
      vi.mocked(commitReplace).mockRejectedValueOnce(new Error(message))
      const res = await post('/api/commit/1', 'token-a', {
        uploadId: 'upl-1-123',
        sha256: 'ab'.repeat(32),
        ext: '.mp3'
      })
      expect(res.status).toBe(status)
      expect((await res.json()).error).toBe(message)
    }
  })

  it('enforces one repair session: a second desktop gets 409 until the first ends', async () => {
    expect((await post('/api/session/start', 'token-a')).status).toBe(200)
    // Same desktop restarting is idempotent, another desktop is refused.
    expect((await post('/api/session/start', 'token-a')).status).toBe(200)
    expect((await post('/api/session/start', 'token-b')).status).toBe(409)
    expect((await post('/api/session/heartbeat', 'token-b', {})).status).toBe(409)
    expect((await post('/api/session/heartbeat', 'token-a', { done: 1, total: 4 })).status).toBe(200)

    expect((await post('/api/session/end', 'token-a')).status).toBe(200)
    expect((await post('/api/session/start', 'token-b')).status).toBe(200)
    expect((await post('/api/session/end', 'token-b')).status).toBe(200)
  })

  it('404s unknown paths', async () => {
    expect((await fetch(`${base}/nope`)).status).toBe(404)
    expect((await get('/api/nope')).status).toBe(404)
  })
})
