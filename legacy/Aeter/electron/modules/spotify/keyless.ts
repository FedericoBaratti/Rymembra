import { getSettings } from '../settings'
import { logWarn } from '../logger'
import { SECRET_CIPHERS, generateTotp } from './totp'
import { parseEmbedHtml } from './embed'
import type { SpotifyPreview, SpotifyResolved, SpotifyTrack } from './types'
import type { ParsedUrl } from '../download/urlDetect'

export type { SpotifyPreview, SpotifyResolved, SpotifyTrack } from './types'

// 100% keyless Spotify metadata resolver (no Client ID/Secret required).
//
// Layered, with graceful degradation — the first layer that succeeds wins:
//   A. Anonymous Web-Player token (TOTP) → official REST api.spotify.com/v1 with
//      offset/limit pagination → reads WHOLE playlists/albums (any size).
//   B. Embed page __NEXT_DATA__ scraping (no token) → covers track/album/small
//      playlist when A is unavailable (tracklist may be capped by Spotify).
//   C. oEmbed (title + cover only) — last-resort so the preview still renders.
//
// Node-12 / nodejs-mobile safe: only fetch (polyfilled) + crypto, NO \p{} regex
// (small-ICU) — the embed JSON is sliced with plain indexOf, not Unicode props.

const UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36'

// ---------- anonymous token (TOTP) ----------

let cachedToken: { value: string; expiresAt: number } | null = null

async function getServerTimeMs(): Promise<number> {
  try {
    const res = await fetch('https://open.spotify.com/api/server-time', {
      headers: { 'User-Agent': UA, Origin: 'https://open.spotify.com' },
      signal: AbortSignal.timeout(15_000)
    })
    if (res.ok) {
      const data = (await res.json()) as { serverTime?: number }
      if (typeof data.serverTime === 'number') return data.serverTime * 1000
    }
    const dateHeader = res.headers.get('date')
    if (dateHeader) {
      const t = Date.parse(dateHeader)
      if (!Number.isNaN(t)) return t
    }
  } catch {
    /* fall through to local clock */
  }
  return Date.now()
}

/** Mint (and cache) an anonymous Web-Player bearer token. Returns null on failure. */
export async function getAnonToken(): Promise<string | null> {
  if (cachedToken && cachedToken.expiresAt > Date.now() + 15_000) return cachedToken.value

  const serverMs = await getServerTimeMs()
  const tsSec = Math.floor(serverMs / 1000)

  for (const secret of SECRET_CIPHERS) {
    const code = generateTotp(secret, serverMs)
    const params = new URLSearchParams({
      reason: 'init',
      productType: 'web-player',
      totp: code,
      totpServer: code,
      totpVer: String(secret.version),
      ts: String(tsSec)
    })
    for (const base of [
      'https://open.spotify.com/api/token',
      'https://open.spotify.com/get_access_token'
    ]) {
      try {
        const res = await fetch(`${base}?${params.toString()}`, {
          headers: {
            'User-Agent': UA,
            Origin: 'https://open.spotify.com',
            Referer: 'https://open.spotify.com/',
            'App-Platform': 'WebPlayer'
          },
          signal: AbortSignal.timeout(15_000)
        })
        if (!res.ok) continue
        const data = (await res.json()) as {
          accessToken?: string
          accessTokenExpirationTimestampMs?: number
        }
        if (data.accessToken) {
          cachedToken = {
            value: data.accessToken,
            expiresAt: data.accessTokenExpirationTimestampMs ?? Date.now() + 3_000_000
          }
          return cachedToken.value
        }
      } catch (err) {
        logWarn('spotify', `token endpoint ${base} fallito`, err)
      }
    }
  }
  return null
}

// ---------- official REST (with the anon token) ----------

interface SpImg {
  url: string
}
interface SpArtist {
  name: string
}

async function apiGet<T>(path: string, token: string): Promise<T | null> {
  const sep = path.includes('?') ? '&' : '?'
  const res = await fetch(`https://api.spotify.com/v1/${path}${sep}market=from_token`, {
    headers: { Authorization: `Bearer ${token}`, 'User-Agent': UA },
    signal: AbortSignal.timeout(15_000)
  })
  if (!res.ok) {
    if (res.status === 401) cachedToken = null // force a refresh next call
    return null
  }
  return (await res.json()) as T
}

interface SpTrackObj {
  name: string
  duration_ms: number
  external_ids?: { isrc?: string }
  artists: SpArtist[]
  track_number?: number
  disc_number?: number
  album?: { name?: string; images?: SpImg[]; artists?: SpArtist[]; release_date?: string }
}

/** Album-level context applied to every track of a container (album endpoint),
 *  where the per-track objects are simplified and carry no album info. */
interface AlbumCtx {
  name: string | null
  artist: string | null
  cover: string | null
  year: number | null
}

/** Parse a 4-digit year out of a Spotify `release_date` ("YYYY" | "YYYY-MM" | "YYYY-MM-DD"). */
function yearFromReleaseDate(date: string | null | undefined): number | null {
  if (!date) return null
  const y = Number(date.slice(0, 4))
  return Number.isFinite(y) && y > 0 ? y : null
}

function trackFromObj(t: SpTrackObj, ctx: AlbumCtx): SpotifyTrack {
  const trackArtist = t.artists.map((a) => a.name).join(', ') || null
  // Album artist: prefer the container's (album endpoint), else the track's own
  // album.artists (playlist/track endpoints carry full album objects), else the
  // track artist as a last resort so grouping stays stable.
  const albumArtist =
    ctx.artist ?? (t.album?.artists?.map((a) => a.name).join(', ') || null) ?? trackArtist
  return {
    title: t.name,
    artist: trackArtist,
    album: t.album?.name ?? ctx.name,
    albumArtist,
    discNumber: t.disc_number ?? null,
    trackNumber: t.track_number ?? null,
    year: ctx.year ?? yearFromReleaseDate(t.album?.release_date),
    durationMs: t.duration_ms ?? null,
    isrc: t.external_ids?.isrc ?? null,
    coverUrl: t.album?.images?.[0]?.url ?? ctx.cover
  }
}

/** Album context for endpoints whose tracks already carry a full `album` object
 *  (track/playlist/artist-top) — let trackFromObj derive everything per track. */
const NO_CTX: AlbumCtx = { name: null, artist: null, cover: null, year: null }

async function resolveViaRest(parsed: ParsedUrl, token: string): Promise<SpotifyResolved | null> {
  const id = parsed.spotifyId!
  const kind = parsed.spotifyKind!

  if (kind === 'track') {
    const t = await apiGet<SpTrackObj>(`tracks/${id}`, token)
    if (!t) return null
    const tr = trackFromObj(t, NO_CTX)
    return { kind, title: t.name, artist: tr.artist, coverUrl: tr.coverUrl ?? null, tracks: [tr] }
  }

  if (kind === 'album') {
    const al = await apiGet<{
      name: string
      artists: SpArtist[]
      images: SpImg[]
      release_date?: string
    }>(`albums/${id}`, token)
    if (!al) return null
    const cover = al.images?.[0]?.url ?? null
    const albumArtist = al.artists.map((a) => a.name).join(', ') || null
    // The album/tracks endpoint returns SIMPLIFIED tracks (no album object): the
    // album name/artist/cover/year MUST come from the album-level call above.
    const ctx: AlbumCtx = {
      name: al.name,
      artist: albumArtist,
      cover,
      year: yearFromReleaseDate(al.release_date)
    }
    const tracks: SpotifyTrack[] = []
    let offset = 0
    for (;;) {
      const page = await apiGet<{ items: SpTrackObj[]; next: string | null }>(
        `albums/${id}/tracks?limit=50&offset=${offset}`,
        token
      )
      if (!page) break
      for (const t of page.items) tracks.push(trackFromObj(t, ctx))
      if (!page.next || page.items.length === 0) break
      offset += page.items.length
    }
    return { kind, title: al.name, artist: albumArtist, coverUrl: cover, tracks }
  }

  if (kind === 'playlist') {
    const pl = await apiGet<{ name: string; owner?: { display_name?: string }; images?: SpImg[] }>(
      `playlists/${id}?fields=name,owner(display_name),images`,
      token
    )
    if (!pl) return null
    const tracks: SpotifyTrack[] = []
    let offset = 0
    for (;;) {
      const page = await apiGet<{ items: { track: SpTrackObj | null }[]; next: string | null }>(
        `playlists/${id}/tracks?limit=100&offset=${offset}&additional_types=track`,
        token
      )
      if (!page) break
      for (const it of page.items) if (it.track) tracks.push(trackFromObj(it.track, NO_CTX))
      if (!page.next || page.items.length === 0) break
      offset += page.items.length
    }
    return {
      kind,
      title: pl.name,
      artist: pl.owner?.display_name ?? null,
      coverUrl: pl.images?.[0]?.url ?? tracks[0]?.coverUrl ?? null,
      tracks
    }
  }

  // artist → top tracks (a sensible, keyless "migrate this artist" set)
  const ar = await apiGet<{ name: string; images: SpImg[] }>(`artists/${id}`, token)
  if (!ar) return null
  const top = await apiGet<{ tracks: SpTrackObj[] }>(`artists/${id}/top-tracks`, token)
  const tracks = (top?.tracks ?? []).map((t) => trackFromObj(t, NO_CTX))
  return { kind, title: ar.name, artist: ar.name, coverUrl: ar.images?.[0]?.url ?? null, tracks }
}

// ---------- embed __NEXT_DATA__ fallback (parsing lives in ./embed) ----------

async function resolveViaEmbed(parsed: ParsedUrl): Promise<SpotifyResolved | null> {
  try {
    const res = await fetch(
      `https://open.spotify.com/embed/${parsed.spotifyKind}/${parsed.spotifyId}`,
      { headers: { 'User-Agent': UA }, signal: AbortSignal.timeout(15_000) }
    )
    if (!res.ok) return null
    const html = await res.text()
    return parseEmbedHtml(html, parsed.spotifyKind!)
  } catch (err) {
    logWarn('spotify', 'embed fallito', err)
    return null
  }
}

// ---------- lightweight preview (count/title/cover, no full pagination) ----------

export async function previewSpotify(parsed: ParsedUrl): Promise<SpotifyPreview> {
  if (!parsed.spotifyKind || !parsed.spotifyId) throw new Error('SPOTIFY_BAD_URL')
  const id = parsed.spotifyId
  const kind = parsed.spotifyKind
  const token = await getAnonToken().catch(() => null)

  if (token) {
    try {
      if (kind === 'track') {
        const t = await apiGet<SpTrackObj>(`tracks/${id}`, token)
        if (t)
          return {
            title: t.name,
            artist: t.artists.map((a) => a.name).join(', ') || null,
            coverUrl: t.album?.images?.[0]?.url ?? null,
            totalTracks: 1,
            durationMs: t.duration_ms ?? null
          }
      } else if (kind === 'album') {
        const al = await apiGet<{ name: string; artists: SpArtist[]; images: SpImg[]; total_tracks: number }>(
          `albums/${id}`,
          token
        )
        if (al)
          return {
            title: al.name,
            artist: al.artists.map((a) => a.name).join(', ') || null,
            coverUrl: al.images?.[0]?.url ?? null,
            totalTracks: al.total_tracks,
            durationMs: null
          }
      } else if (kind === 'playlist') {
        const pl = await apiGet<{
          name: string
          owner?: { display_name?: string }
          images?: SpImg[]
          tracks?: { total?: number }
        }>(`playlists/${id}?fields=name,owner(display_name),images,tracks(total)`, token)
        if (pl)
          return {
            title: pl.name,
            artist: pl.owner?.display_name ?? null,
            coverUrl: pl.images?.[0]?.url ?? null,
            totalTracks: pl.tracks?.total ?? 0,
            durationMs: null
          }
      } else {
        const ar = await apiGet<{ name: string; images: SpImg[] }>(`artists/${id}`, token)
        const top = await apiGet<{ tracks: SpTrackObj[] }>(`artists/${id}/top-tracks`, token)
        if (ar)
          return {
            title: ar.name,
            artist: ar.name,
            coverUrl: ar.images?.[0]?.url ?? null,
            totalTracks: top?.tracks?.length ?? 0,
            durationMs: null
          }
      }
    } catch (err) {
      logWarn('spotify', 'preview REST fallita, passo a embed', err)
    }
  }

  const viaEmbed = await resolveViaEmbed(parsed)
  if (viaEmbed) {
    return {
      title: viaEmbed.title,
      artist: viaEmbed.artist,
      coverUrl: viaEmbed.coverUrl,
      totalTracks: viaEmbed.tracks.length || (kind === 'track' ? 1 : 0),
      durationMs: viaEmbed.tracks.length === 1 ? viaEmbed.tracks[0].durationMs : null
    }
  }

  throw new Error('SPOTIFY_RESOLVE_FAILED: impossibile leggere i metadati da Spotify.')
}

// ---------- public entry ----------

/**
 * Resolve a Spotify URL into a full normalized tracklist, keyless. Tries the
 * anonymous-token REST path first (whole playlists), then embed scraping. If a
 * Client ID/Secret happen to be saved in Settings they are NOT required, but the
 * REST path benefits from a healthy anon token regardless.
 */
export async function resolveSpotify(parsed: ParsedUrl): Promise<SpotifyResolved> {
  if (!parsed.spotifyKind || !parsed.spotifyId) throw new Error('SPOTIFY_BAD_URL')

  const token = await getAnonToken().catch(() => null)
  if (token) {
    try {
      const viaRest = await resolveViaRest(parsed, token)
      if (viaRest && viaRest.tracks.length > 0) return viaRest
    } catch (err) {
      logWarn('spotify', 'REST keyless fallito, passo a embed', err)
    }
  }

  const viaEmbed = await resolveViaEmbed(parsed)
  if (viaEmbed && (viaEmbed.tracks.length > 0 || viaEmbed.title)) return viaEmbed

  throw new Error(
    'SPOTIFY_RESOLVE_FAILED: impossibile leggere i brani da Spotify (link privato o non disponibile).'
  )
}

/** True if a user has Spotify API credentials saved (unused by keyless flow, kept for diagnostics). */
export function hasSpotifyCredentials(): boolean {
  const s = getSettings()
  return !!(s.spotifyClientId && s.spotifyClientSecret)
}
