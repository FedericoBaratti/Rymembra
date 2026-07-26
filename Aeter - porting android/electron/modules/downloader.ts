import { mkdirSync, readdirSync, rmdirSync, statSync, unlinkSync } from 'node:fs'
import { join, extname } from 'node:path'
import type { DownloadItem, DownloadPreview } from '@shared/types'
import { getDb } from './db'
import { getSettings } from './settings'
import { scanFolders, rebuildAggregates, removeTrackByPath, TRASH_DIR_NAME } from './library'
import { broadcast } from './events'
import { logWarn } from './logger'
import { findHandler, handlerForType } from './download/registry'
import { DownloadError } from './download/errors'
import { decideRetry } from './download/retryPolicy'
import { validateAudioFile, MIN_BYTES } from './download/validate'
import { ytdlpTempDir } from './download/tempDir'
import { canPause, canResume, statusOnAbort } from './download/pauseTransitions'
import { correctYoutubeDownloads, unifyAlbumTags } from './download/postProcess'
import { detectUrl } from './download/urlDetect'
import { enqueueEnrichment } from './enrichment/autoEnrich'
import { autoDedupe, purgeTrashDir } from './metadata'

// Queue orchestrator. Source-specific work lives in download/sources/*;
// this module owns the downloads table, concurrency, automatic retry with
// backoff, cancellation and crash recovery.

export { detectUrl } from './download/urlDetect'

const running = new Map<number, AbortController>()
/** Ids whose in-flight abort means "pause", not "cancel". */
const pauseRequested = new Set<number>()

/**
 * Optional sink for a native download foreground-service + progress notification
 * (Android). Mirrors setMigrationLifecycle: the node-backend wires these to
 * callNative('downloadService*'). No-op on desktop. onProgress reports the
 * just-updated item so the notification shows a live title + percentage.
 */
export interface DownloadLifecycle {
  onStart?: () => void
  onProgress?: (active: number, total: number, percent: number, title: string) => void
  onStop?: () => void
}
let downloadLifecycle: DownloadLifecycle = {}
let downloadServiceActive = false
export function setDownloadLifecycle(l: DownloadLifecycle): void {
  downloadLifecycle = l
}


// ---------- preview ----------

export async function previewDownload(url: string): Promise<DownloadPreview> {
  const found = findHandler(url)
  if (!found) throw new Error('DL_UNRECOGNIZED_URL')
  return found.handler.preview(url, found.parsed)
}

// ---------- queue ----------

function rowToItem(row: Record<string, unknown>): DownloadItem {
  return row as unknown as DownloadItem
}

export function getDownloads(): DownloadItem[] {
  return (
    getDb().prepare('SELECT * FROM downloads ORDER BY created_at DESC').all() as Record<
      string,
      unknown
    >[]
  ).map(rowToItem)
}

function getItem(id: number): DownloadItem | null {
  const row = getDb().prepare('SELECT * FROM downloads WHERE id = ?').get(id) as
    | Record<string, unknown>
    | undefined
  return row ? rowToItem(row) : null
}

function updateItem(id: number, patch: Partial<DownloadItem>): void {
  const keys = Object.keys(patch)
  if (keys.length === 0) return
  // Authoritative write: supersedes (and drops) any coalesced progress patch
  // still waiting in the throttle below, so a late flush can never overwrite a
  // terminal status with stale progress.
  pendingProgress.delete(id)
  if (patch.status !== undefined) lastWrittenPct.delete(id)
  const sets = keys.map((k) => `${k} = @${k}`).join(', ')
  getDb().prepare(`UPDATE downloads SET ${sets} WHERE id = @__id`).run({ ...patch, __id: id })
  const item = getItem(id)
  if (item) broadcast('download:updated', item)
}

// ---------- progress throttling ----------
// yt-dlp emits many progress lines per second, and each one used to cost a DB
// UPDATE (→ a debounced full-DB flush on Android) + a SELECT read-back + a
// renderer broadcast + a native-notification RPC. Coalesce per item: the first
// patch of a burst applies immediately (leading edge), then at most one write
// every PROGRESS_INTERVAL_MS while the stream keeps flowing. Status changes and
// terminal transitions go through updateItem directly and are never delayed.
const PROGRESS_INTERVAL_MS = 500
const pendingProgress = new Map<number, Partial<DownloadItem>>()
const progressTimers = new Map<number, ReturnType<typeof setTimeout>>()
const lastWrittenPct = new Map<number, number>()

function applyProgress(id: number, patch: Partial<DownloadItem>): void {
  // Skip the write when the only change is a sub-percent progress delta — the
  // bar can't show it and the DB row would be byte-identical to the reader.
  const keys = Object.keys(patch)
  if (keys.length === 1 && patch.progress != null) {
    const pct = Math.round(patch.progress * 100)
    if (pct === lastWrittenPct.get(id)) return
  }
  if (patch.progress != null) lastWrittenPct.set(id, Math.round(patch.progress * 100))
  updateItem(id, patch)
  const it = getItem(id)
  if (it) {
    downloadLifecycle.onProgress?.(
      running.size,
      it.total_tracks,
      Math.round((it.progress ?? 0) * 100),
      it.title
    )
  }
}

function armProgressTimer(id: number): void {
  const t = setTimeout(() => {
    progressTimers.delete(id)
    const patch = pendingProgress.get(id)
    if (patch) {
      pendingProgress.delete(id)
      applyProgress(id, patch)
      armProgressTimer(id) // stream still flowing → stay in throttled mode
    }
  }, PROGRESS_INTERVAL_MS)
  ;(t as { unref?: () => void }).unref?.()
  progressTimers.set(id, t)
}

function queueProgress(id: number, patch: Partial<DownloadItem>): void {
  if (progressTimers.has(id)) {
    pendingProgress.set(id, { ...pendingProgress.get(id), ...patch })
    return
  }
  applyProgress(id, patch)
  armProgressTimer(id)
}

export function startDownload(preview: DownloadPreview): DownloadItem {
  const db = getDb()
  const res = db
    .prepare(
      `INSERT INTO downloads (source_url, source_type, status, title, artist, album, cover_url, total_tracks, created_at)
       VALUES (?, ?, 'pending', ?, ?, ?, ?, ?, ?)`
    )
    .run(
      preview.source_url,
      preview.source_type,
      preview.title,
      preview.artist,
      preview.album,
      preview.cover_url,
      Math.max(1, preview.total_tracks),
      Date.now()
    )
  const item = getItem(Number(res.lastInsertRowid))!
  broadcast('download:updated', item)
  pump()
  return item
}

export function cancelDownload(id: number): void {
  const controller = running.get(id)
  if (controller) {
    controller.abort()
  } else {
    updateItem(id, { status: 'cancelled' })
  }
}

export function pauseDownload(id: number): void {
  const controller = running.get(id)
  if (controller) {
    pauseRequested.add(id)
    controller.abort()
    return
  }
  const item = getItem(id)
  if (item && canPause(item.status)) {
    updateItem(id, { status: 'paused', next_retry_at: null })
  }
}

export function resumeDownload(id: number): void {
  const item = getItem(id)
  if (!item || !canResume(item.status)) return
  updateItem(id, { status: 'pending', next_retry_at: null, error_message: null })
  pump()
}

export function retryDownload(id: number): void {
  updateItem(id, {
    status: 'pending',
    progress: 0,
    completed_tracks: 0,
    current_file: null,
    error_message: null,
    attempts: 0,
    next_retry_at: null
  })
  pump()
}

export function clearFinished(): void {
  getDb()
    .prepare("DELETE FROM downloads WHERE status IN ('completed', 'error', 'cancelled')")
    .run()
}

/**
 * Recovery on app start: rows stuck in 'downloading' from a crashed session
 * go back to 'pending', and stale partial-download artifacts are removed.
 */
export function recoverStaleDownloads(): void {
  getDb()
    .prepare(
      "UPDATE downloads SET status = 'pending', next_retry_at = NULL WHERE status = 'downloading'"
    )
    .run()
  cleanupPartialFiles()
  cleanupPartialFiles(ytdlpTempDir(), 0, true)
  purgeBrokenDownloads()
  purgeTrashDir()
}

const PARTIAL_EXT = new Set(['.part', '.ytdl', '.temp'])
// A partial older than a day has no live download attached (the queue retries
// within minutes) — it is garbage, not a resumable download.
const PARTIAL_MAX_AGE_MS = 24 * 60 * 60 * 1000

/** Remove stale download artifacts: partial-extension files in the library
 *  download folder; with `anyExt` (the private yt-dlp temp dir) every stale
 *  file goes, whatever its name. */
function cleanupPartialFiles(dir?: string, depth = 0, anyExt = false): void {
  if (depth > 3) return
  const folder = dir ?? getSettings().downloadFolder
  let entries: string[]
  try {
    entries = readdirSync(folder)
  } catch {
    return
  }
  const cutoff = Date.now() - PARTIAL_MAX_AGE_MS
  for (const name of entries) {
    const full = join(folder, name)
    try {
      const st = statSync(full)
      if (st.isDirectory()) {
        if (name === TRASH_DIR_NAME) continue // dedupe trash has its own purge policy
        cleanupPartialFiles(full, depth + 1, anyExt)
        // In the private temp dir, yt-dlp mirrors the output template dirs and
        // leaves them empty after the final move — drop them too.
        if (anyExt) {
          try {
            rmdirSync(full)
          } catch {
            // non-empty (fresh partials inside) — keep
          }
        }
      } else if ((anyExt || PARTIAL_EXT.has(extname(name).toLowerCase())) && st.mtimeMs < cutoff) {
        unlinkSync(full)
        logWarn('download', `Rimosso file parziale orfano: ${full}`)
      }
    } catch {
      // ignore racing deletions
    }
  }
}

/**
 * One-shot boot repair for libraries already polluted by truncated downloads
 * (before -P temp:, yt-dlp intermediates were born inside the watched folder):
 * drop track rows under the download folder whose file is gone or too small to
 * be real audio, deleting the truncated leftovers. Idempotent.
 */
function purgeBrokenDownloads(): void {
  const folder = getSettings().downloadFolder
  if (!folder) return
  try {
    // Skip entirely when the folder is unreachable (e.g. desktop external
    // drive unmounted): missing files there are NOT evidence of broken tracks.
    if (!statSync(folder).isDirectory()) return
  } catch {
    return
  }
  let rows: Array<{ path: string }>
  try {
    rows = getDb().prepare('SELECT path FROM tracks WHERE path LIKE ?').all(`${folder}%`) as Array<{
      path: string
    }>
  } catch {
    return
  }
  let removed = 0
  for (const { path } of rows) {
    let size: number
    try {
      size = statSync(path).size
    } catch (err) {
      // Only a confirmed missing file is a broken download; a transient stat
      // failure (EACCES/EPERM/EIO on a flaky SAF mount) must not delete rows.
      if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
        removeTrackByPath(path)
        removed++
      }
      continue
    }
    if (size < MIN_BYTES) {
      try {
        unlinkSync(path)
      } catch {
        // best-effort: the DB row goes regardless
      }
      removeTrackByPath(path)
      removed++
      logWarn('download', `Rimossa traccia troncata non riproducibile: ${path}`)
    }
  }
  if (removed > 0) {
    logWarn('download', `Bonifica download: rimosse ${removed} tracce non valide`)
    rebuildAggregates()
    broadcast('library:changed', { reason: 'purge' })
  }
}

// ---------- scheduling ----------

let retryTimer: ReturnType<typeof setTimeout> | null = null

/** Re-arms a wake-up for the earliest scheduled retry in the future. */
function armRetryTimer(): void {
  if (retryTimer) {
    clearTimeout(retryTimer)
    retryTimer = null
  }
  const row = getDb()
    .prepare(
      "SELECT MIN(next_retry_at) AS t FROM downloads WHERE status = 'pending' AND next_retry_at > ?"
    )
    .get(Date.now()) as { t: number | null } | undefined
  if (!row?.t) return
  retryTimer = setTimeout(() => {
    retryTimer = null
    pump()
  }, Math.max(50, row.t - Date.now() + 50))
  retryTimer.unref?.()
}

function pump(): void {
  const settings = getSettings()
  const max = Math.max(1, Math.min(10, settings.downloadConcurrency))
  if (running.size >= max) return
  const next = getDb()
    .prepare(
      `SELECT id FROM downloads
       WHERE status = 'pending' AND (next_retry_at IS NULL OR next_retry_at <= ?)
       ORDER BY created_at LIMIT 1`
    )
    .get(Date.now()) as { id: number } | undefined
  if (!next) {
    armRetryTimer()
    // Queue drained: sweep stale yt-dlp temp leftovers (24h cutoff, so a
    // resume within the retry window never loses its partial fragments).
    if (running.size === 0) cleanupPartialFiles(ytdlpTempDir(), 0, true)
    return
  }
  void run(next.id)
  if (running.size < max) pump()
}

// ---------- execution ----------

async function run(id: number): Promise<void> {
  const item = getItem(id)
  if (!item || running.has(id)) return

  const controller = new AbortController()
  running.set(id, controller)

  const settings = getSettings()
  mkdirSync(settings.downloadFolder, { recursive: true })
  updateItem(id, { status: 'downloading', error_message: null })

  // Start the native download notification on the first active download.
  if (!downloadServiceActive) {
    downloadServiceActive = true
    downloadLifecycle.onStart?.()
  }

  try {
    const handler = handlerForType(item.source_type)
    const outcome = await handler.download({
      item,
      settings,
      signal: controller.signal,
      onProgress: (patch) => queueProgress(id, patch)
    })

    if (controller.signal.aborted) {
      // a paused row keeps its progress so the bar survives the pause
      updateItem(id, { status: statusOnAbort(pauseRequested.has(id)) })
      return
    }

    // integrity check: drop corrupted/truncated results
    let invalid = 0
    const validFiles: string[] = []
    for (const file of outcome.files) {
      const v = await validateAudioFile(file)
      if (v.ok) {
        validFiles.push(file)
      } else {
        invalid++
        logWarn('download', `File non valido (${v.reason}): ${file}`)
        try {
          unlinkSync(file)
        } catch {
          // already gone
        }
      }
    }
    if (outcome.files.length > 0 && validFiles.length === 0) {
      throw new DownloadError('DL_INVALID_FILES', 'transient')
    }

    const failures = outcome.partialFailures + invalid
    updateItem(id, {
      status: 'completed',
      progress: 1,
      completed_tracks: Math.max(validFiles.length, item.total_tracks - failures),
      current_file: null,
      file_path: validFiles.length === 1 ? validFiles[0] : null,
      error_message: failures > 0 ? `${failures} tracce non scaricate` : null
    })
    await onDownloadComplete(item, validFiles)
  } catch (err) {
    if (controller.signal.aborted) {
      updateItem(id, { status: statusOnAbort(pauseRequested.has(id)) })
    } else {
      scheduleRetryOrFail(id, err)
    }
  } finally {
    pauseRequested.delete(id)
    running.delete(id)
    pump()
    // Stop the native download notification once the queue has fully drained
    // (checked after pump(), which synchronously starts any next item).
    if (running.size === 0 && downloadServiceActive) {
      downloadServiceActive = false
      downloadLifecycle.onStop?.()
    }
  }
}

/**
 * Failure state machine: permanent errors stop immediately; transient
 * network errors back off exponentially (30s/60s/120s); rate limits pause
 * at least 5 minutes. After MAX_ATTEMPTS the item lands in 'error'.
 */
function scheduleRetryOrFail(id: number, err: unknown): void {
  const decision = decideRetry(err, getItem(id)?.attempts ?? 0)
  if (decision.action === 'fail') {
    updateItem(id, {
      status: 'error',
      error_message: decision.errorMessage,
      last_failure_class: decision.failureClass
    })
    return
  }
  logWarn(
    'download',
    `Download ${id} fallito (${decision.failureClass}), nuovo tentativo tra ${Math.round(decision.delayMs / 1000)}s`,
    err
  )
  updateItem(id, {
    status: 'pending',
    attempts: decision.attempts,
    next_retry_at: Date.now() + decision.delayMs,
    last_failure_class: decision.failureClass,
    error_message: decision.errorMessage
  })
  armRetryTimer()
}

/**
 * Stamp the authoritative Spotify album id on the just-ingested tracks of a Spotify
 * ALBUM download, so a rebuild merges copies of the same release sitting in different
 * folders. Returns true if any row changed (the caller then triggers the merge rebuild).
 */
function stampSpotifyAlbumId(item: DownloadItem, files: string[]): boolean {
  if (item.source_type !== 'spotify-album' || files.length === 0) return false
  const parsed = detectUrl(item.source_url)
  const albumId = parsed?.spotifyKind === 'album' ? parsed.spotifyId : undefined
  if (!albumId) return false
  const stmt = getDb().prepare('UPDATE tracks SET spotify_album_id = ? WHERE path = ?')
  let changed = false
  for (const file of files) {
    if (stmt.run(albumId, file).changes > 0) changed = true
  }
  return changed
}

async function onDownloadComplete(item: DownloadItem, files: string[]): Promise<void> {
  const settings = getSettings()

  // YouTube metadata is usually dirty (channel as artist, "(Official Video)"
  // in the title): clean it before the scan so aggregates never see it, then
  // queue the tracks for fingerprint-based enrichment.
  let enrichIds: number[] = []
  if (item.source_type.startsWith('youtube') && settings.autoFixYoutubeMetadata) {
    try {
      enrichIds = await correctYoutubeDownloads(files)
    } catch (err) {
      logWarn('download', 'Correzione metadati YouTube fallita', err)
    }
  }

  // A YouTube playlist/album: yt-dlp's per-entry album/album_artist tags diverge, which
  // used to split one release into several album cards. Unify them BEFORE the scan so
  // the whole batch resolves to a single album_key.
  if (item.source_type === 'youtube-playlist' && files.length > 1) {
    await unifyAlbumTags(files, item.album?.trim() || item.title, item.artist ?? null)
  }

  await scanFolders([settings.downloadFolder], () => undefined)

  // scanFolders already rebuilt the aggregates. Only rebuild AGAIN when we just stamped
  // a Spotify album id (so cross-folder copies of the release merge); otherwise the scan's
  // own rebuild is sufficient — no redundant pass.
  if (stampSpotifyAlbumId(item, files)) rebuildAggregates()

  broadcast('library:changed', { reason: 'download' })
  // A fresh download may duplicate a track already in the library — collapse it
  // before enrichment runs on the (possibly soon-removed) new rows.
  await autoDedupe()
  if (enrichIds.length > 0 && settings.autoEnrichEnabled) enqueueEnrichment(enrichIds)
}
