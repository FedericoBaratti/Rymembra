import { extname } from 'node:path'

/**
 * HTTP Range (RFC 7233) resolution, shared by the `aether://` protocol handler
 * (electron/main.ts) and the LAN media routes (electron/modules/lan/mediaRoutes.ts).
 * Framework-agnostic: callers translate the result into a Fetch Response or a
 * Node ServerResponse as needed.
 */

export const MEDIA_MIME: Record<string, string> = {
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

export function mimeForPath(path: string, fallback = 'application/octet-stream'): string {
  return MEDIA_MIME[extname(path).toLowerCase()] ?? fallback
}

export type RangeResolution =
  | { status: 200; start: number; end: number; headers: Record<string, string> }
  | { status: 206; start: number; end: number; headers: Record<string, string> }
  | { status: 416; headers: Record<string, string> }

/**
 * Resolve a `Range` header against a resource of `size` bytes. Supports
 * `bytes=START-END`, `bytes=START-` (open-ended) and `bytes=-N` (suffix range,
 * the last N bytes) per RFC 7233. No/unparseable Range → a plain 200.
 */
export function resolveRange(rangeHeader: string | null | undefined, size: number): RangeResolution {
  if (!rangeHeader) {
    return { status: 200, start: 0, end: size - 1, headers: { 'Content-Length': String(size) } }
  }
  const m = /bytes=(\d*)-(\d*)/.exec(rangeHeader)
  if (!m) {
    return { status: 200, start: 0, end: size - 1, headers: { 'Content-Length': String(size) } }
  }

  let start: number
  let end: number
  if (m[1]) {
    // bytes=START-  or  bytes=START-END
    start = parseInt(m[1], 10)
    end = m[2] ? Math.min(parseInt(m[2], 10), size - 1) : size - 1
  } else if (m[2]) {
    // Suffix range: bytes=-N → the last N bytes.
    start = Math.max(0, size - parseInt(m[2], 10))
    end = size - 1
  } else {
    // bytes=- with no numbers: unsatisfiable.
    return { status: 416, headers: { 'Content-Range': `bytes */${size}` } }
  }

  if (start <= end && start < size) {
    return {
      status: 206,
      start,
      end,
      headers: {
        'Content-Range': `bytes ${start}-${end}/${size}`,
        'Content-Length': String(end - start + 1)
      }
    }
  }
  return { status: 416, headers: { 'Content-Range': `bytes */${size}` } }
}
