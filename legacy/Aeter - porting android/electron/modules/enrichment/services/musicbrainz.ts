import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { fetchJson } from '../../net/http'
import { HttpError, RateLimitError } from '../../net/errors'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'
import { logWarn } from '../../logger'
import { cleanQueryText } from '../match'
import { MbRecordingSchema, MbSearchSchema, type MbRecording } from '../schemas'

// MusicBrainz policy is 1 request/second — this limiter is the single global
// serialization point that makes concurrent enrichment safe.
const limiter = new RateLimiter({ name: 'musicbrainz', minIntervalMs: 1100, maxConcurrent: 1 })
export const mbBreaker = new CircuitBreaker({ name: 'MusicBrainz' })

export function isMusicBrainzAvailable(): boolean {
  return mbBreaker.state !== 'open'
}

const BASE = 'https://musicbrainz.org/ws/2'

/** Retry wraps breaker wraps limiter: each attempt re-queues through the limiter. */
async function mbRequest<T>(path: string, schema: { parse(v: unknown): T }): Promise<T | null> {
  try {
    return await withRetry(
      () =>
        mbBreaker.exec(() =>
          limiter.schedule(() =>
            fetchJson(`${BASE}/${path}`, {
              schema,
              init: { headers: { Accept: 'application/json' } }
            })
          )
        ),
      {
        retries: 2,
        onRetry: (err, attempt, delayMs) => {
          if (err instanceof RateLimitError) limiter.notifyRateLimited(err.retryAfterMs ?? undefined)
          logWarn('mb', `Richiesta MusicBrainz fallita (tentativo ${attempt + 1}), riprovo tra ${Math.round(delayMs)}ms`, err)
        }
      }
    )
  } catch (err) {
    if (err instanceof HttpError && err.status === 404) return null
    throw err
  }
}

export async function mbGetRecording(id: string): Promise<MbRecording | null> {
  return getApiCache().cachedJson({
    service: 'mb-recording',
    key: id,
    ttlMs: TTL.MB_RECORDING,
    missTtlMs: TTL.NEGATIVE,
    fetcher: () =>
      mbRequest(
        `recording/${id}?inc=artist-credits+releases+release-groups+media&fmt=json`,
        MbRecordingSchema
      )
  })
}

/** One cached search for a fully-formed Lucene query (keyed by the query itself). */
async function mbSearchQuery(query: string): Promise<MbRecording[]> {
  const result = await getApiCache().cachedJson({
    service: 'mb-search',
    key: query.toLowerCase(),
    ttlMs: TTL.MB_SEARCH,
    missTtlMs: TTL.NEGATIVE,
    fetcher: async () => {
      const data = await mbRequest(
        `recording?query=${encodeURIComponent(query)}&limit=25&fmt=json`,
        MbSearchSchema
      )
      return data?.recordings ?? []
    }
  })
  return result ?? []
}

/**
 * Tiered search: tags from messy files (YouTube etc.) rarely match a strict
 * quoted phrase, so we widen progressively and stop at the first tier with
 * hits — (1) cleaned title+artist, (2) raw title+artist, (3) title only.
 * The composite scorer in match.ts then rejects weak candidates.
 */
export async function mbSearchRecordings(title: string, artist: string): Promise<MbRecording[]> {
  const rawTitle = title.replace(/"/g, '').trim()
  const cleanTitle = cleanQueryText(title).replace(/"/g, '').trim() || rawTitle
  const known = artist && artist !== 'Artista sconosciuto'
  const rawArtist = known ? artist.replace(/"/g, '').trim() : ''
  const cleanArtist = known ? cleanQueryText(artist).replace(/"/g, '').trim() || rawArtist : ''

  const tiers: string[] = []
  if (cleanArtist) tiers.push(`recording:"${cleanTitle}" AND artist:"${cleanArtist}"`)
  if (rawArtist) tiers.push(`recording:"${rawTitle}" AND artist:"${rawArtist}"`)
  tiers.push(`recording:"${cleanTitle}"`)

  const seen = new Set<string>()
  for (const query of tiers) {
    if (seen.has(query)) continue
    seen.add(query)
    const recs = await mbSearchQuery(query)
    if (recs.length > 0) return recs
  }
  return []
}
