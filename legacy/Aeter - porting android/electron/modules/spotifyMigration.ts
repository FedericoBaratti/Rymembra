import { mkdirSync } from 'node:fs'
import type {
  SpotifyMigrationPreview,
  SpotifyMigrationState,
  SpotifyMigrationTrack,
  SpotifyMigrationTrackStatus
} from '@shared/types'
import { getDb } from './db'
import { getSettings } from './settings'
import { broadcast } from './events'
import { logWarn } from './logger'
import { rebuildAggregates, upsertTrackFromFile } from './library'
import { repairSplitAlbums } from './albumRepair'
import { detectUrl, type ParsedUrl } from './download/urlDetect'
import { downloadSpotifyTrack } from './download/spotifyEngine'
import type { SpotifyTrack } from './spotify/types'
import { previewSpotify, resolveSpotify } from './spotify/keyless'

// Orchestrates a full Spotify → Aether migration on top of the spotdl-free
// engine: resolve the link keyless, optionally recreate the playlist in Aether,
// then download+match every track with BOUNDED CONCURRENCY (saturate the network
// without OOM-ing the phone), add matched tracks to the playlist in their
// original order, and push a rich progress event the UI renders live.
//
// Reliability ("no song skipped"): each track outcome is classified (see
// TrackDownloadResult) so a transient failure (network/timeout/403) is RETRIED
// across additional passes instead of being silently marked "not found". The
// whole state is persisted to SQLite so a process/app kill mid-migration resumes
// the not-yet-done tracks. One migration at a time.

// --- Concurrency / retry tuning ------------------------------------------

/** Hard ceiling on parallel yt-dlp runs on mobile: each spawns a Python
 *  interpreter, so too many OOM-kill the process. The user-facing knob is
 *  settings.downloadConcurrency (default 3); we clamp it to this. */
const MAX_MIGRATION_CONCURRENCY = 4
/** Initial pass + reconciliation retries for tracks that failed transiently. */
const MIGRATION_MAX_PASSES = 3
/** Pause before a reconciliation pass so a flaky network can recover. */
const RECONCILE_BACKOFF_MS = 15_000

// --- Platform lifecycle seam (foreground service on Android) --------------
//
// The shared electron/modules code cannot reach Android-native directly; the
// node-backend installs these hooks (mirroring setYtdlpRunner/setTagWriteBack)
// to start/stop a foreground service so Android keeps the process alive — and
// therefore the WebView that the yt-dlp reverse-RPC depends on — for the whole
// migration. No-ops on desktop.

export interface MigrationLifecycle {
  onStart?(total: number): void
  onProgress?(done: number, total: number): void
  onStop?(): void
}
let lifecycle: MigrationLifecycle = {}
export function setMigrationLifecycle(l: MigrationLifecycle): void {
  lifecycle = l
}

// --- Active migration state ----------------------------------------------

let current: SpotifyMigrationState | null = null
let controller: AbortController | null = null
/** Full track metadata for the active migration, aligned by index with
 *  current.tracks (needed to (re)download; not exposed to the UI). */
let activeTracks: SpotifyTrack[] = []
let lastProgressNotify = 0

function emit(): void {
  if (current) broadcast('spotify:migration', current)
}

export function getSpotifyMigration(): SpotifyMigrationState | null {
  return current
}

export async function previewSpotifyMigration(url: string): Promise<SpotifyMigrationPreview> {
  const parsed = detectUrl(url)
  if (!parsed?.spotifyKind) throw new Error('SPOTIFY_BAD_URL')
  const info = await previewSpotify(parsed)
  return {
    kind: parsed.spotifyKind,
    title: info.title,
    artist: info.artist,
    coverUrl: info.coverUrl,
    totalTracks: info.totalTracks
  }
}

export function cancelSpotifyMigration(): void {
  if (controller) controller.abort()
}

function createPlaylist(name: string): number {
  const db = getDb()
  const now = Date.now()
  const res = db
    .prepare('INSERT INTO playlists (name, description, created_at, updated_at) VALUES (?, ?, ?, ?)')
    .run(name, 'Importata da Spotify', now, now)
  return Number(res.lastInsertRowid)
}

/** Insert a track at a fixed playlist position = its original Spotify index.
 *  Using the index (not completion order) keeps the playlist in source order
 *  despite out-of-order concurrent completion, and is idempotent on resume. */
function setPlaylistTrack(playlistId: number, trackId: number, position: number): void {
  const db = getDb()
  db.prepare(
    'INSERT OR REPLACE INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)'
  ).run(playlistId, trackId, position)
  db.prepare('UPDATE playlists SET updated_at = ? WHERE id = ?').run(Date.now(), playlistId)
}

// --- Persistence (SQLite, source of truth for resume) ---------------------

function persistHeader(): void {
  if (!current) return
  getDb()
    .prepare(
      `INSERT OR REPLACE INTO spotify_migration
       (id, migration_id, status, source_url, kind, title, cover_url, recreate_playlist, playlist_id, error, created_at, updated_at)
       VALUES (1, @migration_id, @status, @source_url, @kind, @title, @cover_url, @recreate_playlist, @playlist_id, @error, @created_at, @updated_at)`
    )
    .run({
      migration_id: current.id,
      status: current.status,
      source_url: current.sourceUrl,
      kind: current.kind,
      title: current.title,
      cover_url: current.coverUrl,
      recreate_playlist: current.recreatePlaylist ? 1 : 0,
      playlist_id: current.playlistId,
      error: current.error,
      created_at: Number(current.id) || Date.now(),
      updated_at: Date.now()
    })
}

function persistTracks(tracks: SpotifyTrack[]): void {
  const db = getDb()
  const del = db.prepare('DELETE FROM spotify_migration_tracks')
  const ins = db.prepare(
    `INSERT INTO spotify_migration_tracks
       (idx, title, artist, album, album_artist, disc_number, track_number, year, duration_ms, cover_url, status)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')`
  )
  db.transaction(() => {
    del.run()
    tracks.forEach((t, i) =>
      ins.run(
        i,
        t.title,
        t.artist ?? null,
        t.album ?? null,
        t.albumArtist ?? null,
        t.discNumber ?? null,
        t.trackNumber ?? null,
        t.year ?? null,
        t.durationMs ?? null,
        t.coverUrl ?? null
      )
    )
  })()
}

function persistTrackStatus(idx: number, status: SpotifyMigrationTrackStatus): void {
  getDb()
    .prepare('UPDATE spotify_migration_tracks SET status = ? WHERE idx = ?')
    .run(status, idx)
}

// --- Progress bookkeeping -------------------------------------------------

function recomputeCounters(): void {
  if (!current) return
  let matched = 0
  let done = 0
  for (const t of current.tracks) {
    if (t.status === 'done') matched++
    if (t.status === 'done' || t.status === 'notfound' || t.status === 'failed') done++
  }
  current.matched = matched
  current.done = done
}

function setTrackStatus(idx: number, status: SpotifyMigrationTrackStatus): void {
  if (!current) return
  current.tracks[idx].status = status
  if (status === 'downloading') current.currentTitle = current.tracks[idx].title
  recomputeCounters()
  persistTrackStatus(idx, status)
  // Throttle the foreground-service notification update (cheap, but no need to
  // spam the reverse-RPC bridge on every track transition).
  const now = Date.now()
  if (now - lastProgressNotify > 1000) {
    lastProgressNotify = now
    lifecycle.onProgress?.(current.done, current.total)
  }
  emit()
}

// --- Bounded-concurrency worker pool -------------------------------------

async function runPool(
  indices: number[],
  concurrency: number,
  worker: (index: number) => Promise<void>
): Promise<void> {
  let next = 0
  const n = Math.max(1, Math.min(concurrency, indices.length))
  const runners: Promise<void>[] = []
  for (let k = 0; k < n; k++) {
    runners.push(
      (async () => {
        for (;;) {
          const i = next++
          if (i >= indices.length) return
          await worker(indices[i])
        }
      })()
    )
  }
  await Promise.all(runners)
}

function indicesNeedingWork(): number[] {
  if (!current) return []
  const out: number[] = []
  current.tracks.forEach((t, i) => {
    if (t.status === 'pending' || t.status === 'failed') out.push(i)
  })
  return out
}

async function processTrack(idx: number, signal: AbortSignal): Promise<void> {
  if (!current || signal.aborted) return
  const status = current.tracks[idx].status
  if (status === 'done' || status === 'notfound') return // terminal — skip
  const track = activeTracks[idx]
  setTrackStatus(idx, 'downloading')

  let res: Awaited<ReturnType<typeof downloadSpotifyTrack>>
  try {
    res = await downloadSpotifyTrack(track, idx, { settings: getSettings(), signal })
  } catch (err) {
    logWarn('spotify', `Brano fallito: ${track.title}`, err)
    res = { ok: false, reason: 'failed' }
  }

  if (signal.aborted) {
    // Leave it retryable so a resume picks it up; don't mark terminal on cancel.
    setTrackStatus(idx, 'pending')
    return
  }

  if (res.ok) {
    try {
      const trackId = await upsertTrackFromFile(res.path)
      if (trackId != null && current.playlistId != null) {
        setPlaylistTrack(current.playlistId, trackId, idx)
      }
      setTrackStatus(idx, 'done')
    } catch (err) {
      logWarn('spotify', `Import in libreria fallito: ${res.path}`, err)
      setTrackStatus(idx, 'failed') // transient: retry on a later pass
    }
  } else if (res.reason === 'not-found') {
    setTrackStatus(idx, 'notfound')
  } else {
    setTrackStatus(idx, 'failed')
  }
}

function delay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) return resolve()
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    const onAbort = (): void => {
      clearTimeout(timer)
      resolve()
    }
    signal.addEventListener('abort', onAbort, { once: true })
  })
}

async function runPasses(signal: AbortSignal): Promise<void> {
  const concurrency = Math.max(
    1,
    Math.min(MAX_MIGRATION_CONCURRENCY, getSettings().downloadConcurrency)
  )
  for (let pass = 0; pass < MIGRATION_MAX_PASSES; pass++) {
    if (signal.aborted) break
    const todo = indicesNeedingWork()
    if (todo.length === 0) break
    if (pass > 0) {
      await delay(RECONCILE_BACKOFF_MS, signal)
      if (signal.aborted) break
    }
    await runPool(todo, concurrency, (i) => processTrack(i, signal))
  }
}

function finalize(signal: AbortSignal): void {
  if (!current) return
  // Surface everything we imported, even on cancel.
  rebuildAggregates()
  broadcast('library:changed', { reason: 'spotify-migration' })
  // Auto-heal any album that still ended up split (e.g. older files mixed in, or
  // an embed-fallback import without an album artist). Idempotent + best-effort.
  void repairSplitAlbums()
    .then((res) => {
      if (res.retagged > 0) broadcast('library:changed', { reason: 'repair' })
    })
    .catch((err) => logWarn('spotify', 'repairSplitAlbums dopo migrazione fallito', err))

  current.currentTitle = null
  current.notFound = current.tracks
    .filter((t) => t.status === 'notfound')
    .map((t) => ({ title: t.title, artist: t.artist }))
  current.status = signal.aborted ? 'cancelled' : 'done'
  recomputeCounters()
  persistHeader()
  emit()
  lifecycle.onStop?.()
}

// --- Public start / resume ------------------------------------------------

export async function startSpotifyMigration(opts: {
  url: string
  recreatePlaylist: boolean
}): Promise<SpotifyMigrationState> {
  if (current && current.status === 'running') return current

  const parsed = detectUrl(opts.url)
  if (!parsed?.spotifyKind) throw new Error('SPOTIFY_BAD_URL')

  controller = new AbortController()
  current = {
    id: String(Date.now()),
    status: 'resolving',
    sourceUrl: opts.url,
    kind: parsed.spotifyKind,
    title: '',
    coverUrl: null,
    recreatePlaylist: opts.recreatePlaylist,
    playlistId: null,
    total: 0,
    done: 0,
    matched: 0,
    currentTitle: null,
    tracks: [],
    notFound: [],
    error: null
  }
  activeTracks = []
  persistHeader()
  emit()

  // Run in the background; the UI follows via the 'spotify:migration' event.
  void runMigration(opts, parsed, controller.signal)
  return current
}

async function runMigration(
  opts: { url: string; recreatePlaylist: boolean },
  parsed: ParsedUrl,
  signal: AbortSignal
): Promise<void> {
  const settings = getSettings()
  const kind = parsed.spotifyKind!
  try {
    const resolved = await resolveSpotify(parsed)
    if (!current) return

    current.title = resolved.title
    current.coverUrl = resolved.coverUrl
    current.total = resolved.tracks.length
    activeTracks = resolved.tracks
    current.tracks = resolved.tracks.map<SpotifyMigrationTrack>((t) => ({
      title: t.title,
      artist: t.artist,
      status: 'pending'
    }))
    current.status = 'running'

    if (current.total === 0) {
      current.status = 'error'
      current.error = 'SPOTIFY_NO_TRACKS'
      persistHeader()
      emit()
      return
    }

    // Recreate the playlist only for multi-track containers when asked.
    if (opts.recreatePlaylist && (kind === 'playlist' || kind === 'album' || kind === 'artist')) {
      current.playlistId = createPlaylist(resolved.title || 'Playlist Spotify')
    }
    persistHeader()
    persistTracks(resolved.tracks)
    emit()

    lifecycle.onStart?.(current.total)
    mkdirSync(settings.downloadFolder, { recursive: true })

    await runPasses(signal)
    finalize(signal)
  } catch (err) {
    if (current) {
      current.status = signal.aborted ? 'cancelled' : 'error'
      current.error = err instanceof Error ? err.message : String(err)
      persistHeader()
      emit()
    }
    lifecycle.onStop?.()
  } finally {
    controller = null
  }
}

/**
 * Resume an interrupted migration on backend boot: if the persisted header is
 * still 'running'/'resolving', reload the tracks, reset any 'downloading' (from
 * the crash) to 'pending', and run the passes over the not-yet-done tracks.
 * No-op when there is nothing to resume.
 */
export async function resumeSpotifyMigration(): Promise<void> {
  if (current && current.status === 'running') return
  const db = getDb()
  const header = db.prepare('SELECT * FROM spotify_migration WHERE id = 1').get() as
    | Record<string, unknown>
    | undefined
  if (!header) return
  const status = String(header.status)
  if (status !== 'running' && status !== 'resolving') return

  const rows = db
    .prepare('SELECT * FROM spotify_migration_tracks ORDER BY idx')
    .all() as Record<string, unknown>[]
  if (rows.length === 0) return

  activeTracks = rows.map((r) => ({
    title: String(r.title),
    artist: (r.artist as string | null) ?? null,
    album: (r.album as string | null) ?? null,
    albumArtist: (r.album_artist as string | null) ?? null,
    discNumber: (r.disc_number as number | null) ?? null,
    trackNumber: (r.track_number as number | null) ?? null,
    year: (r.year as number | null) ?? null,
    durationMs: (r.duration_ms as number | null) ?? null,
    coverUrl: (r.cover_url as string | null) ?? null
  }))

  current = {
    id: String(header.migration_id),
    status: 'running',
    sourceUrl: String(header.source_url),
    kind: String(header.kind),
    title: String(header.title ?? ''),
    coverUrl: (header.cover_url as string | null) ?? null,
    recreatePlaylist: !!header.recreate_playlist,
    playlistId: (header.playlist_id as number | null) ?? null,
    total: rows.length,
    done: 0,
    matched: 0,
    currentTitle: null,
    tracks: rows.map((r) => {
      let st = String(r.status) as SpotifyMigrationTrackStatus
      if (st === 'downloading') st = 'pending' // interrupted mid-download
      return { title: String(r.title), artist: (r.artist as string | null) ?? null, status: st }
    }),
    notFound: [],
    error: null
  }
  // Persist the 'downloading' → 'pending' normalisation.
  current.tracks.forEach((_t, i) => {
    if (String(rows[i].status) === 'downloading') persistTrackStatus(i, 'pending')
  })
  recomputeCounters()

  if (indicesNeedingWork().length === 0) {
    // Nothing left — just finalise the record.
    controller = new AbortController()
    finalize(controller.signal)
    controller = null
    return
  }

  controller = new AbortController()
  const signal = controller.signal
  persistHeader()
  emit()
  logWarn('spotify', `Ripresa migrazione Spotify: ${indicesNeedingWork().length} brani rimasti`)

  void (async () => {
    try {
      lifecycle.onStart?.(current!.total)
      mkdirSync(getSettings().downloadFolder, { recursive: true })
      await runPasses(signal)
      finalize(signal)
    } catch (err) {
      if (current) {
        current.status = 'error'
        current.error = err instanceof Error ? err.message : String(err)
        persistHeader()
        emit()
      }
      lifecycle.onStop?.()
    } finally {
      controller = null
    }
  })()
}
