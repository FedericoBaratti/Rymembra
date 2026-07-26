import { mkdirSync, readdirSync, statSync, unlinkSync, rmdirSync } from 'node:fs'
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
import { canPause, canResume, statusOnAbort } from './download/pauseTransitions'
import { correctYoutubeDownloads, unifyAlbumTags } from './download/postProcess'
import { ytdlpTempDir } from './download/tempDir'
import { enqueueEnrichment } from './enrichment/autoEnrich'

// Queue orchestrator. Source-specific work lives in download/sources/*;
// this module owns the downloads table, concurrency, automatic retry with
// backoff, cancellation and crash recovery.

export { detectUrl } from './download/urlDetect'

const running = new Map<number, AbortController>()
/** Ids whose in-flight abort means "pause", not "cancel". */
const pauseRequested = new Set<number>()

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
  const sets = keys.map((k) => `${k} = @${k}`).join(', ')
  getDb().prepare(`UPDATE downloads SET ${sets} WHERE id = @__id`).run({ ...patch, __id: id })
  const item = getItem(id)
  if (item) broadcast('download:updated', item)
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
    // Skip entirely when the folder is unreachable (e.g. external drive
    // unmounted): missing files there are NOT evidence of broken tracks.
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
      // failure (EACCES/EPERM/EIO on an unhappy drive) must not delete rows.
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

  try {
    const handler = handlerForType(item.source_type)
    const outcome = await handler.download({
      item,
      settings,
      signal: controller.signal,
      onProgress: (patch) => updateItem(id, patch)
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

async function onDownloadComplete(item: DownloadItem, files: string[]): Promise<void> {
  const settings = getSettings()

  // YouTube metadata is usually dirty (channel as artist, "(Official Video)"
  // in the title): clean it before the scan so aggregates never see it, then
  // queue the tracks for fingerprint-based enrichment.
  // A YouTube playlist/album whose per-entry yt-dlp tags diverge would scatter
  // across albums: force one album/album_artist across the batch (file tags
  // only) BEFORE the scan re-ingests and recomputes album_key.
  if (item.source_type === 'youtube-playlist' && files.length > 1) {
    try {
      await unifyAlbumTags(files, item.album?.trim() || item.title, item.artist ?? null)
    } catch (err) {
      logWarn('download', 'Unificazione album playlist YouTube fallita', err)
    }
  }

  let enrichIds: number[] = []
  if (item.source_type.startsWith('youtube') && settings.autoFixYoutubeMetadata) {
    try {
      enrichIds = await correctYoutubeDownloads(files)
    } catch (err) {
      logWarn('download', 'Correzione metadati YouTube fallita', err)
    }
  }

  await scanFolders([settings.downloadFolder], () => undefined)
  rebuildAggregates()
  broadcast('library:changed', { reason: 'download' })
  // Respect the autoEnrichEnabled master switch on the post-download path too —
  // not just the scan trigger (library.ipc.ts). Without this, disabling
  // auto-enrichment still enriched every freshly downloaded track.
  if (enrichIds.length > 0 && settings.autoEnrichEnabled) enqueueEnrichment(enrichIds)
}
