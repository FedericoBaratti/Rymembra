export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00'
  const s = Math.floor(seconds % 60)
  const m = Math.floor((seconds / 60) % 60)
  const h = Math.floor(seconds / 3600)
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  return `${m}:${String(s).padStart(2, '0')}`
}

export function formatLongDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600)
  const m = Math.floor((seconds / 60) % 60)
  if (h > 0) return `${h} h ${m} min`
  return `${m} min`
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB']
  let v = bytes
  let i = -1
  do {
    v /= 1024
    i++
  } while (v >= 1024 && i < units.length - 1)
  return `${v.toFixed(1)} ${units[i]}`
}

// Base origin for media/cover/remote URLs. Desktop keeps the aether:// custom
// protocol; the mobile bridge overrides this with the local server origin —
// either the loopback nodejs-mobile server (http://127.0.0.1:<port>/) via
// setMediaBase(), or the desktop's LAN server (http://<lan-ip>:<port>/) in
// thin-client mode (src/lib/lanClient.ts).
let mediaBase = 'aether://'

export function setMediaBase(base: string): void {
  mediaBase = base.endsWith('/') ? base : `${base}/`
}

// Bearer token appended as ?token= to the two binary routes, which are loaded
// via plain <audio>/<img> tags that can't set an Authorization header. Unused
// (null) against the loopback nodejs-mobile server, which needs no auth.
let mediaToken: string | null = null

export function setMediaToken(token: string | null): void {
  mediaToken = token
}

function withToken(url: string): string {
  if (!mediaToken) return url
  return `${url}${url.includes('?') ? '&' : '?'}token=${encodeURIComponent(mediaToken)}`
}

export function coverUrl(hash: string | null | undefined, thumb = false): string | null {
  if (!hash) return null
  return withToken(`${mediaBase}art/${hash}${thumb ? '?thumb=1' : ''}`)
}

export function mediaUrl(trackId: number): string {
  return withToken(`${mediaBase}media/${trackId}`)
}

export function remoteImageUrl(url: string): string {
  // In LAN thin-client mode (mediaToken set) the desktop server has no /remote
  // proxy route — and external art is https anyway, so the WebView can load it
  // directly instead of 404ing against the desktop.
  if (mediaToken) return url
  return `${mediaBase}remote?url=${encodeURIComponent(url)}`
}

// Remote audio (podcast episodes, external recs) routed through the local media
// server so the WebView/ExoPlayer only ever hit 127.0.0.1 — this sidesteps
// Android's cleartext block on http:// feeds and gives Range/seek for free.
export function streamProxyUrl(url: string): string {
  return `${mediaBase}stream?url=${encodeURIComponent(url)}`
}
