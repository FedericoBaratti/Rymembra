import { z } from 'zod'
import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { fetchJson } from '../../net/http'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'
import { cleanQueryText, normalizeForMatch, type MetaCandidate } from '../match'

// iTunes Search API is keyless and returns authoritative title/artist/album/
// genre/duration plus a high-quality square artwork — ideal on Android where
// AcoustID/Last.fm keys and fpcalc are absent. ~20 req/min soft limit.
const limiter = new RateLimiter({ name: 'itunes', minIntervalMs: 350, maxConcurrent: 2 })
const breaker = new CircuitBreaker({ name: 'iTunes' })

const UNKNOWN_ARTIST = 'Artista sconosciuto'

const ItunesSchema = z.object({
  results: z
    .array(
      z.object({
        trackName: z.string().optional(),
        artistName: z.string().optional(),
        collectionName: z.string().optional(),
        primaryGenreName: z.string().optional(),
        releaseDate: z.string().optional(),
        trackTimeMillis: z.number().optional(),
        artworkUrl100: z.string().optional()
      })
    )
    .optional()
})

/** Artwork URLs end with /<W>x<H>bb.jpg; request a larger square (600px). */
function upscaleArtwork(url: string): string {
  return url.replace(/\/\d+x\d+bb\.(jpg|png|jpeg)/i, '/600x600bb.$1')
}

/** Searches iTunes for a track. Throws on network failure (so misses aren't cached); returns [] on empty. */
export async function itunesSearch(title: string, artist: string): Promise<MetaCandidate[]> {
  const knownArtist = artist && artist !== UNKNOWN_ARTIST ? cleanQueryText(artist) : ''
  const term = `${knownArtist} ${cleanQueryText(title)}`.replace(/\s+/g, ' ').trim()
  if (!term) return []
  const result = await getApiCache().cachedJson<MetaCandidate[]>({
    service: 'itunes-search',
    key: normalizeForMatch(term),
    ttlMs: TTL.MB_SEARCH,
    missTtlMs: TTL.NEGATIVE,
    fetcher: async () => {
      const url = `https://itunes.apple.com/search?term=${encodeURIComponent(term)}&entity=song&media=music&limit=10`
      const data = await withRetry(
        () => breaker.exec(() => limiter.schedule(() => fetchJson(url, { schema: ItunesSchema }))),
        { retries: 2 }
      )
      const out: MetaCandidate[] = []
      for (const r of data.results ?? []) {
        if (!r.trackName || !r.artistName) continue
        out.push({
          source: 'itunes',
          title: r.trackName,
          artist: r.artistName,
          album: r.collectionName ?? null,
          year: r.releaseDate ? Number(r.releaseDate.slice(0, 4)) || null : null,
          genre: r.primaryGenreName ?? null,
          durationMs: r.trackTimeMillis ?? null,
          coverUrl: r.artworkUrl100 ? upscaleArtwork(r.artworkUrl100) : null
        })
      }
      return out
    }
  })
  return result ?? []
}
