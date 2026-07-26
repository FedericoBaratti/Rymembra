import type { IncomingMessage, ServerResponse } from 'node:http'
import { createReadStream, statSync } from 'node:fs'
import { resolveRange, mimeForPath } from '../httpRange'
import { getCover } from '../coverArt'
import { getTrackById } from '../libraryQueries'
import { logError } from '../logger'
import { CORS_HEADERS } from './httpJson'

/** Pipe a file stream to the response, ending the response (not crashing the
    LAN server) if the read fails mid-stream. */
function pipeStream(stream: ReturnType<typeof createReadStream>, res: ServerResponse): void {
  stream.on('error', (err) => {
    logError('lan', 'media stream error', err)
    res.destroy(err)
  })
  stream.pipe(res)
}

function streamFileToResponse(
  req: IncomingMessage,
  path: string,
  res: ServerResponse,
  opts?: { mime?: string; cacheControl?: string }
): void {
  let size: number
  try {
    size = statSync(path).size
  } catch {
    res.writeHead(404, CORS_HEADERS).end('Not found')
    return
  }
  const mime = opts?.mime ?? mimeForPath(path)
  const common: Record<string, string> = {
    ...CORS_HEADERS,
    'Accept-Ranges': 'bytes',
    'Content-Type': mime
  }
  if (opts?.cacheControl) common['Cache-Control'] = opts.cacheControl

  const range = resolveRange(req.headers.range, size)
  if (range.status === 416) {
    res.writeHead(416, { ...common, ...range.headers }).end()
    return
  }
  res.writeHead(range.status, { ...common, ...range.headers })
  // HEAD probes (some native players send one before streaming) get the exact
  // GET headers — Content-Length/Range included — with no body.
  if (req.method === 'HEAD') {
    res.end()
    return
  }
  pipeStream(createReadStream(path, { start: range.start, end: range.end }), res)
}

export function handleMediaRoute(req: IncomingMessage, res: ServerResponse, trackIdParam: string): void {
  const track = getTrackById(Number(trackIdParam))
  if (!track) {
    res.writeHead(404, CORS_HEADERS).end('Not found')
    return
  }
  streamFileToResponse(req, track.path, res)
}

export function handleArtRoute(
  req: IncomingMessage,
  res: ServerResponse,
  hash: string,
  thumb: boolean
): void {
  const cover = getCover(hash, thumb)
  if (!cover) {
    res.writeHead(404, CORS_HEADERS).end('Not found')
    return
  }
  streamFileToResponse(req, cover.path, res, {
    mime: cover.mime,
    cacheControl: 'max-age=31536000, immutable'
  })
}
