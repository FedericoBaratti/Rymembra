/**
 * Networking polyfills for nodejs-mobile (Node 12.19 / V8 7.8).
 *
 * Node 12 predates several globals our reused electron/modules networking code
 * relies on:
 *   - `fetch`                 (global fetch landed in Node 18)
 *   - `AbortController` / `AbortSignal`  (Node 15)
 *   - `AbortSignal.timeout`   (Node 17.3)
 *
 * `electron/modules/net/http.ts` does `fetch(url, { signal: AbortSignal.timeout(…) })`,
 * so on the device `AbortSignal.timeout` throws *before* fetch is even called;
 * the error is swallowed into a generic NetworkError, retried, and the circuit
 * breaker opens — which is why MusicBrainz/enrichment/cover-art-archive/Last.fm
 * all fail. `downloader.ts` also constructs `new AbortController()`.
 *
 * This module installs minimal, dependency-free implementations (only when the
 * global is missing) built on node:http/node:https. It MUST be imported first in
 * node-backend/main.ts so the globals exist before any other module loads or runs.
 *
 * It mirrors only the subset of the WHATWG fetch surface the app actually uses
 * (see electron/modules/net/http.ts and enrichment/services/coverart.ts,
 * spotify.ts): method/headers/string|Buffer body, redirects, an abort signal,
 * and a response with ok/status/statusText/headers.get/text/json/arrayBuffer.
 * It also advertises `Accept-Encoding` and transparently decompresses
 * gzip/deflate/br responses — core http/https never does this (only undici does),
 * so without it a compressed payload would reach .json()/.text() as raw bytes.
 */
import http from 'http'
import https from 'https'
import zlib from 'zlib'
import dns from 'dns'
import type { LookupOptions, LookupAddress } from 'dns'
import { URL } from 'url'

type AnyRecord = Record<string, unknown>
type HeaderInit = Record<string, string> | Array<[string, string]> | { forEach(cb: (v: string, k: string) => void): void }

const g = globalThis as unknown as AnyRecord

function makeError(name: string, message: string): Error {
  const err = new Error(message)
  err.name = name
  return err
}

// --- AbortController / AbortSignal -------------------------------------------

type AbortListener = (ev: { type: 'abort' }) => void

class PolyAbortSignal {
  aborted = false
  reason: unknown = undefined
  onabort: AbortListener | null = null
  private listeners: AbortListener[] = []

  addEventListener(type: string, cb: AbortListener): void {
    if (type === 'abort') this.listeners.push(cb)
  }

  removeEventListener(type: string, cb: AbortListener): void {
    if (type !== 'abort') return
    const i = this.listeners.indexOf(cb)
    if (i >= 0) this.listeners.splice(i, 1)
  }

  throwIfAborted(): void {
    if (this.aborted) throw this.reason
  }

  /** Internal: flip to aborted and notify listeners. */
  fire(reason: unknown): void {
    if (this.aborted) return
    this.aborted = true
    this.reason = reason
    const ev = { type: 'abort' as const }
    if (this.onabort) {
      try {
        this.onabort(ev)
      } catch {
        /* listener errors must not break the controller */
      }
    }
    for (const cb of this.listeners.slice()) {
      try {
        cb(ev)
      } catch {
        /* ignore */
      }
    }
  }
}

class PolyAbortController {
  readonly signal = new PolyAbortSignal()

  abort(reason?: unknown): void {
    this.signal.fire(reason !== undefined ? reason : makeError('AbortError', 'This operation was aborted'))
  }
}

/** AbortSignal.timeout(ms): aborts with a TimeoutError after `ms`. */
function timeoutSignal(ms: number): { signal: unknown } {
  const Ctrl = g.AbortController as new () => { signal: unknown; abort(reason?: unknown): void }
  const ctrl = new Ctrl()
  const t = setTimeout(() => ctrl.abort(makeError('TimeoutError', 'The operation timed out')), ms)
  if (t && typeof (t as { unref?: () => void }).unref === 'function') (t as { unref: () => void }).unref()
  return ctrl.signal as { signal: unknown }
}

function installAbort(): void {
  if (typeof g.AbortController === 'undefined') {
    g.AbortController = PolyAbortController
    g.AbortSignal = PolyAbortSignal
  }
  const Signal = g.AbortSignal as { timeout?: (ms: number) => unknown } | undefined
  if (Signal && typeof Signal.timeout !== 'function') {
    Signal.timeout = (ms: number) => timeoutSignal(ms)
  }
}

// --- fetch -------------------------------------------------------------------

interface MinimalSignal {
  aborted: boolean
  reason?: unknown
  addEventListener(type: string, cb: AbortListener): void
  removeEventListener?(type: string, cb: AbortListener): void
}

interface FetchInit {
  method?: string
  headers?: HeaderInit
  body?: string | Buffer | null
  signal?: MinimalSignal
  redirect?: 'follow' | 'manual' | 'error'
}

function normalizeHeaders(h: HeaderInit | undefined): Record<string, string> {
  const out: Record<string, string> = {}
  if (!h) return out
  if (Array.isArray(h)) {
    for (const pair of h) if (pair && pair.length >= 2) out[pair[0]] = pair[1]
    return out
  }
  if (typeof (h as { forEach?: unknown }).forEach === 'function') {
    ;(h as { forEach(cb: (v: string, k: string) => void): void }).forEach((v, k) => {
      out[k] = v
    })
    return out
  }
  for (const k of Object.keys(h)) out[k] = (h as Record<string, string>)[k]
  return out
}

function hasHeader(headers: Record<string, string>, name: string): boolean {
  const lower = name.toLowerCase()
  for (const k of Object.keys(headers)) if (k.toLowerCase() === lower) return true
  return false
}

function stripContentHeaders(headers: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {}
  for (const k of Object.keys(headers)) {
    const lower = k.toLowerCase()
    if (lower === 'content-length' || lower === 'content-type' || lower === 'content-encoding') continue
    out[k] = headers[k]
  }
  return out
}

/**
 * Decompresses a response body according to its `Content-Encoding`. Node's core
 * http/https — unlike the WHATWG fetch (undici) we emulate — never auto-inflates,
 * so without this every gzip/br/deflate body would reach `.json()`/`.text()` as
 * raw compressed bytes. All zlib calls below exist on Node 12 (brotli since 11.7).
 * Multiple stacked encodings (rare) are applied right-to-left. Unknown tokens and
 * decode failures fall back to the bytes as-is (= the pre-fix behaviour).
 */
function decompressOnce(buf: Buffer, enc: string): Buffer {
  switch (enc) {
    case 'gzip':
    case 'x-gzip':
      return zlib.gunzipSync(buf)
    case 'br':
      return zlib.brotliDecompressSync(buf)
    case 'deflate':
      // Standard `deflate` is zlib-wrapped, but some servers send raw DEFLATE.
      try {
        return zlib.inflateSync(buf)
      } catch {
        return zlib.inflateRawSync(buf)
      }
    default:
      return buf
  }
}

function decompress(buf: Buffer, contentEncoding: string): Buffer {
  const encodings = contentEncoding
    .split(',')
    .map((e) => e.trim().toLowerCase())
    .filter((e) => e && e !== 'identity')
  let out = buf
  for (let i = encodings.length - 1; i >= 0; i--) out = decompressOnce(out, encodings[i])
  return out
}

function makeResponse(
  status: number,
  statusText: string,
  rawHeaders: http.IncomingHttpHeaders,
  buf: Buffer,
  url: string
): unknown {
  const headers = {
    get(name: string): string | null {
      const v = rawHeaders[String(name).toLowerCase()]
      if (v == null) return null
      return Array.isArray(v) ? v.join(', ') : String(v)
    }
  }
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText,
    url,
    redirected: false,
    headers,
    async text(): Promise<string> {
      return buf.toString('utf8')
    },
    async json(): Promise<unknown> {
      return JSON.parse(buf.toString('utf8'))
    },
    async arrayBuffer(): Promise<ArrayBuffer> {
      return buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength) as ArrayBuffer
    }
  }
}

// --- DNS: IPv4-first lookup with a c-ares fallback --------------------------
//
// nodejs-mobile (Node 12 / bionic getaddrinfo) on Android returns ENOTFOUND for
// strongly dual-stack hosts (oauth2.googleapis.com, accounts.google.com) on an
// IPv4-only network path, even though IPv4-only hosts (musicbrainz.org,
// api.deezer.com …) resolve fine — which is why enrichment works but the Drive
// OAuth token exchange fails with `getaddrinfo ENOTFOUND oauth2.googleapis.com`.
// Force IPv4 for every request (the AAAA path is what fails); if getaddrinfo
// still fails, fall back to c-ares A-record resolution. Android has no
// /etc/resolv.conf, so seed public resolvers for that fallback only, and only
// when the current server list is empty/loopback — never disturbing the path
// that already works. All hosts the backend talks to have IPv4, and the local
// media server is 127.0.0.1, so forcing IPv4 is safe.

let publicResolversSeeded = false
function seedPublicResolvers(): void {
  if (publicResolversSeeded) return
  publicResolversSeeded = true
  try {
    const servers = dns.getServers()
    if (!servers.length || servers.every((s) => s === '127.0.0.1' || s === '::1')) {
      dns.setServers(['8.8.8.8', '1.1.1.1', '8.8.4.4'])
    }
  } catch {
    /* best effort — leave the default resolver in place */
  }
}

function ipv4Lookup(
  hostname: string,
  options: LookupOptions,
  callback: (err: NodeJS.ErrnoException | null, address: string | LookupAddress[], family?: number) => void
): void {
  const wantAll = options?.all === true
  dns.lookup(hostname, { family: 4 }, (err, address, family) => {
    if (!err) {
      callback(null, wantAll ? [{ address, family: family || 4 }] : address, family || 4)
      return
    }
    // getaddrinfo failed → try c-ares A records via public resolvers.
    seedPublicResolvers()
    dns.resolve4(hostname, (err2, addrs) => {
      if (!err2 && addrs && addrs.length) {
        callback(null, wantAll ? addrs.map((a) => ({ address: a, family: 4 })) : addrs[0], 4)
        return
      }
      callback(err, '') // surface the original getaddrinfo error
    })
  })
}

const REDIRECT_CODES = [301, 302, 303, 307, 308]
const MAX_REDIRECTS = 20

function requestWithRedirects(urlStr: string, init: FetchInit, redirectCount: number): Promise<unknown> {
  return new Promise((resolve, reject) => {
    let settled = false
    const finish = (fn: (v: unknown) => void, v: unknown): void => {
      if (settled) return
      settled = true
      fn(v)
    }

    let u: URL
    try {
      u = new URL(urlStr)
    } catch {
      finish(reject, new TypeError(`Invalid URL: ${urlStr}`))
      return
    }

    const signal = init.signal
    const abortError = (): Error =>
      (signal && (signal.reason as Error)) || makeError('AbortError', 'This operation was aborted')
    if (signal && signal.aborted) {
      finish(reject, abortError())
      return
    }

    const lib = u.protocol === 'http:' ? http : https
    const method = (init.method || 'GET').toUpperCase()
    const headers = normalizeHeaders(init.headers)

    // Match real fetch: advertise compression so large JSON payloads (e.g.
    // MusicBrainz searches) travel compressed on mobile. The response handler
    // decompresses by Content-Encoding. Only set it when the caller hasn't.
    if (!hasHeader(headers, 'accept-encoding')) headers['Accept-Encoding'] = 'gzip, deflate, br'

    let body = init.body
    if (body != null && typeof body !== 'string' && !Buffer.isBuffer(body)) body = String(body)
    if (body != null && !hasHeader(headers, 'content-length')) {
      headers['Content-Length'] = String(Buffer.byteLength(body as string | Buffer))
    }

    const req = lib.request(u, { method, headers, lookup: ipv4Lookup }, (res) => {
      const status = res.statusCode || 0
      const loc = res.headers['location']
      if (loc && REDIRECT_CODES.indexOf(status) >= 0 && init.redirect !== 'manual') {
        res.resume() // drain & discard the redirect body
        if (redirectCount >= MAX_REDIRECTS) {
          finish(reject, new Error(`Too many redirects (${urlStr})`))
          return
        }
        let nextUrl: string
        try {
          nextUrl = new URL(loc, u).toString()
        } catch {
          finish(reject, new Error(`Invalid redirect location from ${urlStr}`))
          return
        }
        // Like the real fetch: drop sensitive headers when redirecting to a
        // different origin, so an Authorization/Cookie isn't leaked to the
        // redirect target (e.g. a CDN the auth'd host points us at).
        let crossOrigin: boolean
        try {
          crossOrigin = new URL(nextUrl).origin !== u.origin
        } catch {
          crossOrigin = true
        }
        let nextHeaders = headers
        if (crossOrigin) {
          nextHeaders = {}
          for (const k of Object.keys(headers)) {
            const lk = k.toLowerCase()
            if (lk === 'authorization' || lk === 'cookie' || lk === 'proxy-authorization') continue
            nextHeaders[k] = headers[k]
          }
        }
        const nextInit: FetchInit = { ...init, headers: nextHeaders }
        // 303 always becomes GET; 301/302 turn POST into GET (matches browsers).
        if (status === 303 || ((status === 301 || status === 302) && method === 'POST')) {
          nextInit.method = 'GET'
          nextInit.body = null
          nextInit.headers = stripContentHeaders(nextHeaders)
        }
        if (!settled) {
          settled = true
          requestWithRedirects(nextUrl, nextInit, redirectCount + 1).then(resolve, reject)
        }
        return
      }

      const chunks: Buffer[] = []
      res.on('data', (c: Buffer) => chunks.push(c))
      res.on('end', () => {
        const raw = Buffer.concat(chunks)
        const enc = String(res.headers['content-encoding'] || '').toLowerCase().trim()
        let body: Buffer = raw
        let headers = res.headers
        if (enc && enc !== 'identity') {
          try {
            body = decompress(raw, enc)
            // Like the real fetch: once decoded, these headers no longer
            // describe the body the consumer sees.
            headers = { ...res.headers }
            delete headers['content-encoding']
            delete headers['content-length']
          } catch {
            /* truncated/corrupt → fall back to the raw bytes (pre-fix behaviour) */
          }
        }
        finish(resolve, makeResponse(status, res.statusMessage || '', headers, body, u.toString()))
      })
      res.on('error', (e: Error) => finish(reject, e))
    })

    req.on('error', (e: Error) => finish(reject, e))

    if (signal) {
      signal.addEventListener('abort', () => {
        try {
          req.destroy(abortError())
        } catch {
          /* already destroyed */
        }
        finish(reject, abortError())
      })
    }

    if (body != null) req.write(body)
    req.end()
  })
}

export function polyFetch(input: unknown, init?: FetchInit): Promise<unknown> {
  const urlStr =
    typeof input === 'string'
      ? input
      : input && typeof (input as { url?: unknown }).url === 'string'
        ? (input as { url: string }).url
        : String(input)
  return requestWithRedirects(urlStr, init || {}, 0)
}

function installFetch(): void {
  if (typeof g.fetch !== 'function') g.fetch = polyFetch
}

// Install immediately on import (side-effecting module).
installAbort()
installFetch()
