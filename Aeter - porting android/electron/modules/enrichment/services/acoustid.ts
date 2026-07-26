import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { createHash } from 'node:crypto'
import { getBinaries } from '../../binaries'
import { getSettings } from '../../settings'
import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { fetchJson } from '../../net/http'
import { getApiCache } from '../../net/apiCacheSingleton'
import { TTL } from '../../net/apiCache'
import { AcoustidLookupSchema } from '../schemas'

const execFileAsync = promisify(execFile)

// AcoustID allows up to 3 req/s
const limiter = new RateLimiter({ name: 'acoustid', minIntervalMs: 350 })
const breaker = new CircuitBreaker({ name: 'AcoustID' })

export interface Fingerprint {
  duration: number
  fingerprint: string
}

/** Runs fpcalc on the file. Returns null when the binary is missing; throws on failure. */
export async function computeFingerprint(path: string): Promise<Fingerprint | null> {
  const { fpcalc } = getBinaries()
  if (!fpcalc) return null
  const { stdout } = await execFileAsync(fpcalc, ['-json', path], { timeout: 60_000 })
  const parsed = JSON.parse(stdout) as Fingerprint
  if (!parsed.fingerprint || !Number.isFinite(parsed.duration)) return null
  return parsed
}

/** The app-level key embedded at build time, or '' when none was provided. */
function embeddedAcoustidKey(): string {
  return typeof __ACOUSTID_APP_KEY__ !== 'undefined' ? __ACOUSTID_APP_KEY__ : ''
}

/**
 * True when an AcoustID key (user-supplied or app-embedded) is available, so
 * callers can skip the fpcalc subprocess entirely when a lookup would be futile.
 */
export function acoustidConfigured(): boolean {
  return Boolean(getSettings().acoustidApiKey || embeddedAcoustidKey())
}

/** Looks up the best-matching MusicBrainz recording id (score > 0.7). */
export async function acoustidLookup(fp: Fingerprint): Promise<string | null> {
  // A user-supplied key always wins; otherwise fall back to the app key so
  // enrichment needs zero configuration for non-developers.
  const apiKey = getSettings().acoustidApiKey || embeddedAcoustidKey()
  if (!apiKey) return null
  const key = createHash('sha1').update(fp.fingerprint).digest('hex')
  return getApiCache().cachedJson({
    service: 'acoustid',
    key,
    ttlMs: TTL.ACOUSTID,
    missTtlMs: TTL.NEGATIVE,
    fetcher: async () => {
      const data = await withRetry(
        () =>
          breaker.exec(() =>
            limiter.schedule(() =>
              fetchJson('https://api.acoustid.org/v2/lookup', {
                schema: AcoustidLookupSchema,
                init: {
                  method: 'POST',
                  headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                  body: new URLSearchParams({
                    client: apiKey,
                    duration: String(Math.round(fp.duration)),
                    fingerprint: fp.fingerprint,
                    meta: 'recordingids'
                  }).toString()
                }
              })
            )
          ),
        { retries: 2 }
      )
      const best = data.results?.slice().sort((a, b) => b.score - a.score)[0]
      return best && best.score > 0.7 && best.recordings?.[0] ? best.recordings[0].id : null
    }
  })
}
