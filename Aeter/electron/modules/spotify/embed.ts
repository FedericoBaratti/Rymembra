import type { SpotifyResolved, SpotifyTrack } from './types'

// Pure parsing of Spotify's public embed page (__NEXT_DATA__) — the keyless,
// tokenless fallback. No electron imports → unit-tested. Node-12/small-ICU safe:
// the JSON is sliced with plain indexOf (no \p{} Unicode-property regex).

/** Slice the JSON out of the embed page's __NEXT_DATA__ script tag. */
export function extractNextData(html: string): unknown | null {
  const marker = '__NEXT_DATA__'
  const at = html.indexOf(marker)
  if (at < 0) return null
  const open = html.indexOf('>', at)
  if (open < 0) return null
  const close = html.indexOf('</script>', open)
  if (close < 0) return null
  const json = html.slice(open + 1, close).trim()
  try {
    return JSON.parse(json)
  } catch {
    return null
  }
}

/** Depth-first search for the first non-empty array under a `trackList` key. */
function findTrackList(node: unknown, depth = 0): Record<string, unknown>[] | null {
  if (depth > 8 || node === null || typeof node !== 'object') return null
  const obj = node as Record<string, unknown>
  const tl = obj['trackList']
  if (Array.isArray(tl) && tl.length > 0 && typeof tl[0] === 'object') {
    return tl as Record<string, unknown>[]
  }
  for (const key of Object.keys(obj)) {
    const found = findTrackList(obj[key], depth + 1)
    if (found) return found
  }
  return null
}

/** Depth-first search for the entity carrying the container name + cover. */
function findEntity(node: unknown, depth = 0): Record<string, unknown> | null {
  if (depth > 8 || node === null || typeof node !== 'object') return null
  const obj = node as Record<string, unknown>
  if (typeof obj['name'] === 'string' && (obj['coverArt'] || obj['trackList'])) return obj
  for (const key of Object.keys(obj)) {
    const found = findEntity(obj[key], depth + 1)
    if (found) return found
  }
  return null
}

function coverFromArt(art: unknown): string | null {
  if (!art || typeof art !== 'object') return null
  const sources = (art as { sources?: { url?: string }[] }).sources
  if (!sources || sources.length === 0) return null
  return sources[sources.length - 1]?.url ?? sources[0]?.url ?? null
}

/** Parse embed page HTML into a resolved entity (keyless, no token). */
export function parseEmbedHtml(html: string, kind: SpotifyResolved['kind']): SpotifyResolved | null {
  const data = extractNextData(html)
  if (!data) return null
  const entity = findEntity(data)
  const rawList = findTrackList(data) ?? []
  const cover = coverFromArt(entity?.['coverArt'])
  const title = entity ? String(entity['name'] ?? '') : ''
  // For an album/playlist embed, the container name is the album name and its
  // subtitle is the album artist. Carrying these onto every track keeps the
  // tokenless fallback from splitting an album (each track had album:null before).
  const albumName = kind === 'album' && title ? title : null
  const albumArtist =
    kind === 'album' && entity && entity['subtitle']
      ? String(entity['subtitle']).trim() || null
      : null
  const tracks: SpotifyTrack[] = rawList.map((it) => {
    const durMs =
      typeof it['duration'] === 'number'
        ? (it['duration'] as number)
        : ((it['duration'] as { totalMilliseconds?: number } | undefined)?.totalMilliseconds ?? null)
    const trackArtist = it['subtitle'] ? String(it['subtitle']).trim() : null
    return {
      title: String(it['title'] ?? it['name'] ?? '').trim(),
      artist: trackArtist,
      album: albumName,
      albumArtist: albumArtist ?? trackArtist,
      discNumber: null,
      trackNumber: null,
      year: null,
      durationMs: durMs,
      coverUrl: coverFromArt(it['coverArt'])
    }
  })
  if (!title && tracks.length === 0) return null
  return {
    kind,
    title: title || tracks[0]?.title || 'Spotify',
    artist: tracks.length === 1 ? tracks[0].artist : null,
    coverUrl: cover,
    tracks:
      tracks.length > 0
        ? tracks
        : title
          ? [{ title, artist: null, album: null, durationMs: null, coverUrl: cover }]
          : []
  }
}
