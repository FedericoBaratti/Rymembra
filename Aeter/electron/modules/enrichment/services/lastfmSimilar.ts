import { getSettings } from '../../settings'
import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { fetchJson } from '../../net/http'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'
import { logWarn } from '../../logger'
import { normalizeForMatch } from '../match'
import { LastfmSimilarTracksSchema, LastfmSimilarArtistsSchema } from '../../reco/schemas'
import type { RawCandidate } from '../../reco/engine'

// Last.fm track.getSimilar / artist.getSimilar. Requires a free API key (no user
// auth); the key already lives in settings (lastfmApiKey, set in Integrations).
// Without a key these are no-ops returning [] so the keyless ListenBrainz + local
// paths still drive discovery.

const ROOT = 'https://ws.audioscrobbler.com/2.0/'
const limiter = new RateLimiter({ name: 'lastfm', minIntervalMs: 250 })
const breaker = new CircuitBreaker({ name: 'Last.fm' })

/** Tracks similar to (artist,title). Soft-fails to []. */
export async function fetchSimilarTracks(artist: string, title: string): Promise<RawCandidate[]> {
  const apiKey = getSettings().lastfmApiKey
  if (!apiKey || !artist || !title) return []
  try {
    const out = await getApiCache().cachedJson<RawCandidate[]>({
      service: 'lastfm-similar-track',
      key: `${normalizeForMatch(artist)}|${normalizeForMatch(title)}`,
      ttlMs: TTL.LASTFM,
      missTtlMs: TTL.NEGATIVE,
      fetcher: async () => {
        const url = `${ROOT}?method=track.getsimilar&artist=${encodeURIComponent(artist)}&track=${encodeURIComponent(title)}&api_key=${apiKey}&format=json&limit=50`
        const data = await withRetry(
          () => breaker.exec(() => limiter.schedule(() => fetchJson(url, { schema: LastfmSimilarTracksSchema }))),
          { retries: 2 }
        )
        return (data.similartracks?.track ?? []).map((tk) => ({
          mbid: tk.mbid ? tk.mbid.toLowerCase() : null,
          title: tk.name,
          artist: tk.artist?.name ?? null,
          score: tk.match ?? 0,
          source: 'lastfm' as const
        }))
      }
    })
    return out ?? []
  } catch (err) {
    logWarn('reco', `Last.fm getSimilar track non raggiungibile (${artist} — ${title})`, err)
    return []
  }
}

/** Artists similar to `artist`. Soft-fails to []. */
export async function fetchSimilarArtists(artist: string): Promise<RawCandidate[]> {
  const apiKey = getSettings().lastfmApiKey
  if (!apiKey || !artist) return []
  try {
    const out = await getApiCache().cachedJson<RawCandidate[]>({
      service: 'lastfm-similar-artist',
      key: normalizeForMatch(artist),
      ttlMs: TTL.LASTFM,
      missTtlMs: TTL.NEGATIVE,
      fetcher: async () => {
        const url = `${ROOT}?method=artist.getsimilar&artist=${encodeURIComponent(artist)}&api_key=${apiKey}&format=json&limit=50`
        const data = await withRetry(
          () => breaker.exec(() => limiter.schedule(() => fetchJson(url, { schema: LastfmSimilarArtistsSchema }))),
          { retries: 2 }
        )
        return (data.similarartists?.artist ?? []).map((ar) => ({
          mbid: null,
          title: null,
          artist: ar.name,
          score: ar.match ?? 0,
          source: 'lastfm' as const
        }))
      }
    })
    return out ?? []
  } catch (err) {
    logWarn('reco', `Last.fm getSimilar artist non raggiungibile (${artist})`, err)
    return []
  }
}
