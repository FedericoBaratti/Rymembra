import { getSettings } from '../../settings'
import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { fetchJson } from '../../net/http'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'
import { logWarn } from '../../logger'
import { normalizeForMatch } from '../match'
import { LastfmTopTagsSchema } from '../schemas'

const limiter = new RateLimiter({ name: 'lastfm', minIntervalMs: 250 })
const breaker = new CircuitBreaker({ name: 'Last.fm' })

/** Top tag for a track (used as genre). Soft-fails to null. */
export async function fetchLastfmGenre(artist: string, title: string): Promise<string | null> {
  const apiKey = getSettings().lastfmApiKey
  if (!apiKey) return null
  try {
    return await getApiCache().cachedJson({
      service: 'lastfm-tags',
      key: `${normalizeForMatch(artist)}|${normalizeForMatch(title)}`,
      ttlMs: TTL.LASTFM,
      missTtlMs: TTL.NEGATIVE,
      fetcher: async () => {
        const url = `https://ws.audioscrobbler.com/2.0/?method=track.gettoptags&artist=${encodeURIComponent(artist)}&track=${encodeURIComponent(title)}&api_key=${apiKey}&format=json`
        const data = await withRetry(
          () => breaker.exec(() => limiter.schedule(() => fetchJson(url, { schema: LastfmTopTagsSchema }))),
          { retries: 2 }
        )
        return data.toptags?.tag?.[0]?.name ?? null
      }
    })
  } catch (err) {
    logWarn('enrich', `Last.fm non raggiungibile (${artist} — ${title})`, err)
    return null
  }
}
