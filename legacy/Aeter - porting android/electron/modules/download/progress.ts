// Pure stdout parsers for yt-dlp and spotdl. No electron imports (unit-tested).
//
// yt-dlp is invoked with sentinel templates (see sources/youtube.ts):
//   --progress-template "download:AETHER_P:<downloaded>/<total>"
//   --print "before_dl:AETHER_F:<index>/<total>:<title>"
//   --print "after_move:AETHER_D:<filepath>"
// The pre-sentinel regexes are kept as fallback so a yt-dlp version quirk
// degrades progress UX instead of breaking downloads.

export type YtdlpEvent =
  | { kind: 'progress'; downloadedBytes: number; totalBytes: number; fraction: number | null }
  | { kind: 'item'; index: number; total: number }
  | { kind: 'file-start'; index: number; total: number; title: string }
  | { kind: 'file-done'; path: string }
  | { kind: 'legacy-percent'; fraction: number }
  | { kind: 'destination'; file: string }

export function parseYtdlpLine(rawLine: string): YtdlpEvent | null {
  const line = rawLine.trim()
  if (!line) return null

  if (line.startsWith('AETHER_P:')) {
    const m = /^AETHER_P:(\d+|NA)\/(\d+|NA)$/.exec(line)
    if (!m) return null
    const downloaded = m[1] === 'NA' ? 0 : Number(m[1])
    const total = m[2] === 'NA' ? 0 : Number(m[2])
    return {
      kind: 'progress',
      downloadedBytes: downloaded,
      totalBytes: total,
      fraction: total > 0 ? Math.min(1, downloaded / total) : null
    }
  }

  if (line.startsWith('AETHER_F:')) {
    const m = /^AETHER_F:(\d+|NA)\/(\d+|NA):(.*)$/.exec(line)
    if (!m) return null
    return {
      kind: 'file-start',
      index: m[1] === 'NA' ? 1 : Number(m[1]),
      total: m[2] === 'NA' ? 1 : Number(m[2]),
      title: m[3].trim()
    }
  }

  if (line.startsWith('AETHER_D:')) {
    const path = line.slice('AETHER_D:'.length).trim()
    return path ? { kind: 'file-done', path } : null
  }

  // ---- legacy fallbacks (pre-sentinel yt-dlp output) ----

  const itemMatch = /\[download\] Downloading item (\d+) of (\d+)/.exec(line)
  if (itemMatch) {
    return { kind: 'item', index: Number(itemMatch[1]), total: Number(itemMatch[2]) }
  }

  const pct = /^\[download\]\s+([\d.]+)%/.exec(line)
  if (pct) {
    return { kind: 'legacy-percent', fraction: Math.min(1, Number(pct[1]) / 100) }
  }

  const dest = /Destination:\s+(.+)$/.exec(line)
  if (dest) {
    const file = dest[1].split(/[\\/]/).pop() ?? dest[1]
    return { kind: 'destination', file }
  }

  return null
}

export type SpotdlEvent =
  | { kind: 'downloaded'; title: string }
  | { kind: 'processing'; title: string }

const ANSI = new RegExp('\\u001b?\\[[0-9;]*m', 'g')

export function parseSpotdlLine(rawLine: string): SpotdlEvent | null {
  const line = rawLine.replace(ANSI, '').trim()
  if (!line) return null

  const downloaded = /Downloaded\s+"(.+?)"/.exec(line)
  if (downloaded) return { kind: 'downloaded', title: downloaded[1] }

  const processing = /(?:Processing|Downloading)\s+"?([^"]+)"?/.exec(line)
  if (processing && !line.includes('query')) {
    return { kind: 'processing', title: processing[1].slice(0, 120) }
  }

  return null
}
