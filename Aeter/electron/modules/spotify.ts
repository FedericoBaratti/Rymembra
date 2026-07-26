import { getSettings } from './settings'
import { RateLimitError } from './net/errors'
import { parseRetryAfter, fetchWithTimeout } from './net/http'

let token: { value: string; expiresAt: number } | null = null

async function getToken(): Promise<string | null> {
  const { spotifyClientId, spotifyClientSecret } = getSettings()
  if (!spotifyClientId || !spotifyClientSecret) return null
  if (token && token.expiresAt > Date.now() + 10_000) return token.value

  const res = await fetchWithTimeout('https://accounts.spotify.com/api/token', {
    init: {
      method: 'POST',
      headers: {
        'Content-Type': 'application/x-www-form-urlencoded',
        Authorization:
          'Basic ' + Buffer.from(`${spotifyClientId}:${spotifyClientSecret}`).toString('base64')
      },
      body: 'grant_type=client_credentials'
    }
  })
  if (!res.ok) throw new Error(`SPOTIFY_AUTH_FAILED:${res.status}`)
  const data = (await res.json()) as { access_token: string; expires_in: number }
  token = { value: data.access_token, expiresAt: Date.now() + data.expires_in * 1000 }
  return token.value
}

async function api<T>(path: string): Promise<T | null> {
  const tk = await getToken()
  if (!tk) return null
  const res = await fetchWithTimeout(`https://api.spotify.com/v1/${path}`, {
    init: { headers: { Authorization: `Bearer ${tk}` } }
  })
  if (res.status === 429) {
    const err = new RateLimitError(path, parseRetryAfter(res.headers.get('retry-after')))
    // typed for retry/breaker logic, Italian message for the renderer
    err.message = 'Spotify rate limit raggiunto, riprova tra qualche secondo.'
    throw err
  }
  if (!res.ok) throw new Error(`Spotify API: ${res.status}`)
  return (await res.json()) as T
}

export interface SpotifyEntityInfo {
  title: string
  artist: string | null
  album: string | null
  coverUrl: string | null
  totalTracks: number
  durationMs: number | null
}

interface Img {
  url: string
}

/** Fetch display metadata for a Spotify entity. Falls back to the public oEmbed endpoint when no API credentials are configured. */
export async function getSpotifyInfo(
  kind: 'track' | 'album' | 'artist' | 'playlist',
  id: string,
  url: string
): Promise<SpotifyEntityInfo> {
  if (kind === 'track') {
    const tr = await api<{
      name: string
      artists: { name: string }[]
      album: { name: string; images: Img[] }
      duration_ms: number
    }>(`tracks/${id}`)
    if (tr) {
      return {
        title: tr.name,
        artist: tr.artists.map((a) => a.name).join(', '),
        album: tr.album.name,
        coverUrl: tr.album.images[0]?.url ?? null,
        totalTracks: 1,
        durationMs: tr.duration_ms
      }
    }
  } else if (kind === 'album') {
    const al = await api<{
      name: string
      artists: { name: string }[]
      images: Img[]
      total_tracks: number
    }>(`albums/${id}`)
    if (al) {
      return {
        title: al.name,
        artist: al.artists.map((a) => a.name).join(', '),
        album: al.name,
        coverUrl: al.images[0]?.url ?? null,
        totalTracks: al.total_tracks,
        durationMs: null
      }
    }
  } else if (kind === 'playlist') {
    const pl = await api<{
      name: string
      owner: { display_name: string }
      images: Img[]
      tracks: { total: number }
    }>(`playlists/${id}`)
    if (pl) {
      return {
        title: pl.name,
        artist: pl.owner.display_name,
        album: null,
        coverUrl: pl.images?.[0]?.url ?? null,
        totalTracks: pl.tracks.total,
        durationMs: null
      }
    }
  } else if (kind === 'artist') {
    const ar = await api<{ name: string; images: Img[] }>(`artists/${id}`)
    if (ar) {
      // count their albums for the preview
      const albums = await api<{ total: number }>(
        `artists/${id}/albums?include_groups=album,single&limit=1`
      )
      return {
        title: ar.name,
        artist: ar.name,
        album: null,
        coverUrl: ar.images[0]?.url ?? null,
        totalTracks: albums?.total ?? 0,
        durationMs: null
      }
    }
  }

  // no credentials — public oEmbed fallback (title + thumbnail only)
  const res = await fetchWithTimeout(
    `https://open.spotify.com/oembed?url=${encodeURIComponent(url)}`
  )
  if (!res.ok) {
    throw new Error(
      'Impossibile leggere i metadati Spotify. Aggiungi le credenziali API in Impostazioni → Integrazioni.'
    )
  }
  const o = (await res.json()) as { title: string; thumbnail_url?: string }
  return {
    title: o.title,
    artist: null,
    album: null,
    coverUrl: o.thumbnail_url ?? null,
    totalTracks: kind === 'track' ? 1 : 0,
    durationMs: null
  }
}
