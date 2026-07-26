import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import { createReadStream, statSync } from 'node:fs'
import { extname } from 'node:path'
import { getDb } from '../electron/modules/db'
import { getCover } from '../electron/modules/coverArt'
import { proxyStream } from './proxyStream'

/**
 * Local media/cover server — the mobile replacement for the `aether://`
 * protocol handler (electron/main.ts:79-119). Runs on 127.0.0.1:<ephemeral>
 * inside nodejs-mobile. The renderer points mediaUrl()/coverUrl() at this
 * origin (see src/lib/format.ts) instead of aether://.
 *
 *   GET /media/:id        → audio stream with Range/206 support
 *   GET /art/:hash?thumb=1 → cover art (WebP)
 *   GET /remote?url=http…  → CORS/CSP proxy for remote preview images (buffered)
 *   GET /stream?url=http…  → streaming proxy for remote audio (podcast episodes,
 *                            external recs) with Range pass-through, so the WebView/
 *                            ExoPlayer only ever talk to 127.0.0.1 — http feeds are
 *                            no longer blocked by Android's cleartext policy.
 */

/**
 * Pipe a file stream to the response, terminating the response (rather than
 * crashing the whole nodejs-mobile process) if the read fails mid-stream — the
 * file can be moved/become unreadable after statSync succeeded.
 */
function pipeStream(stream: ReturnType<typeof createReadStream>, res: ServerResponse): void {
  stream.on('error', (err) => {
    console.error('media stream error', err)
    res.destroy(err)
  })
  stream.pipe(res)
}

const MIME: Record<string, string> = {
  '.mp3': 'audio/mpeg',
  '.flac': 'audio/flac',
  '.m4a': 'audio/mp4',
  '.aac': 'audio/aac',
  '.ogg': 'audio/ogg',
  '.opus': 'audio/ogg',
  '.wav': 'audio/wav',
  '.aiff': 'audio/aiff',
  '.aif': 'audio/aiff',
  '.wma': 'audio/x-ms-wma'
}

function streamFile(
  path: string,
  rangeHeader: string | undefined,
  res: ServerResponse,
  opts?: { mime?: string; cacheControl?: string }
): void {
  let size: number
  try {
    size = statSync(path).size
  } catch {
    res.writeHead(404).end('Not found')
    return
  }
  const mime = opts?.mime ?? MIME[extname(path).toLowerCase()] ?? 'application/octet-stream'
  const common: Record<string, string> = {
    'Accept-Ranges': 'bytes',
    'Content-Type': mime,
    'Access-Control-Allow-Origin': '*'
  }
  if (opts?.cacheControl) common['Cache-Control'] = opts.cacheControl

  if (rangeHeader) {
    const m = /bytes=(\d*)-(\d*)/.exec(rangeHeader)
    if (m) {
      let start: number
      let end: number
      if (m[1]) {
        // bytes=START-  or  bytes=START-END
        start = parseInt(m[1], 10)
        end = m[2] ? Math.min(parseInt(m[2], 10), size - 1) : size - 1
      } else if (m[2]) {
        // Suffix range: bytes=-N → the last N bytes (RFC 7233).
        start = Math.max(0, size - parseInt(m[2], 10))
        end = size - 1
      } else {
        // bytes=- with no numbers: unsatisfiable.
        res.writeHead(416, { 'Content-Range': `bytes */${size}` }).end()
        return
      }
      if (start <= end && start < size) {
        res.writeHead(206, {
          ...common,
          'Content-Range': `bytes ${start}-${end}/${size}`,
          'Content-Length': String(end - start + 1)
        })
        pipeStream(createReadStream(path, { start, end }), res)
        return
      }
      res.writeHead(416, { 'Content-Range': `bytes */${size}` }).end()
      return
    }
  }

  res.writeHead(200, { ...common, 'Content-Length': String(size) })
  pipeStream(createReadStream(path), res)
}

function handle(req: IncomingMessage, res: ServerResponse): void {
  try {
    const url = new URL(req.url ?? '/', 'http://127.0.0.1')
    const seg = url.pathname.split('/').filter(Boolean)

    if (seg[0] === 'media') {
      const trackId = Number(seg[1])
      const row = getDb().prepare('SELECT path FROM tracks WHERE id = ?').get(trackId) as
        | { path: string }
        | undefined
      if (!row) return void res.writeHead(404).end('Not found')
      return streamFile(row.path, req.headers.range, res)
    }

    if (seg[0] === 'art') {
      const hash = seg[1] ?? ''
      const thumb = url.searchParams.get('thumb') === '1'
      const cover = getCover(hash, thumb)
      if (!cover) return void res.writeHead(404).end('Not found')
      // Covers are immutable (keyed by content hash) → stream from file with a
      // long-lived cache header. streamFile adds Range support for free.
      return streamFile(cover.path, req.headers.range, res, {
        mime: cover.mime,
        cacheControl: 'max-age=31536000, immutable'
      })
    }

    if (seg[0] === 'stream') {
      const target = url.searchParams.get('url')
      if (!target || !/^https?:\/\//i.test(target)) return void res.writeHead(400).end('Bad request')
      return proxyStream(target, req, res)
    }

    if (seg[0] === 'remote') {
      const target = url.searchParams.get('url')
      if (!target || !/^https?:\/\//i.test(target)) return void res.writeHead(400).end('Bad request')
      // Same streaming path as /stream: piped with backpressure (the fetch
      // polyfill buffered whole bodies in memory) and covered by proxyStream's
      // upstream idle timeout, so a stalled CDN can't hang the request forever.
      return proxyStream(target, req, res)
    }

    res.writeHead(400).end('Bad request')
  } catch (err) {
    res.writeHead(500).end('Internal error')
    console.error('media server error', err)
  }
}

/** Start the server on an ephemeral loopback port. Resolves with the port. */
export function startMediaServer(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer(handle)
    server.on('error', reject)
    // 127.0.0.1 only — never expose on the network.
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address()
      if (addr && typeof addr === 'object') resolve(addr.port)
      else reject(new Error('failed to bind media server'))
    })
  })
}
