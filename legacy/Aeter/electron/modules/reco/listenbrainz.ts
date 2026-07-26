import { CircuitBreaker } from '../net/circuitBreaker'
import { RateLimiter } from '../net/rateLimiter'
import { withRetry } from '../net/retry'
import { fetchJson } from '../net/http'
import { getApiCache } from '../net/apiCacheSingleton'
import { TTL } from '../net/apiCache'
import { logWarn } from '../logger'
import { LbSimilarSchema, LbRadioSchema } from './schemas'
import type { RawCandidate } from './engine'

// ListenBrainz is KEYLESS and indexed by MusicBrainz recording MBID — which the
// enrichment pipeline already stores on tracks (mb_recording_id). Two surfaces:
//  - labs similar-recordings: nearest recordings to a seed MBID;
//  - LB Radio: a JSPF playlist from a free-text prompt (artist:/tag:/recording:).
// Both soft-fail to [] so a missing network / cold device never breaks discovery.

const SIMILAR_BASE = 'https://labs.api.listenbrainz.org/similar-recordings/json'
const RADIO_BASE = 'https://api.listenbrainz.org/1/explore/lb-radio'
// Default similar-recordings algorithm (session-based). Tunable; the labs JSON
// interface documents the available algorithm names.
const SIMILAR_ALGO =
  'session_based_days_7500_session_300_contribution_5_threshold_15_limit_50_filter_True_skip_30'

const limiter = new RateLimiter({ name: 'listenbrainz', minIntervalMs: 300 })
const breaker = new CircuitBreaker({ name: 'ListenBrainz' })

const MBID_RE = /([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})/i

/** Pull the recording MBID out of a JSPF `identifier` (string or string[]). */
function mbidFromIdentifier(id: string | string[] | null | undefined): string | null {
  const list = id == null ? [] : Array.isArray(id) ? id : [id]
  for (const entry of list) {
    const m = MBID_RE.exec(entry)
    if (m) return m[1].toLowerCase()
  }
  return null
}

/** Similar recordings to a seed MBID. Keyless. Soft-fails to []. */
export async function fetchSimilarRecordings(mbid: string): Promise<RawCandidate[]> {
  if (!mbid) return []
  try {
    const out = await getApiCache().cachedJson<RawCandidate[]>({
      service: 'lb-similar',
      key: mbid.toLowerCase(),
      ttlMs: TTL.MB_RECORDING,
      missTtlMs: TTL.NEGATIVE,
      fetcher: async () => {
        const url = `${SIMILAR_BASE}?recording_mbids=${encodeURIComponent(mbid)}&algorithm=${encodeURIComponent(SIMILAR_ALGO)}`
        const rows = await withRetry(
          () => breaker.exec(() => limiter.schedule(() => fetchJson(url, { schema: LbSimilarSchema }))),
          { retries: 2 }
        )
        const max = rows.reduce((m, r) => Math.max(m, r.score ?? 0), 0) || 1
        return rows
          // the seed row carries a `comment`; similar rows don't
          .filter((r) => !r.comment && r.recording_mbid && r.recording_mbid.toLowerCase() !== mbid.toLowerCase())
          .map((r) => ({
            mbid: r.recording_mbid.toLowerCase(),
            title: r.recording_name ?? null,
            artist: r.artist_credit_name ?? null,
            score: (r.score ?? 0) / max,
            source: 'listenbrainz' as const
          }))
      }
    })
    return out ?? []
  } catch (err) {
    logWarn('reco', `ListenBrainz similar non raggiungibile (${mbid})`, err)
    return []
  }
}

export type RadioMode = 'easy' | 'medium' | 'hard'

/** LB Radio playlist from a prompt (e.g. `artist:(<name>)`, `tag:(<genre>)`). Keyless. */
export async function fetchRadioByPrompt(prompt: string, mode: RadioMode = 'easy'): Promise<RawCandidate[]> {
  const trimmed = prompt.trim()
  if (!trimmed) return []
  try {
    const out = await getApiCache().cachedJson<RawCandidate[]>({
      service: 'lb-radio',
      key: `${mode}|${trimmed}`,
      ttlMs: TTL.MB_SEARCH,
      missTtlMs: TTL.NEGATIVE,
      fetcher: async () => {
        const url = `${RADIO_BASE}?prompt=${encodeURIComponent(trimmed)}&mode=${mode}`
        const data = await withRetry(
          () => breaker.exec(() => limiter.schedule(() => fetchJson(url, { schema: LbRadioSchema }))),
          { retries: 2 }
        )
        const tracks = data.payload?.jspf?.playlist?.track ?? []
        const out: RawCandidate[] = []
        tracks.forEach((tk, i) => {
          const mbid = mbidFromIdentifier(tk.identifier)
          out.push({
            mbid,
            title: tk.title ?? null,
            artist: tk.creator ?? null,
            // JSPF order is the curation rank; decay so earlier picks score higher
            score: 1 - i / Math.max(tracks.length, 1),
            source: 'listenbrainz' as const
          })
        })
        return out
      }
    })
    return out ?? []
  } catch (err) {
    logWarn('reco', `ListenBrainz radio non raggiungibile (${trimmed})`, err)
    return []
  }
}
