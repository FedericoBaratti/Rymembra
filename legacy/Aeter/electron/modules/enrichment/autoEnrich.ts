import PQueue from 'p-queue'
import { getDb } from '../db'
import { broadcast } from '../events'
import { logWarn } from '../logger'
import { getApiCache } from '../net/apiCacheSingleton'
import { TTL } from '../net/apiCache'
import { enrichTrack, NO_MATCH_MESSAGE } from './pipeline'
import { isMusicBrainzAvailable } from './services/musicbrainz'
import { CANDIDATE_WHERE } from './stats'
import { thermalManager } from '../adaptiveConcurrency'

// Concurrent low-priority enrichment. Throughput is bounded by the global
// MusicBrainz limiter (1 req/1.1s); 3 workers overlap fpcalc CPU work and
// the other API calls.
const CONCURRENCY = 3

/** Persistent skip-list for tracks with no MusicBrainz match (replaces the old in-memory Set). */
export const SKIP_SERVICE = 'enrich-miss'

const attempted = new Set<number>()
let autoRunning = false

// Shared queue: batch auto-enrichment and per-download enqueues feed the same
// workers, so MB rate limiting and concurrency stay globally bounded.
// Thermal pressure retunes the workers live (fpcalc is the CPU-heavy part).
const queue = new PQueue({ concurrency: thermalManager.getConcurrency(CONCURRENCY) })
thermalManager.onChange(() => {
  queue.concurrency = thermalManager.getConcurrency(CONCURRENCY)
})

/** Lets "retry failed" re-run tracks already attempted in this session. */
export function resetAttempted(): void {
  attempted.clear()
}

async function processTrack(id: number, onDone?: () => void): Promise<void> {
  // stop the whole batch early instead of stacking timeouts
  if (!isMusicBrainzAvailable()) {
    queue.clear()
    logWarn('enrich', 'Auto-arricchimento interrotto: MusicBrainz non raggiungibile')
    return
  }
  try {
    const result = await enrichTrack(id)
    if (!result.applied && result.message === NO_MATCH_MESSAGE) {
      getApiCache().set(SKIP_SERVICE, String(id), null, TTL.NEGATIVE)
    }
    broadcast('enrichment:updated', result)
  } catch (err) {
    getDb()
      .prepare(`UPDATE tracks SET enrich_status = 'error', enrich_attempted_at = ? WHERE id = ?`)
      .run(Date.now(), id)
    logWarn('enrich', `Auto-arricchimento fallito per la traccia ${id}`, err)
  } finally {
    onDone?.()
  }
}

/**
 * Queues specific tracks (e.g. fresh YouTube downloads) for enrichment,
 * reusing the auto-enrich workers. Fire-and-forget.
 */
export function enqueueEnrichment(ids: number[]): void {
  const cache = getApiCache()
  for (const id of ids) {
    if (attempted.has(id) || cache.has(SKIP_SERVICE, String(id))) continue
    attempted.add(id)
    void queue.add(() => processTrack(id))
  }
}

export async function autoEnrichMissing(): Promise<void> {
  // Device critically hot: don't start a NEW background batch; the next
  // trigger picks the backlog up once it cools. (Never fires on desktop.)
  if (thermalManager.shouldDefer('auto-enrich')) return
  if (autoRunning) return
  autoRunning = true
  try {
    const rows = getDb()
      .prepare(
        `SELECT id FROM tracks
         WHERE ${CANDIDATE_WHERE}
         ORDER BY date_added DESC LIMIT 100`
      )
      .all() as { id: number }[]

    const cache = getApiCache()
    const candidates = rows.filter(
      ({ id }) => !attempted.has(id) && !cache.has(SKIP_SERVICE, String(id))
    )
    const total = candidates.length
    let done = 0

    for (const { id } of candidates) {
      attempted.add(id)
      void queue.add(() =>
        processTrack(id, () => {
          done++
          broadcast('enrichment:progress', { phase: 'enrich', done, total })
        })
      )
    }
    await queue.onIdle()
  } finally {
    autoRunning = false
  }
}
