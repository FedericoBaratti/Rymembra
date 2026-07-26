import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { fetchJson } from '../../net/http'
import { HttpError } from '../../net/errors'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'
import { logWarn } from '../../logger'
import { LrclibGetSchema } from '../schemas'

const limiter = new RateLimiter({ name: 'lrclib', minIntervalMs: 250 })
const breaker = new CircuitBreaker({ name: 'lrclib' })

export interface RemoteLyrics {
  synced: string | null
  plain: string | null
}

/** Fetches lyrics from lrclib.net. 404 (no lyrics) is a cached negative. Soft-fails to null. */
export async function fetchLyricsRemote(q: {
  artist: string
  title: string
  album: string
  duration: number
}): Promise<RemoteLyrics | null> {
  const key = `${q.artist}|${q.title}|${q.album}|${Math.round(q.duration)}`.toLowerCase()
  try {
    return await getApiCache().cachedJson({
      service: 'lrclib',
      key,
      ttlMs: TTL.LRCLIB,
      missTtlMs: TTL.NEGATIVE,
      fetcher: async () => {
        const params = new URLSearchParams({
          artist_name: q.artist,
          track_name: q.title,
          album_name: q.album,
          duration: String(Math.round(q.duration))
        })
        try {
          const data = await withRetry(
            () =>
              breaker.exec(() =>
                limiter.schedule(() =>
                  fetchJson(`https://lrclib.net/api/get?${params}`, { schema: LrclibGetSchema })
                )
              ),
            { retries: 1 }
          )
          const synced = data.syncedLyrics ?? null
          const plain = data.plainLyrics ?? null
          return synced || plain ? { synced, plain } : null
        } catch (err) {
          if (err instanceof HttpError && err.status === 404) return null
          throw err
        }
      }
    })
  } catch (err) {
    logWarn('lyrics', `lrclib non raggiungibile (${q.artist} — ${q.title})`, err)
    return null
  }
}
