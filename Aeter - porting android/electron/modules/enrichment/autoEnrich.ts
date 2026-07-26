import PQueue from 'p-queue'
import { getDb } from '../db'
import { rebuildAggregates } from '../library'
import { broadcast } from '../events'
import { logWarn } from '../logger'
import { getSettings } from '../settings'
import { isBackground } from '../appLifecycle'
import { getApiCache } from '../net/apiCacheSingleton'
import { TTL } from '../net/apiCache'
import { enrichTrack, NO_MATCH_MESSAGE, NEEDS_REVIEW_MESSAGE, MB_UNAVAILABLE_MESSAGE } from './pipeline'
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
  try {
    const result = await enrichTrack(id)
    // All metadata providers unreachable: stop the batch instead of stacking
    // failures; leaves enrich_status untouched so it retries next pass.
    if (!result.applied && result.message === MB_UNAVAILABLE_MESSAGE) {
      queue.clear()
      logWarn('enrich', 'Auto-arricchimento interrotto: provider metadati non raggiungibili')
      return
    }
    // needs-review shares the no-match skip policy: don't hammer the providers
    // again this week; the TTL expiry doubles as the automatic retry.
    if (!result.applied && (result.message === NO_MATCH_MESSAGE || result.message === NEEDS_REVIEW_MESSAGE)) {
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

/**
 * Forces re-enrichment of tracks the user just edited: a fresh title/artist
 * gives the matcher a real shot at a track it previously couldn't identify, so
 * clear the per-session attempted mark and the negative skip-cache first — a
 * stale no-match/needs-review verdict must not suppress the retry.
 */
export function requeueEnrichment(ids: number[]): void {
  const cache = getApiCache()
  for (const id of ids) {
    attempted.delete(id)
    cache.invalidate(SKIP_SERVICE, String(id))
  }
  enqueueEnrichment(ids)
}

// Idle re-sweep: after a scan/download the queue drains and stops, so a stable
// library would never retry its needs-review/no-match tracks — even though the
// 7-day skip-cache TTL is designed for exactly that. A single low-frequency
// timer picks them up once something new can identify them (a later Shazam hit,
// or AcoustID). The skip-cache throttles per-track re-attempts to ~weekly, so a
// 6h tick never hammers the providers. Foreground-only on Android (enforced via
// appLifecycle — a foreground service can keep the process alive backgrounded);
// the post-scan trigger covers app relaunch.
const RESWEEP_INTERVAL_MS = 6 * 60 * 60 * 1000
let schedulerStarted = false

/** Starts the idle re-sweep timer once. Idempotent; safe to call on every scan. */
export function ensureAutoEnrichScheduler(): void {
  if (schedulerStarted) return
  schedulerStarted = true
  const timer = setInterval(() => {
    // Skip while backgrounded on Android: with a media/download foreground
    // service alive the process ISN'T suspended, and a 100-track enrichment
    // batch (fingerprint CPU + provider calls) must not start screen-off.
    // The post-scan trigger and the next foreground tick cover the backlog.
    if (isBackground()) return
    // Read the setting each tick so toggling the master switch on later works
    // without a restart; autoEnrichMissing also re-checks it defensively.
    if (getSettings().autoEnrichEnabled) void autoEnrichMissing()
  }, RESWEEP_INTERVAL_MS)
  // Never keep the process alive solely for this sweep.
  timer.unref()
}

export async function autoEnrichMissing(): Promise<void> {
  // Master switch: when the user opts out, no background pass ever queues work.
  // The manual per-track enrichTrack IPC stays available as an escape hatch.
  if (!getSettings().autoEnrichEnabled) return
  // Device critically hot: don't start a NEW background batch; the 6h re-sweep
  // or the next post-scan trigger picks the backlog up once it cools.
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
    // Enrichment can change a track's album (album_key), so refresh the
    // materialized albums/artists tables once the batch settles.
    if (total > 0) rebuildAggregates()
  } finally {
    autoRunning = false
  }
}
