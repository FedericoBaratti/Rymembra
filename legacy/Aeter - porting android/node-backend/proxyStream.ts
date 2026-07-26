import { request as httpRequest, type IncomingMessage, type ServerResponse } from 'node:http'
import { request as httpsRequest } from 'node:https'
import { DEFAULT_USER_AGENT } from '../electron/modules/net/http'

/**
 * Streaming proxy for remote audio (podcast episodes, external recs) used by the
 * loopback media server (`GET /stream?url=…`). Forwards the client's Range
 * header, relays the upstream status/headers, and follows redirects. Uses
 * node:http/https directly (NOT fetch — the net-polyfill buffers into
 * arrayBuffer, fatal for a 100 MB episode) so the body is piped with
 * backpressure. The proxy runs in Node, which is not subject to Android's
 * cleartext policy, so http:// upstreams work too.
 *
 * Lives in its own module (no db.ts import chain) so it stays unit-testable on
 * machines where the native better-sqlite3 addon can't load.
 */

/**
 * Idle timeout for the upstream socket: a stalled origin (connected but sending
 * nothing) must not hang the client forever. It is a socket-idle timeout, so a
 * slow-but-alive stream is never killed. Env override exists for tests; read
 * lazily so tests can set it without an import-order dance.
 */
function upstreamIdleTimeoutMs(): number {
  return Number(process.env['AETHER_STREAM_TIMEOUT_MS'] || 30_000)
}

export function proxyStream(
  target: string,
  req: IncomingMessage,
  res: ServerResponse,
  hops = 0
): void {
  let parsed: URL
  try {
    parsed = new URL(target)
  } catch {
    res.writeHead(400).end('Bad request')
    return
  }
  const reqFn = parsed.protocol === 'https:' ? httpsRequest : httpRequest
  const headers: Record<string, string> = { 'User-Agent': DEFAULT_USER_AGENT, Accept: '*/*' }
  if (typeof req.headers.range === 'string') headers.Range = req.headers.range

  let timedOut = false
  const upstream = reqFn(parsed, { method: 'GET', headers }, (up) => {
    const status = up.statusCode ?? 502
    // Follow redirects (bounded) — enclosure URLs commonly bounce to a CDN.
    if (status >= 301 && status <= 308 && up.headers.location && hops < 5) {
      up.resume() // drain the redirect body
      proxyStream(new URL(up.headers.location, parsed).toString(), req, res, hops + 1)
      return
    }
    const out: Record<string, string> = { 'Access-Control-Allow-Origin': '*' }
    for (const h of ['content-type', 'content-length', 'content-range', 'accept-ranges', 'cache-control']) {
      const v = up.headers[h]
      if (typeof v === 'string') out[h] = v
    }
    res.writeHead(status, out)
    up.on('error', () => res.destroy())
    up.pipe(res)
  })
  upstream.setTimeout(upstreamIdleTimeoutMs(), () => {
    timedOut = true
    if (!res.headersSent) res.writeHead(504).end('Gateway timeout')
    else res.destroy()
    upstream.destroy() // also fires 'error' below — guarded by timedOut
  })
  upstream.on('error', () => {
    if (timedOut) return // already answered with 504
    if (!res.headersSent) res.writeHead(502).end('Bad gateway')
    else res.destroy()
  })
  // Client navigated away / stopped playback → tear down the upstream request.
  req.on('close', () => upstream.destroy())
  upstream.end()
}
