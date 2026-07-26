import { createHash, randomBytes } from 'node:crypto'
import { z } from 'zod'
import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { fetchJson } from '../../net/http'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'

// Unofficial Shazam recognition endpoint (same one the Android app calls).
// Fully keyless: the signature is computed locally (shazam/signature.ts) and
// POSTed with random UUIDs. Unofficial ⇒ defensive stack: gentle rate limit,
// breaker, single retry, zod schema, 30-day cache keyed by signature hash.
const limiter = new RateLimiter({ name: 'shazam', minIntervalMs: 3000, maxConcurrent: 1 })
const breaker = new CircuitBreaker({ name: 'Shazam' })

const ShazamResponseSchema = z.object({
  matches: z.array(z.unknown()).optional(),
  track: z
    .object({
      key: z.string(),
      title: z.string(),
      /** The artist line ("subtitle" in Shazam's payload). */
      subtitle: z.string().optional(),
      images: z
        .object({
          coverart: z.string().optional(),
          coverarthq: z.string().optional()
        })
        .optional(),
      sections: z
        .array(
          z.object({
            type: z.string().optional(),
            metadata: z
              .array(z.object({ title: z.string().optional(), text: z.string().optional() }))
              .optional()
          })
        )
        .optional()
    })
    .optional()
})

export interface ShazamMatch {
  /** Shazam's stable track key — segment corroboration compares this. */
  key: string
  title: string
  artist: string
  album: string | null
  year: number | null
  coverUrl: string | null
}

/** nodejs-mobile (Node 12) has no crypto.randomUUID; build a v4 from randomBytes. */
function uuidv4(): string {
  const b = randomBytes(16)
  b[6] = (b[6] & 0x0f) | 0x40
  b[8] = (b[8] & 0x3f) | 0x80
  const h = b.toString('hex')
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`
}

function parseMatch(data: z.infer<typeof ShazamResponseSchema>): ShazamMatch | null {
  const track = data.track
  if (!track || !data.matches || data.matches.length === 0) return null

  let album: string | null = null
  let year: number | null = null
  for (const section of track.sections ?? []) {
    for (const meta of section.metadata ?? []) {
      if (meta.title === 'Album' && meta.text) album = meta.text
      if (meta.title === 'Released' && meta.text) year = Number(meta.text.slice(0, 4)) || null
    }
  }
  return {
    key: track.key,
    title: track.title,
    artist: track.subtitle ?? '',
    album,
    year,
    coverUrl: track.images?.coverarthq ?? track.images?.coverart ?? null
  }
}

/**
 * Recognizes one locally-generated signature. Cached 30 days (positives and
 * negatives) keyed by the signature hash, so re-enriching the same audio never
 * re-hits the endpoint. Throws on network/breaker/schema failure so callers
 * can distinguish "no match" (null) from "unavailable".
 */
export async function shazamRecognize(
  signatureUri: string,
  samplems: number
): Promise<ShazamMatch | null> {
  const cacheKey = createHash('sha1').update(signatureUri).digest('hex')
  return getApiCache().cachedJson<ShazamMatch>({
    service: 'shazam',
    key: cacheKey,
    ttlMs: TTL.ACOUSTID,
    missTtlMs: TTL.NEGATIVE,
    fetcher: async () => {
      const url =
        `https://amp.shazam.com/discovery/v5/en/US/android/-/tag/${uuidv4()}/${uuidv4()}` +
        `?sync=true&webv3=true&sampling=true&connected=&shazamapiversion=v3&sharehub=true&video=v3`
      const body = {
        timezone: 'Europe/Rome',
        signature: { uri: signatureUri, samplems },
        timestamp: Date.now(),
        context: {},
        geolocation: {}
      }
      const data = await withRetry(
        () =>
          breaker.exec(() =>
            limiter.schedule(() =>
              fetchJson(url, {
                schema: ShazamResponseSchema,
                timeoutMs: 20_000,
                userAgent:
                  'Dalvik/2.1.0 (Linux; U; Android 13; Pixel 7 Build/TQ3A.230901.001)',
                init: {
                  method: 'POST',
                  headers: {
                    'Content-Type': 'application/json',
                    'Content-Language': 'en_US'
                  },
                  body: JSON.stringify(body)
                }
              })
            )
          ),
        { retries: 1 }
      )
      return parseMatch(data)
    }
  })
}
