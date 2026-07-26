import { request } from 'node:http'
import { createReadStream, createWriteStream } from 'node:fs'
import { stat } from 'node:fs/promises'
import Bonjour from 'bonjour-service'
import type { PhoneTrackInfo } from '@shared/types'
import { logWarn } from '../logger'
import { getPhonePeer, getPhoneToken, savePhonePeerEndpoint, setPhoneOnline } from './pairing'

/**
 * Typed REST client for the phone's transfer server (AND node-backend/
 * transfer/server.ts). JSON goes through fetch; the two big byte streams
 * (pull /api/file, push /api/upload) go through node:http.request so they are
 * piped with backpressure instead of buffered.
 *
 * connect() strategy: last known endpoint first (`/health` + deviceId match),
 * then a 5s mDNS browse of `_aether-transfer._tcp` matched on the TXT deviceId
 * — the phone's DHCP address changes; its identity doesn't.
 */

export class PhoneClientError extends Error {
  constructor(
    message: string,
    readonly status?: number
  ) {
    super(message)
  }
}

export interface PhoneEndpoint {
  host: string
  port: number
}

interface HealthInfo {
  ok: boolean
  deviceId: string
  deviceName?: string
}

const HEALTH_TIMEOUT_MS = 4000
const BROWSE_TIMEOUT_MS = 5000
const JSON_TIMEOUT_MS = 30_000

let current: PhoneEndpoint | null = null

async function probeHealth(host: string, port: number): Promise<HealthInfo | null> {
  try {
    const res = await fetch(`http://${host}:${port}/health`, {
      signal: AbortSignal.timeout(HEALTH_TIMEOUT_MS)
    })
    if (!res.ok) return null
    const body = (await res.json()) as Partial<HealthInfo> | null
    return body && body.ok === true && typeof body.deviceId === 'string'
      ? (body as HealthInfo)
      : null
  } catch {
    return null
  }
}

/** 5s mDNS browse for the paired phone's transfer service (TXT deviceId match). */
function browseForPhone(deviceId: string): Promise<PhoneEndpoint | null> {
  return new Promise((resolve) => {
    const bonjour = new Bonjour()
    let settled = false
    const finish = (ep: PhoneEndpoint | null): void => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      browser.stop()
      bonjour.destroy()
      resolve(ep)
    }
    const timer = setTimeout(() => finish(null), BROWSE_TIMEOUT_MS)
    const browser = bonjour.find({ type: 'aether-transfer', protocol: 'tcp' }, (service) => {
      const txt = (service.txt ?? {}) as Record<string, string>
      if (txt.deviceId !== deviceId) return
      const host = (service.addresses ?? []).find((a) => a.includes('.')) ?? null
      if (!host || !service.port) return
      finish({ host, port: service.port })
    })
  })
}

/**
 * Resolves a live endpoint for the paired phone, updating the persisted
 * last-known host/port. Throws PHONE_NOT_PAIRED / PHONE_OFFLINE.
 */
export async function connect(): Promise<PhoneEndpoint> {
  const peer = getPhonePeer()
  const token = getPhoneToken()
  if (!peer || !token) throw new PhoneClientError('PHONE_NOT_PAIRED')

  // Fast path: the endpoint that worked last time.
  const last = current ?? { host: peer.lastHost, port: peer.lastPort }
  const fromLast = await probeHealth(last.host, last.port)
  if (fromLast && fromLast.deviceId === peer.deviceId) {
    current = last
    savePhonePeerEndpoint(last.host, last.port)
    setPhoneOnline(true)
    return last
  }

  // The phone moved (DHCP) — ask the network who it is now.
  const discovered = await browseForPhone(peer.deviceId)
  if (discovered) {
    const check = await probeHealth(discovered.host, discovered.port)
    if (check && check.deviceId === peer.deviceId) {
      current = discovered
      savePhonePeerEndpoint(discovered.host, discovered.port)
      setPhoneOnline(true)
      return discovered
    }
  }

  current = null
  setPhoneOnline(false)
  throw new PhoneClientError('PHONE_OFFLINE')
}

function requireEndpoint(): PhoneEndpoint {
  if (!current) throw new PhoneClientError('PHONE_OFFLINE')
  return current
}

function authHeaders(): Record<string, string> {
  return { Authorization: `Bearer ${getPhoneToken()}` }
}

async function apiJson<T>(
  path: string,
  init: { method?: string; body?: unknown; timeoutMs?: number } = {}
): Promise<T> {
  const { host, port } = requireEndpoint()
  let res: Response
  try {
    res = await fetch(`http://${host}:${port}${path}`, {
      method: init.method ?? 'GET',
      headers: {
        ...authHeaders(),
        ...(init.body !== undefined ? { 'Content-Type': 'application/json' } : {})
      },
      body: init.body !== undefined ? JSON.stringify(init.body) : undefined,
      signal: AbortSignal.timeout(init.timeoutMs ?? JSON_TIMEOUT_MS)
    })
  } catch (err) {
    setPhoneOnline(false)
    throw new PhoneClientError(`PHONE_UNREACHABLE:${(err as Error).message}`)
  }
  if (!res.ok) {
    let detail = ''
    try {
      detail = String(((await res.json()) as { error?: string }).error ?? '')
    } catch {
      /* body not JSON */
    }
    throw new PhoneClientError(`PHONE_API_${res.status}${detail ? `:${detail}` : ''}`, res.status)
  }
  return (await res.json()) as T
}

// ---- API surface ---------------------------------------------------------

export function listTracks(): Promise<PhoneTrackInfo[]> {
  return apiJson<PhoneTrackInfo[]>('/api/tracks', { timeoutMs: 60_000 })
}

export function getHash(id: number): Promise<{ sha256: string; size: number; mtimeMs: number }> {
  // Streaming sha256 of a big FLAC on the phone can take a while.
  return apiJson('/api/hash/' + id, { timeoutMs: 120_000 })
}

export function commitTrack(
  id: number,
  body: { uploadId: string; sha256: string; ext: string }
): Promise<{ ok: boolean; trackId: number; path: string; changedExt: boolean }> {
  return apiJson('/api/commit/' + id, { method: 'POST', body, timeoutMs: 180_000 })
}

export function deleteUpload(id: number): Promise<{ removed: number }> {
  return apiJson('/api/upload/' + id, { method: 'DELETE' })
}

export function sessionStart(): Promise<{ ok: boolean }> {
  return apiJson('/api/session/start', { method: 'POST', body: {} })
}

export function sessionHeartbeat(done?: number, total?: number): Promise<{ ok: boolean }> {
  return apiJson('/api/session/heartbeat', { method: 'POST', body: { done, total } })
}

export async function sessionEnd(): Promise<void> {
  try {
    await apiJson('/api/session/end', { method: 'POST', body: {} })
  } catch (err) {
    logWarn('phoneSync', 'session end fallito (ignorato)', err)
  }
}

/** Streams GET /api/file/:id into destPath. Rejects on abort/HTTP/stream error. */
export function downloadFile(id: number, destPath: string, signal: AbortSignal): Promise<void> {
  const { host, port } = requireEndpoint()
  return new Promise((resolve, reject) => {
    const req = request(
      { host, port, path: `/api/file/${id}`, method: 'GET', headers: authHeaders() },
      (res) => {
        if (res.statusCode !== 200) {
          res.resume()
          reject(new PhoneClientError(`PHONE_PULL_${res.statusCode}`, res.statusCode))
          return
        }
        const sink = createWriteStream(destPath)
        res.pipe(sink)
        res.on('error', (err) => {
          sink.destroy()
          reject(err)
        })
        sink.on('error', reject)
        sink.on('finish', resolve)
      }
    )
    // No fixed timeout on the transfer itself (files can be huge on slow WiFi);
    // a dead socket is caught by TCP + the idle timeout below.
    req.setTimeout(60_000, () => req.destroy(new PhoneClientError('PHONE_PULL_TIMEOUT')))
    signal.addEventListener('abort', () => req.destroy(new PhoneClientError('ABORTED')), {
      once: true
    })
    req.on('error', reject)
    req.end()
  })
}

/** Streams srcPath to POST /api/upload/:id; resolves with the staged uploadId. */
export async function uploadFile(
  id: number,
  srcPath: string,
  sha256: string,
  ext: string,
  signal: AbortSignal
): Promise<{ uploadId: string }> {
  const { host, port } = requireEndpoint()
  const size = (await stat(srcPath)).size
  return new Promise((resolve, reject) => {
    const req = request(
      {
        host,
        port,
        path: `/api/upload/${id}`,
        method: 'POST',
        headers: {
          ...authHeaders(),
          'Content-Type': 'application/octet-stream',
          'Content-Length': String(size),
          'X-Aether-Sha256': sha256,
          'X-Aether-Ext': ext
        }
      },
      (res) => {
        let raw = ''
        res.setEncoding('utf-8')
        res.on('data', (chunk: string) => {
          raw = (raw + chunk).slice(0, 4096)
        })
        res.on('end', () => {
          if (res.statusCode !== 200) {
            reject(new PhoneClientError(`PHONE_PUSH_${res.statusCode}:${raw}`, res.statusCode))
            return
          }
          try {
            const body = JSON.parse(raw) as { uploadId?: string }
            if (!body.uploadId) throw new Error('missing uploadId')
            resolve({ uploadId: body.uploadId })
          } catch {
            reject(new PhoneClientError('PHONE_PUSH_BAD_RESPONSE'))
          }
        })
        res.on('error', reject)
      }
    )
    req.setTimeout(60_000, () => req.destroy(new PhoneClientError('PHONE_PUSH_TIMEOUT')))
    signal.addEventListener('abort', () => req.destroy(new PhoneClientError('ABORTED')), {
      once: true
    })
    req.on('error', reject)
    const source = createReadStream(srcPath)
    source.on('error', (err) => req.destroy(err))
    source.pipe(req)
  })
}
