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

export function coverUrl(hash: string | null | undefined, thumb = false): string | null {
  if (!hash) return null
  return `aether://art/${hash}${thumb ? '?thumb=1' : ''}`
}

export function mediaUrl(trackId: number): string {
  return `aether://media/${trackId}`
}

/** Route a remote image (download previews, podcast/Spotify covers) through the
 *  aether:// protocol handler so a strict CSP doesn't block the external host. */
export function remoteImageUrl(url: string): string {
  return `aether://remote?url=${encodeURIComponent(url)}`
}
