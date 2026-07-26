import { z } from 'zod'
import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { fetchJson } from '../../net/http'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'
import { cleanQueryText, normalizeForMatch, type MetaCandidate } from '../match'

// Deezer's search endpoint is keyless and returns album.cover_xl (1000px) plus
// title/artist/duration — a strong second opinion alongside iTunes/MusicBrainz.
const limiter = new RateLimiter({ name: 'deezer', minIntervalMs: 220, maxConcurrent: 2 })
const breaker = new CircuitBreaker({ name: 'Deezer' })

const UNKNOWN_ARTIST = 'Artista sconosciuto'

const DeezerSchema = z.object({
  data: z
    .array(
      z.object({
        title: z.string().optional(),
        duration: z.number().optional(),
        artist: z.object({ name: z.string().optional() }).optional(),
        album: z
          .object({
            title: z.string().optional(),
            cover_xl: z.string().nullish(),
            cover_big: z.string().nullish()
          })
          .optional()
      })
    )
    .optional()
})

/** Searches Deezer for a track. Throws on network failure; returns [] on empty. */
export async function deezerSearch(title: string, artist: string): Promise<MetaCandidate[]> {
  const knownArtist = artist && artist !== UNKNOWN_ARTIST ? cleanQueryText(artist) : ''
  const term = `${knownArtist} ${cleanQueryText(title)}`.replace(/\s+/g, ' ').trim()
  if (!term) return []
  const result = await getApiCache().cachedJson<MetaCandidate[]>({
    service: 'deezer-search',
    key: normalizeForMatch(term),
    ttlMs: TTL.MB_SEARCH,
    missTtlMs: TTL.NEGATIVE,
    fetcher: async () => {
      const url = `https://api.deezer.com/search?q=${encodeURIComponent(term)}&limit=10`
      const data = await withRetry(
        () => breaker.exec(() => limiter.schedule(() => fetchJson(url, { schema: DeezerSchema }))),
        { retries: 2 }
      )
      const out: MetaCandidate[] = []
      for (const r of data.data ?? []) {
        if (!r.title || !r.artist?.name) continue
        out.push({
          source: 'deezer',
          title: r.title,
          artist: r.artist.name,
          album: r.album?.title ?? null,
          durationMs: r.duration ? r.duration * 1000 : null,
          coverUrl: r.album?.cover_xl ?? r.album?.cover_big ?? null
        })
      }
      return out
    }
  })
  return result ?? []
}
