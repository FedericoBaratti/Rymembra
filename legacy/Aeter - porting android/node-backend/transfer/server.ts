import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http'
import { createHash } from 'node:crypto'
import { createReadStream, createWriteStream } from 'node:fs'
import { mkdir, readdir, rename, stat, unlink } from 'node:fs/promises'
import { basename, extname, join } from 'node:path'
import { app } from 'electron'
import type { PhoneTrackInfo, TransferState } from '@shared/types'
import { trackKey } from '@shared/trackKey'
import { getDb } from '../../electron/modules/db'
import { getSettings } from '../../electron/modules/settings'
import { broadcast } from '../../electron/modules/events'
import { logWarn } from '../../electron/modules/logger'
import { resolveRange, mimeForPath } from '../../electron/modules/httpRange'
import { callNative } from '../runtime'
import { verifyDesktopToken, listPeers, type StoredPeer } from './peers'
import { commitReplace, sha256File } from './commit'

/**
 * Authenticated LAN transfer server — the phone side of the "repair from PC"
 * feature. The OPPOSITE of the loopback media server (server.ts): this one
 * binds 0.0.0.0 on an ephemeral port, requires a Bearer token minted at QR
 * pairing (transfer/peers.ts) on every /api route, and exposes the library for
 * a paired desktop to pull files, push repaired versions and commit the
 * replacement (transfer/commit.ts).
 *
 *   GET  /health              → {ok, name, deviceId, deviceName, version} (no auth)
 *   GET  /api/tracks          → PhoneTrackInfo[]
 *   GET  /api/file/:id        → audio bytes, Range + ETag "<size>-<mtime>"
 *   GET  /api/hash/:id        → {sha256, size, mtimeMs} (streamed, cached)
 *   POST /api/upload/:id      → staged upload (X-Aether-Sha256/X-Aether-Ext)
 *   POST /api/commit/:id      → {uploadId, sha256, ext} → replace on device
 *   DELETE /api/upload/:id    → drop staged uploads for the track
 *   POST /api/session/start|heartbeat|end → exclusive repair session; start
 *        raises the TransferForegroundService (WakeLock+WifiLock) via
 *        reverse-RPC, a 60s missed-heartbeat watchdog tears it down.
 */

// Injected by vite.config.node-backend.ts (electron/globals.d.ts is outside
// the node-backend tsconfig include, so re-declare locally).
declare const __APP_VERSION__: string | undefined

const PROTOCOL_VERSION = 1
const MAX_UPLOAD_BYTES = 2 * 1024 * 1024 * 1024 // >2GB is excluded by design
const MAX_JSON_BYTES = 64 * 1024
const SESSION_TIMEOUT_MS = 60_000
const WATCHDOG_EVERY_MS = 15_000

const APP_VERSION =
  typeof __APP_VERSION__ !== 'undefined' && __APP_VERSION__ ? __APP_VERSION__ : '0.0.0'

let server: Server | null = null
let port: number | null = null
let deviceName = 'Telefono Android'

interface Session {
  peerId: string
  startedAt: number
  lastBeatAt: number
}
let session: Session | null = null
let watchdog: ReturnType<typeof setInterval> | null = null

// ---- state ---------------------------------------------------------------

export function getTransferState(): TransferState {
  const settings = getSettings()
  return {
    enabled: settings.transferServerEnabled,
    running: server !== null,
    port,
    deviceId: settings.syncDeviceId,
    deviceName,
    peers: listPeers(),
    sessionActive: session !== null
  }
}

export function broadcastTransferState(): void {
  broadcast('transfer:state', getTransferState())
}

// ---- session / foreground service ---------------------------------------

function startSession(peer: StoredPeer): void {
  session = { peerId: peer.peerId, startedAt: Date.now(), lastBeatAt: Date.now() }
  // Keep the device awake for the whole run (dataSync FGS + WakeLock + WifiLock).
  // Best-effort: a missing plugin must never block the transfer itself.
  void callNative('transferServiceStart', undefined, 8000).catch(() => {})
  if (!watchdog) {
    watchdog = setInterval(() => {
      if (session && Date.now() - session.lastBeatAt > SESSION_TIMEOUT_MS) {
        logWarn('transfer', 'Sessione di riparazione scaduta (heartbeat perso)')
        endSession()
      }
    }, WATCHDOG_EVERY_MS)
  }
  broadcastTransferState()
}

function endSession(): void {
  session = null
  if (watchdog) {
    clearInterval(watchdog)
    watchdog = null
  }
  void callNative('transferServiceStop', undefined, 8000).catch(() => {})
  broadcastTransferState()
}

// ---- helpers -------------------------------------------------------------

function sendJson(res: ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body)
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': String(Buffer.byteLength(payload))
  })
  res.end(payload)
}

function readJsonBody(req: IncomingMessage): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    let size = 0
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => {
      size += chunk.length
      if (size > MAX_JSON_BYTES) {
        req.destroy()
        reject(new Error('BODY_TOO_LARGE'))
        return
      }
      chunks.push(chunk)
    })
    req.on('end', () => {
      if (chunks.length === 0) return resolve({})
      try {
        const parsed = JSON.parse(Buffer.concat(chunks).toString('utf-8'))
        resolve(typeof parsed === 'object' && parsed !== null ? parsed : {})
      } catch {
        reject(new Error('BAD_JSON'))
      }
    })
    req.on('error', reject)
  })
}

function bearerToken(req: IncomingMessage): string | null {
  const header = req.headers.authorization
  if (!header || !header.startsWith('Bearer ')) return null
  return header.slice('Bearer '.length).trim()
}

function transferDir(): string {
  return join(app.getPath('temp'), 'transfer')
}

/** mtime with the same rounding the scanner persists (floor of mtimeMs). */
function fileMtime(st: { mtimeMs?: number; mtime?: Date }): number {
  return Math.floor(st.mtimeMs ?? st.mtime?.getTime() ?? Date.now())
}

interface TrackRow {
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

function getTrackRow(id: number): TrackRow | null {
  if (!Number.isFinite(id)) return null
  return (
    (getDb().prepare('SELECT * FROM tracks WHERE id = ? AND is_local = 1').get(id) as
      | TrackRow
      | undefined) ?? null
  )
}

// ---- routes --------------------------------------------------------------

function listTracks(res: ServerResponse): void {
  const rows = getDb()
    .prepare('SELECT * FROM tracks WHERE is_local = 1 ORDER BY id')
    .all() as TrackRow[]
  const infos: PhoneTrackInfo[] = rows.map((r) => ({
    id: r.id,
    trackKey: trackKey({ artist: r.artist, title: r.title, album: r.album }),
    title: r.title,
    artist: r.artist,
    album: r.album,
    year: r.year,
    genre: r.genre,
    durationS: r.duration,
    codec: r.codec,
    bitrate: r.bitrate,
    sampleRate: r.sample_rate,
    fileSize: r.file_size,
    // DB value (floor of the file mtime at last scan): cheap for thousands of
    // rows; per-file freshness is re-checked by /api/hash and the ETag.
    mtimeMs: r.date_modified,
    ext: extname(r.path).toLowerCase(),
    basename: basename(r.path),
    hasCover: r.cover_art_hash !== null,
    enrichStatus: r.enrich_status
  }))
  sendJson(res, 200, infos)
}

async function serveFile(req: IncomingMessage, res: ServerResponse, id: number): Promise<void> {
  const row = getTrackRow(id)
  if (!row) return sendJson(res, 404, { error: 'not-found' })
  let st
  try {
    st = await stat(row.path)
  } catch {
    return sendJson(res, 404, { error: 'file-missing' })
  }
  const etag = `"${st.size}-${fileMtime(st)}"`
  const range = resolveRange(req.headers.range, st.size)
  const headers: Record<string, string> = {
    ...range.headers,
    'Accept-Ranges': 'bytes',
    'Content-Type': mimeForPath(row.path),
    ETag: etag
  }
  if (range.status === 416) {
    res.writeHead(416, headers)
    res.end()
    return
  }
  res.writeHead(range.status, headers)
  const stream = createReadStream(row.path, { start: range.start, end: range.end })
  stream.on('error', (err) => {
    logWarn('transfer', `Stream fallito: ${row.path}`, err)
    res.destroy(err)
  })
  stream.pipe(res)
}

// sha256 results keyed by `id:size:mtime` — a repair run hashes each file at
// most twice (pull verify + post-commit verify); never rehash unchanged files.
const hashCache = new Map<string, string>()

async function serveHash(res: ServerResponse, id: number): Promise<void> {
  const row = getTrackRow(id)
  if (!row) return sendJson(res, 404, { error: 'not-found' })
  let st
  try {
    st = await stat(row.path)
  } catch {
    return sendJson(res, 404, { error: 'file-missing' })
  }
  const mtimeMs = fileMtime(st)
  const key = `${id}:${st.size}:${mtimeMs}`
  let sha256 = hashCache.get(key)
  if (!sha256) {
    sha256 = await sha256File(row.path)
    if (hashCache.size > 512) hashCache.clear()
    hashCache.set(key, sha256)
  }
  sendJson(res, 200, { sha256, size: st.size, mtimeMs })
}

async function handleUpload(req: IncomingMessage, res: ServerResponse, id: number): Promise<void> {
  const row = getTrackRow(id)
  if (!row) return sendJson(res, 404, { error: 'not-found' })
  const expectedSha = String(req.headers['x-aether-sha256'] ?? '').toLowerCase()
  const ext = String(req.headers['x-aether-ext'] ?? '')
  if (!/^[0-9a-f]{64}$/.test(expectedSha)) return sendJson(res, 400, { error: 'bad-sha256' })
  if (!/^\.[a-z0-9]{1,5}$/i.test(ext)) return sendJson(res, 400, { error: 'bad-ext' })
  const declared = Number(req.headers['content-length'] ?? 0)
  if (declared > MAX_UPLOAD_BYTES) return sendJson(res, 413, { error: 'too-large' })

  const dir = transferDir()
  await mkdir(dir, { recursive: true })
  const uploadId = `upl-${id}-${Date.now()}`
  const partPath = join(dir, `${uploadId}.part`)
  const finalPath = join(dir, `${uploadId}.bin`)

  const hash = createHash('sha256')
  let received = 0
  let failed = false
  const sink = createWriteStream(partPath)

  const fail = (status: number, error: string): void => {
    if (failed) return
    failed = true
    sink.destroy()
    void unlink(partPath).catch(() => {})
    // The client may still be streaming: answer and cut the connection.
    sendJson(res, status, { error })
    req.destroy()
  }

  req.on('data', (chunk: Buffer) => {
    received += chunk.length
    if (received > MAX_UPLOAD_BYTES) return fail(413, 'too-large')
    hash.update(chunk)
  })
  req.pipe(sink)
  req.on('error', () => fail(499, 'aborted'))
  sink.on('error', () => fail(500, 'write-failed'))
  sink.on('finish', () => {
    if (failed) return
    void (async () => {
      const actual = hash.digest('hex')
      if (actual !== expectedSha) {
        await unlink(partPath).catch(() => {})
        sendJson(res, 422, { error: 'hash-mismatch' })
        return
      }
      await rename(partPath, finalPath)
      sendJson(res, 200, { uploadId })
    })().catch((err) => {
      logWarn('transfer', 'Upload finalize fallito', err)
      sendJson(res, 500, { error: 'finalize-failed' })
    })
  })
}

async function handleCommit(req: IncomingMessage, res: ServerResponse, id: number): Promise<void> {
  const body = await readJsonBody(req)
  const uploadId = String(body.uploadId ?? '')
  const sha256 = String(body.sha256 ?? '').toLowerCase()
  const ext = String(body.ext ?? '')
  // uploadId is used to build a path — accept only our own minted shape.
  if (!/^upl-\d+-\d+$/.test(uploadId)) return sendJson(res, 400, { error: 'bad-upload-id' })
  if (!/^[0-9a-f]{64}$/.test(sha256)) return sendJson(res, 400, { error: 'bad-sha256' })
  if (!/^\.[a-z0-9]{1,5}$/i.test(ext)) return sendJson(res, 400, { error: 'bad-ext' })
  const uploadPath = join(transferDir(), `${uploadId}.bin`)
  try {
    const result = await commitReplace(id, uploadPath, ext, sha256)
    sendJson(res, 200, { ok: true, ...result })
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    const status =
      message === 'TRACK_NOT_FOUND' || message === 'UPLOAD_NOT_FOUND'
        ? 404
        : message === 'HASH_MISMATCH'
          ? 422
          : 500
    sendJson(res, status, { error: message })
  }
}

async function handleUploadDelete(res: ServerResponse, id: number): Promise<void> {
  const dir = transferDir()
  const names = await readdir(dir).catch(() => [] as string[])
  const prefix = `upl-${id}-`
  let removed = 0
  for (const name of names) {
    if (!name.startsWith(prefix)) continue
    await unlink(join(dir, name)).catch(() => {})
    removed++
  }
  sendJson(res, 200, { removed })
}

async function handleSession(
  req: IncomingMessage,
  res: ServerResponse,
  peer: StoredPeer,
  action: string
): Promise<void> {
  if (action === 'start') {
    if (session && session.peerId !== peer.peerId) {
      return sendJson(res, 409, { error: 'busy' })
    }
    if (!session) startSession(peer)
    else session.lastBeatAt = Date.now() // idempotent re-start from the same desktop
    return sendJson(res, 200, { ok: true })
  }
  if (action === 'heartbeat') {
    if (!session || session.peerId !== peer.peerId) {
      return sendJson(res, 409, { error: 'no-session' })
    }
    session.lastBeatAt = Date.now()
    const body = await readJsonBody(req)
    const done = Number(body.done)
    const total = Number(body.total)
    if (Number.isFinite(done) && Number.isFinite(total) && total > 0) {
      void callNative('transferServiceProgress', { done, total }, 8000).catch(() => {})
    }
    return sendJson(res, 200, { ok: true })
  }
  if (action === 'end') {
    if (session && session.peerId === peer.peerId) endSession()
    return sendJson(res, 200, { ok: true })
  }
  sendJson(res, 404, { error: 'not-found' })
}

// ---- dispatcher ----------------------------------------------------------

/** Exported for server.routes.test.ts, which mounts it on a scratch server. */
export async function handleRequest(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const url = new URL(req.url ?? '/', 'http://phone.invalid')
  const seg = url.pathname.split('/').filter(Boolean)

  if (seg[0] === 'health' && req.method === 'GET') {
    const settings = getSettings()
    return sendJson(res, 200, {
      ok: true,
      name: 'AetherPhone',
      deviceId: settings.syncDeviceId,
      deviceName,
      version: APP_VERSION,
      protocol: PROTOCOL_VERSION
    })
  }

  if (seg[0] !== 'api') return sendJson(res, 404, { error: 'not-found' })

  const peer = verifyDesktopToken(bearerToken(req))
  if (!peer) return sendJson(res, 401, { error: 'unauthorized' })

  if (seg[1] === 'tracks' && req.method === 'GET') return listTracks(res)
  if (seg[1] === 'file' && req.method === 'GET') return serveFile(req, res, Number(seg[2]))
  if (seg[1] === 'hash' && req.method === 'GET') return serveHash(res, Number(seg[2]))
  if (seg[1] === 'upload' && req.method === 'POST') return handleUpload(req, res, Number(seg[2]))
  if (seg[1] === 'upload' && req.method === 'DELETE') {
    return handleUploadDelete(res, Number(seg[2]))
  }
  if (seg[1] === 'commit' && req.method === 'POST') return handleCommit(req, res, Number(seg[2]))
  if (seg[1] === 'session' && req.method === 'POST') {
    return handleSession(req, res, peer, seg[2] ?? '')
  }

  sendJson(res, 404, { error: 'not-found' })
}

// ---- lifecycle -----------------------------------------------------------

async function resolveDeviceName(): Promise<void> {
  try {
    const res = (await callNative('transferDeviceName', undefined, 5000)) as {
      name?: string
    } | null
    if (res?.name) deviceName = res.name
  } catch {
    /* keep the default */
  }
}

/** Starts the transfer server (idempotent). Resolves with the bound port. */
export function startTransferServer(): Promise<number> {
  if (server && port !== null) return Promise.resolve(port)
  return new Promise((resolve, reject) => {
    const srv = createServer((req, res) => {
      handleRequest(req, res).catch((err) => {
        logWarn('transfer', 'Errore richiesta transfer', err)
        if (!res.headersSent) sendJson(res, 500, { error: 'internal' })
        else res.destroy()
      })
    })
    srv.on('error', reject)
    // 0.0.0.0: the whole point is being reachable from the desktop on the LAN;
    // every /api route requires the pairing Bearer token.
    srv.listen(0, '0.0.0.0', () => {
      const addr = srv.address()
      if (!addr || typeof addr !== 'object') {
        reject(new Error('failed to bind transfer server'))
        return
      }
      server = srv
      port = addr.port
      void resolveDeviceName().then(() => {
        // Advertise for the desktop's rediscovery (mDNS/NSD). Best-effort.
        void callNative(
          'lanRegisterService',
          { port: addr.port, deviceId: getSettings().syncDeviceId, deviceName },
          8000
        ).catch(() => {})
        broadcastTransferState()
      })
      resolve(addr.port)
    })
  })
}

export function stopTransferServer(): void {
  if (session) endSession()
  if (server) {
    server.close()
    server = null
    port = null
    void callNative('lanUnregisterService', undefined, 8000).catch(() => {})
  }
  broadcastTransferState()
}
