import { CircuitBreaker } from '../../net/circuitBreaker'
import { RateLimiter } from '../../net/rateLimiter'
import { withRetry } from '../../net/retry'
import { DEFAULT_USER_AGENT } from '../../net/http'
import { HttpError, NetworkError, RateLimitError } from '../../net/errors'
import { logWarn } from '../../logger'
import { validateCoverBuffer } from '../../coverValidation'

const limiter = new RateLimiter({ name: 'coverart', minIntervalMs: 250, maxConcurrent: 2 })
const breaker = new CircuitBreaker({ name: 'CoverArtArchive' })

/** GETs an image and returns the bytes only if they pass validation. 404 → null. */
async function fetchImage(url: string, useBreaker: boolean): Promise<Buffer | null> {
  const run = async (): Promise<Buffer | null> => {
    let res: Response
    try {
      res = await fetch(url, {
        headers: { 'User-Agent': DEFAULT_USER_AGENT },
        signal: AbortSignal.timeout(20_000)
      })
    } catch (err) {
      throw new NetworkError(`Richiesta fallita verso ${url}`, err)
    }
    if (res.status === 404) return null
    if (res.status === 429) throw new RateLimitError(url, null)
    if (!res.ok) throw new HttpError(res.status, url)
    const buf = Buffer.from(await res.arrayBuffer())
    return validateCoverBuffer(buf) ? buf : null
  }
  try {
    return await withRetry(
      () => (useBreaker ? breaker.exec(() => limiter.schedule(run)) : run()),
      { retries: 2 }
    )
  } catch (err) {
    logWarn('enrich', `Cover non raggiungibile (${url})`, err)
    return null
  }
}

/** Cover Art Archive: front 500px for a specific release. Missing (404) → null. */
export async function fetchCoverArt(releaseId: string): Promise<Buffer | null> {
  return fetchImage(`https://coverartarchive.org/release/${releaseId}/front-500`, true)
}

/** Cover Art Archive: front 500px chosen for a whole release-group (recommended). */
export async function fetchReleaseGroupCover(releaseGroupId: string): Promise<Buffer | null> {
  return fetchImage(`https://coverartarchive.org/release-group/${releaseGroupId}/front-500`, true)
}

export interface CoverSources {
  /** Direct cover URL from the winning provider (iTunes/Deezer/Spotify). */
  coverUrl?: string | null
  mbReleaseGroupId?: string | null
  mbReleaseIds?: string[]
}

export interface ResolvedCover {
  buffer: Buffer
  /** Provenance for cover_art.source: exact provider match vs CAA guess. */
  origin: 'provider' | 'caa'
}

/**
 * Resolves a valid cover image through a fallback chain, returning the first
 * that passes validation:
 *   1. the provider's direct cover URL (tied to the exact matched album);
 *   2. Cover Art Archive release-group front;
 *   3. Cover Art Archive per-release front, across all candidate releases.
 * Returns null only when nothing valid is found anywhere.
 */
export async function resolveCover(sources: CoverSources): Promise<ResolvedCover | null> {
  if (sources.coverUrl) {
    const buf = await fetchImage(sources.coverUrl, false)
    if (buf) return { buffer: buf, origin: 'provider' }
  }
  if (sources.mbReleaseGroupId) {
    const buf = await fetchReleaseGroupCover(sources.mbReleaseGroupId)
    if (buf) return { buffer: buf, origin: 'caa' }
  }
  for (const releaseId of sources.mbReleaseIds ?? []) {
    const buf = await fetchCoverArt(releaseId)
    if (buf) return { buffer: buf, origin: 'caa' }
  }
  return null
}
