/**
 * Auto-fetch of tracks that appear in the synced metadata but have no local
 * audio file on this device. The Drive sync stays metadata-only; this module
 * closes the gap by RE-DOWNLOADING the missing audio from the ordinary download
 * sources (a YouTube search on artist+title), funnelling each fetch through the
 * existing download queue.
 *
 * Flow:
 *  - `syncMissingSet(local, merged)` (called from doSync after the merge) keeps
 *    the `library_fetch` table in step with the merged library: every key that
 *    is in `merged.tracks` but not local is a candidate; keys that became local
 *    (downloaded) or left the library are removed. Existing rows keep their
 *    attempt/backoff state so a permanent no-match is not retried forever.
 *  - A background worker (timer + reaction to library changes) resolves pending
 *    rows to a YouTube video and enqueues the download, capped so it never
 *    floods the queue. On completion it stamps the AUTHORITATIVE metadata from
 *    the sync record onto the file (so the re-scanned row's `trackKey` matches
 *    the missing track and the library converges) and marks the row done.
 *
 * The network policy (Wi-Fi only vs any) is enforced through an injected probe
 * so this file is identical on desktop and the Android port; desktop leaves the
 * probe null (not on metered mobile data) and the policy is treated as allowed.
 */
import type { SyncFile } from './schema'
import type { DownloadItem, MissingFetchStatus } from '@shared/types'
import { getDb } from '../db'
import { getSettings } from '../settings'
import { onBroadcast, broadcast } from '../events'
import { logWarn } from '../logger'
import { downloadExternalTrack } from '../download/externalDownload'
import { upsertTrackFromFile, rebuildAggregates } from '../library'
import { writeTags } from '../tagIO'

const MAX_ATTEMPTS = 3
// Backoff between retries of a track that failed to resolve/download.
const RETRY_BACKOFF_MS = [30 * 60_000, 3 * 60 * 60_000, 12 * 60 * 60_000] // 30m, 3h, 12h
const MAX_ACTIVE = 6 // cap concurrent in-flight auto-fetch downloads
const WORKER_INTERVAL_MS = 20_000
// A remote track whose artist|title exists locally with a duration within this
// tolerance is the same recording under a drifted album tag — not missing.
const FUZZY_DURATION_TOLERANCE_S = 3

export type NetworkType = 'wifi' | 'cellular' | 'ethernet' | 'none' | 'unknown'

// DI: the Android backend injects a probe reading the device connection type.
// Desktop leaves it null → the Wi-Fi policy cannot be measured and is treated as
// allowed (a desktop is not on metered mobile data).
let networkProbe: (() => Promise<NetworkType>) | null = null
export function setNetworkProbe(fn: (() => Promise<NetworkType>) | null): void {
  networkProbe = fn
}

interface FetchRow {
  track_key: string
  title: string
  artist: string
  album: string
  duration: number
  status: string
  attempts: number
  next_retry_at: number | null
  download_id: number | null
}

// ---- table reconciliation (called from doSync) -----------------------------

/** Reconcile `library_fetch` with the merged library: add missing tracks,
 *  drop ones now present locally or gone from the library. */
/** `artist|title` prefix of a v2 trackKey (album segment dropped). */
function artistTitlePrefix(key: string): string {
  return key.split('|').slice(0, 2).join('|')
}

export function syncMissingSet(local: SyncFile, merged: SyncFile): void {
  const db = getDb()
  const localKeys = new Set(Object.keys(local.tracks))
  const now = Date.now()

  // Fuzzy presence guard: album tags drift across devices ('Album sconosciuto',
  // YouTube vs Spotify metadata), so a key-miss alone must not trigger a
  // re-download. Index local durations by artist|title and treat a remote track
  // as present when a local match is within tolerance (or a duration is unknown).
  const localDurations = new Map<string, number[]>()
  for (const [key, t] of Object.entries(local.tracks)) {
    const prefix = artistTitlePrefix(key)
    const list = localDurations.get(prefix) ?? []
    list.push(t.duration)
    localDurations.set(prefix, list)
  }
  const presentFuzzy = (key: string, duration: number): boolean => {
    const candidates = localDurations.get(artistTitlePrefix(key))
    if (!candidates) return false
    if (!duration) return true // remote duration unknown → same artist+title is enough
    return candidates.some(
      (d) => !d || Math.abs(d - duration) <= FUZZY_DURATION_TOLERANCE_S
    )
  }

  const upsert = db.prepare(
    `INSERT INTO library_fetch (track_key, title, artist, album, duration, status, updated_at)
     VALUES (@track_key, @title, @artist, @album, @duration, 'pending', @now)
     ON CONFLICT(track_key) DO UPDATE SET
       title = excluded.title, artist = excluded.artist, album = excluded.album,
       duration = excluded.duration, updated_at = @now`
  )
  const del = db.prepare('DELETE FROM library_fetch WHERE track_key = ?')
  const existing = (
    db.prepare('SELECT track_key FROM library_fetch').all() as { track_key: string }[]
  ).map((r) => r.track_key)

  const wanted = new Set<string>()
  const tx = db.transaction(() => {
    for (const [key, t] of Object.entries(merged.tracks)) {
      if (localKeys.has(key)) continue // already present here → not missing
      if (presentFuzzy(key, t.duration)) continue // same recording, drifted album tag
      wanted.add(key)
      upsert.run({
        track_key: key,
        title: t.title,
        artist: t.artist,
        album: t.album,
        duration: t.duration,
        now
      })
    }
    for (const key of existing) {
      if (!wanted.has(key)) del.run(key) // downloaded, or no longer in the library
    }
  })
  tx()
}

// ---- worker ----------------------------------------------------------------

let workerTimer: ReturnType<typeof setInterval> | null = null
let unsubBroadcast: (() => void) | null = null
let filling = false
let reconciling = false

export function startMissingFetchWorker(): void {
  if (!unsubBroadcast) {
    unsubBroadcast = onBroadcast((event) => {
      // A finished download broadcasts library:changed at the end of its
      // post-processing; that's our cue to stamp tags on completed fetches.
      if (event === 'library:changed') void tick()
    })
  }
  if (!workerTimer) {
    workerTimer = setInterval(() => void tick(), WORKER_INTERVAL_MS)
    workerTimer.unref?.()
  }
  void tick()
}

export function stopMissingFetchWorker(): void {
  unsubBroadcast?.()
  unsubBroadcast = null
  if (workerTimer) {
    clearInterval(workerTimer)
    workerTimer = null
  }
}

/** One pass: settle completed fetches, then start new ones if slots are free. */
async function tick(): Promise<void> {
  const s = getSettings()
  if (!s.autoFetchMissing) return
  await reconcileQueued()
  await fillSlots(s.autoFetchNetwork)
}

async function networkAllows(policy: 'wifi' | 'any'): Promise<boolean> {
  if (policy === 'any') return true
  if (!networkProbe) return true // desktop / no probe → not metered
  try {
    const type = await networkProbe()
    return type === 'wifi' || type === 'ethernet' || type === 'unknown'
  } catch {
    return true // probe failed → don't get stuck
  }
}

/** Kick off resolves+downloads for pending rows up to the active cap. */
async function fillSlots(policy: 'wifi' | 'any'): Promise<void> {
  if (filling) return
  filling = true
  try {
    if (!(await networkAllows(policy))) return
    const db = getDb()
    const active = (
      db.prepare("SELECT COUNT(*) AS n FROM library_fetch WHERE status = 'queued'").get() as {
        n: number
      }
    ).n
    const slots = MAX_ACTIVE - active
    if (slots <= 0) return
    const rows = db
      .prepare(
        `SELECT * FROM library_fetch
         WHERE status IN ('pending', 'searching')
           AND (next_retry_at IS NULL OR next_retry_at <= ?)
         ORDER BY updated_at LIMIT ?`
      )
      .all(Date.now(), slots) as FetchRow[]
    // Resolves are network-bound YouTube searches; run the batch in parallel.
    // Each resolveOne marks its row 'searching' up front and the `filling` flag
    // keeps concurrent fillSlots passes from double-picking rows.
    await Promise.all(rows.map((row) => resolveOne(row)))
  } finally {
    filling = false
  }
}

async function resolveOne(row: FetchRow): Promise<void> {
  const db = getDb()
  db.prepare("UPDATE library_fetch SET status = 'searching', updated_at = ? WHERE track_key = ?").run(
    Date.now(),
    row.track_key
  )
  let item: DownloadItem | null = null
  try {
    item = await downloadExternalTrack({
      artist: row.artist,
      title: row.title,
      durationMs: row.duration ? row.duration * 1000 : null
    })
  } catch (err) {
    markFailure(row, err instanceof Error ? err.message : String(err))
    return
  }
  if (!item) {
    markFailure(row, 'DL_NO_MATCH')
    return
  }
  db.prepare(
    "UPDATE library_fetch SET status = 'queued', download_id = ?, error = NULL, updated_at = ? WHERE track_key = ?"
  ).run(item.id, Date.now(), row.track_key)
}

/** Settle every 'queued' row whose download has finished (or failed). Only acts
 *  once the queue has already ingested the file (a tracks row exists at its
 *  path) so we never rewrite tags concurrently with the queue's post-process. */
async function reconcileQueued(): Promise<void> {
  if (reconciling) return
  reconciling = true
  try {
    const db = getDb()
    const rows = db
      .prepare("SELECT * FROM library_fetch WHERE status = 'queued' AND download_id IS NOT NULL")
      .all() as FetchRow[]
    for (const row of rows) {
      const dl = db
        .prepare('SELECT id, status, file_path, error_message FROM downloads WHERE id = ?')
        .get(row.download_id) as
        | { id: number; status: string; file_path: string | null; error_message: string | null }
        | undefined
      if (!dl) {
        // download row was cleared before we could settle it
        markFailure(row, 'DL_LOST')
        continue
      }
      if (dl.status === 'error' || dl.status === 'cancelled') {
        markFailure(row, dl.error_message ?? 'DL_FAILED')
      } else if (dl.status === 'completed') {
        await settleCompleted(row, dl.file_path)
      }
      // still downloading/paused → leave it for a later tick
    }
  } finally {
    reconciling = false
  }
}

async function settleCompleted(row: FetchRow, filePath: string | null): Promise<void> {
  const db = getDb()
  if (filePath) {
    const ingested = db.prepare('SELECT 1 FROM tracks WHERE path = ?').get(filePath)
    if (!ingested) return // queue hasn't ingested it yet → wait for the next tick
    try {
      // Stamp the authoritative metadata so the re-scanned row's trackKey matches
      // the missing track (duration can't be forced — the closest match was picked).
      // writeTags is sync on desktop and async (SAF) on Android; awaiting works for
      // both and guarantees the file is written before we re-scan it.
      await writeTags(filePath, { title: row.title, artist: row.artist, album: row.album }, null)
      await upsertTrackFromFile(filePath)
      rebuildAggregates()
      broadcast('library:changed', { reason: 'sync' })
    } catch (err) {
      logWarn('sync', `stamp metadati brano scaricato fallito: ${filePath}`, err)
    }
  }
  db.prepare("UPDATE library_fetch SET status = 'done', error = NULL, updated_at = ? WHERE track_key = ?").run(
    Date.now(),
    row.track_key
  )
}

function markFailure(row: FetchRow, message: string): void {
  const attempts = row.attempts + 1
  const now = Date.now()
  const db = getDb()
  if (attempts >= MAX_ATTEMPTS) {
    db.prepare(
      "UPDATE library_fetch SET status = 'failed', attempts = ?, next_retry_at = NULL, error = ?, updated_at = ? WHERE track_key = ?"
    ).run(attempts, message, now, row.track_key)
  } else {
    const backoff = RETRY_BACKOFF_MS[Math.min(attempts - 1, RETRY_BACKOFF_MS.length - 1)]
    db.prepare(
      "UPDATE library_fetch SET status = 'pending', attempts = ?, next_retry_at = ?, error = ?, updated_at = ? WHERE track_key = ?"
    ).run(attempts, now + backoff, message, now, row.track_key)
  }
}

// ---- manual controls / status (for the settings UI) ------------------------

/** Reset failed rows to pending and kick the worker (manual "retry" button). */
export function retryFailedFetches(): MissingFetchStatus {
  getDb()
    .prepare(
      "UPDATE library_fetch SET status = 'pending', attempts = 0, next_retry_at = NULL, error = NULL, updated_at = ? WHERE status = 'failed'"
    )
    .run(Date.now())
  void tick()
  return getMissingFetchStatus()
}

export function getMissingFetchStatus(): MissingFetchStatus {
  const db = getDb()
  const row = db
    .prepare(
      `SELECT
         SUM(CASE WHEN status IN ('pending','searching') THEN 1 ELSE 0 END) AS pending,
         SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END) AS active,
         SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed,
         COUNT(*) AS total
       FROM library_fetch WHERE status != 'done'`
    )
    .get() as { pending: number | null; active: number | null; failed: number | null; total: number | null }
  return {
    pending: row.pending ?? 0,
    active: row.active ?? 0,
    failed: row.failed ?? 0,
    total: row.total ?? 0
  }
}
