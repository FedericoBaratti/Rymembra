import { dialog, shell, BrowserWindow } from 'electron'
import { handle } from './handle'
import type {
  Track,
  TrackQuery,
  SearchResults,
  Playlist,
  AppSettings,
  PersistedQueue,
  SmartPlaylistRules
} from '@shared/types'
import { foldText } from '@shared/text'
import { playlistKey } from '@shared/trackKey'
import { getDb } from '../modules/db'
import { recordTombstone } from '../modules/sync/tombstones'
import { markLibraryDirty } from '../modules/sync/dirty'
import { scanFolders } from '../modules/library'
import { repairSplitAlbums } from '../modules/albumRepair'
import { watchFolders } from '../modules/watcher'
import { getSettings, setSettings, syncNativeTheme, flushSettingsSync } from '../modules/settings'
import { secretsEncryptionAvailable, flushSecretsSync } from '../modules/secrets'
import { broadcast } from '../modules/events'
import { scheduleAutoCatalogRefresh } from '../modules/auto/autoCatalog'
import { registerMediaKeys } from '../modules/mediaKeys'
import { autoEnrichMissing, autoDedupe, ensureAutoEnrichScheduler } from '../modules/metadata'
import { readQueueState, writeQueueState, flushQueueStateSync } from '../modules/queueState'
import { logWarn } from '../modules/logger'
import { setAppState } from '../modules/appLifecycle'
import {
  evaluateSmartPlaylist,
  smartPlaylistSummary,
  validateRules,
  rulesToSql
} from '../modules/smartPlaylists'

const SORTABLE = new Set([
  'title', 'artist', 'album', 'year', 'duration', 'rating', 'date_added', 'play_count'
])

export function startLibraryScan(): void {
  const settings = getSettings()
  // Arm the idle re-sweep once (shared desktop+Android entry point), so stale
  // needs-review/no-match tracks keep getting retried on a stable library.
  ensureAutoEnrichScheduler()
  scanFolders(settings.watchFolders, (p) => {
    broadcast('scan:progress', p)
    if (p.phase === 'done') {
      broadcast('library:changed', { reason: 'scan' })
      // Collapse duplicates just imported by this scan BEFORE enriching, so the
      // network-bound enrichment never runs on tracks about to be removed.
      void autoDedupe().finally(() => {
        // low-priority background enrichment once the scan settles (opt-out via
        // the autoEnrichEnabled master switch)
        if (getSettings().autoEnrichEnabled) setTimeout(() => void autoEnrichMissing(), 5000)
      })
    }
  }).catch((err) => logWarn('scan', 'Scansione libreria fallita', err))
}

export function restartWatcher(): void {
  watchFolders(getSettings().watchFolders, () => broadcast('library:changed', { reason: 'watcher' }))
}

function playlistSummaries(): Playlist[] {
  const db = getDb()
  const rows = db
    .prepare(
      `SELECT p.*, COUNT(pt.track_id) AS track_count, COALESCE(SUM(t.duration), 0) AS total_duration
       FROM playlists p
       LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
       LEFT JOIN tracks t ON t.id = pt.track_id
       GROUP BY p.id ORDER BY p.updated_at DESC`
    )
    .all() as (Playlist & { cover_hashes?: string[] })[]
  const coverRows = db
    .prepare(
      `SELECT pt.playlist_id, t.cover_art_hash FROM playlist_tracks pt
       JOIN tracks t ON t.id = pt.track_id
       WHERE t.cover_art_hash IS NOT NULL
       ORDER BY pt.playlist_id, pt.position`
    )
    .all() as { playlist_id: number; cover_art_hash: string }[]
  const covers = new Map<number, Set<string>>()
  for (const { playlist_id, cover_art_hash } of coverRows) {
    let set = covers.get(playlist_id)
    if (!set) covers.set(playlist_id, (set = new Set()))
    if (set.size < 4) set.add(cover_art_hash)
  }
  for (const row of rows) {
    if (row.is_smart) {
      const summary = smartPlaylistSummary(db, row.rules)
      row.track_count = summary.track_count
      row.total_duration = summary.total_duration
      row.cover_hashes = summary.cover_hashes
    } else {
      row.cover_hashes = [...(covers.get(row.id) ?? [])]
    }
  }
  return rows
}

function isSmartPlaylist(playlistId: number): boolean {
  const row = getDb().prepare('SELECT is_smart FROM playlists WHERE id = ?').get(playlistId) as
    | { is_smart: number }
    | undefined
  return row?.is_smart === 1
}

export function registerLibraryIpc(): void {
  const db = getDb()

  handle('getTracks', (_e, query?: TrackQuery): Track[] => {
    const q = query ?? {}
    const sortBy = q.sortBy && SORTABLE.has(q.sortBy) ? q.sortBy : 'artist'
    const dir = q.sortDir === 'desc' ? 'DESC' : 'ASC'
    const params: unknown[] = []
    let where = ''
    if (q.albumId != null) {
      where = `WHERE album_key = (SELECT album_key FROM albums WHERE id = ?)`
      params.push(q.albumId)
    } else if (q.artistName) {
      where = 'WHERE artist = ? OR album_artist = ?'
      params.push(q.artistName, q.artistName)
    }
    const order =
      q.albumId != null
        ? 'ORDER BY disc_number ASC NULLS FIRST, track_number ASC NULLS LAST'
        : `ORDER BY ${sortBy} COLLATE NOCASE ${dir}, album COLLATE NOCASE, disc_number, track_number`
    const limit = q.limit != null ? `LIMIT ${Number(q.limit)} OFFSET ${Number(q.offset ?? 0)}` : ''
    return db.prepare(`SELECT * FROM tracks ${where} ${order} ${limit}`).all(...params) as Track[]
  })

  handle('getTrackCount', () => {
    return (db.prepare('SELECT COUNT(*) AS n FROM tracks').get() as { n: number }).n
  })

  handle('getTrackById', (_e, id: number) => {
    return (db.prepare('SELECT * FROM tracks WHERE id = ?').get(id) as Track | undefined) ?? null
  })

  handle('getTracksByIds', (_e, ids: number[]): Track[] => {
    if (!Array.isArray(ids) || ids.length === 0) return []
    const byId = new Map<number, Track>()
    for (let i = 0; i < ids.length; i += 500) {
      const chunk = ids.slice(i, i + 500)
      const rows = db
        .prepare(`SELECT * FROM tracks WHERE id IN (${chunk.map(() => '?').join(',')})`)
        .all(...chunk) as Track[]
      for (const row of rows) byId.set(row.id, row)
    }
    return ids.map((id) => byId.get(id)).filter((t): t is Track => t != null)
  })

  handle('getAlbums', () => {
    return db.prepare('SELECT * FROM albums ORDER BY artist COLLATE NOCASE, year, title').all()
  })

  // Heal albums split across multiple entries by an inconsistent album_artist
  // (legacy Spotify-migration imports). Re-tags the affected files and rebuilds
  // aggregates; idempotent. Broadcasts library:changed so the UI refreshes.
  handle('repairSplitAlbums', async () => {
    const res = await repairSplitAlbums()
    // repairSplitAlbums always rebuilds aggregates (album_key grouping), so refresh
    // the UI regardless of whether any file was re-tagged.
    broadcast('library:changed', { reason: 'repair' })
    return res
  })

  handle('getAlbumTracks', (_e, albumId: number) => {
    return db
      .prepare(
        `SELECT t.* FROM tracks t JOIN albums a ON t.album_key = a.album_key
         WHERE a.id = ? ORDER BY t.disc_number, t.track_number, t.title`
      )
      .all(albumId)
  })

  handle('getArtists', () => {
    return db
      .prepare(
        `SELECT a.*,
          (SELECT COUNT(DISTINCT t.album_key) FROM tracks t
            WHERE COALESCE(t.album_artist, t.artist) = a.name) AS album_count,
          (SELECT COUNT(*) FROM tracks t
            WHERE t.artist = a.name OR t.album_artist = a.name) AS track_count
         FROM artists a
         ORDER BY a.name COLLATE NOCASE`
      )
      .all()
  })

  handle('getArtistAlbums', (_e, artistName: string) => {
    return db
      .prepare('SELECT * FROM albums WHERE artist = ? ORDER BY year DESC, title')
      .all(artistName)
  })

  handle('getLibraryStats', () => {
    return {
      tracks: (db.prepare('SELECT COUNT(*) n FROM tracks').get() as { n: number }).n,
      albums: (db.prepare('SELECT COUNT(*) n FROM albums').get() as { n: number }).n,
      artists: (db.prepare('SELECT COUNT(*) n FROM artists').get() as { n: number }).n,
      totalDuration: (
        db.prepare('SELECT COALESCE(SUM(duration),0) n FROM tracks').get() as { n: number }
      ).n
    }
  })

  handle('search', (_e, term: string): SearchResults => {
    const cleaned = term.trim()
    if (!cleaned) return { tracks: [], albums: [], artists: [] }

    // Tokens, diacritic-insensitively folded. Every token must match (AND).
    const tokens = cleaned.split(/\s+/).map(foldText).filter(Boolean)
    if (tokens.length === 0) return { tracks: [], albums: [], artists: [] }
    const matches = (haystack: string): boolean => {
      const hay = foldText(haystack)
      return tokens.every((tk) => hay.includes(tk))
    }

    // Fast path: FTS5 (desktop / better-sqlite3 only). Absent on Android, where
    // the DB is sql.js (WASM) — the prepare throws and we fall through to the
    // JS-side fold+filter below.
    let tracks: Track[] = []
    try {
      const ftsQuery = tokens.map((t) => `"${t.replace(/"/g, '')}"*`).join(' ')
      tracks = db
        .prepare(
          `SELECT t.* FROM tracks_fts f JOIN tracks t ON t.id = f.rowid
           WHERE tracks_fts MATCH ? ORDER BY rank LIMIT 50`
        )
        .all(ftsQuery) as Track[]
    } catch {
      // no FTS table (Android) or malformed query — fall through
    }

    // Universal fallback: fold + match in plain JS. This deliberately does NOT
    // use a custom SQL function (afold): on nodejs-mobile the sql.js (V8 7.8)
    // create_function callback dispatch is unreliable and threw at call time,
    // which made search() reject and return nothing on Android. Pure-JS folding
    // (normalize is fully supported under small-icu) is engine-independent.
    try {
      if (tracks.length === 0) {
        tracks = (db.prepare('SELECT * FROM tracks').all() as Track[])
          .filter((t) => matches(`${t.title} ${t.artist} ${t.album}`))
          .sort((a, b) => {
            if ((b.play_count ?? 0) !== (a.play_count ?? 0))
              return (b.play_count ?? 0) - (a.play_count ?? 0)
            const ta = foldText(a.title)
            const tb = foldText(b.title)
            return ta < tb ? -1 : ta > tb ? 1 : 0
          })
          .slice(0, 50)
      }

      const albums = (db.prepare('SELECT * FROM albums').all() as SearchResults['albums'])
        .filter((al) => matches(`${al.title} ${al.artist}`))
        .slice(0, 20)
      const artists = (
        db.prepare('SELECT *, 0 AS album_count, 0 AS track_count FROM artists').all() as SearchResults['artists']
      )
        .filter((ar) => matches(ar.name))
        .slice(0, 20)

      return { tracks, albums, artists }
    } catch (err) {
      // Search must never reject towards the renderer (a bare search bar with
      // no feedback); log the engine error and degrade to empty sections.
      logWarn('search', `query fallito per "${cleaned}"`, err)
      return { tracks, albums: [], artists: [] }
    }
  })

  handle('rescanLibrary', () => {
    startLibraryScan()
  })

  // Persist all pending writes now. Called from the renderer when the app is
  // backgrounded (MainActivity.onPause → window.aether.flushNow, plus a
  // visibilitychange fallback). On Android the nodejs-mobile process is killed
  // without warning when backgrounded, so this closes the window where the
  // debounced DB/settings/queue writes would otherwise be lost. On desktop the
  // JSON flushes are cheap no-ops if nothing is dirty and better-sqlite3 has no
  // flushNow (WAL persists immediately).
  handle('flushNow', () => {
    try {
      ;(db as unknown as { flushNow?: () => void }).flushNow?.()
    } catch {
      /* best effort */
    }
    try {
      flushSettingsSync()
    } catch {
      /* best effort */
    }
    try {
      flushQueueStateSync()
    } catch {
      /* best effort */
    }
    try {
      flushSecretsSync()
    } catch {
      /* best effort */
    }
  })

  // Android only: the renderer reports foreground/background transitions so the
  // backend can pause periodic work (missing-fetch worker, Drive sync, enrich
  // re-sweep) and relax the DB flush cadence while the screen is off. Desktop
  // never invokes this → state stays 'foreground'.
  handle('setAppState', (_e, next: 'foreground' | 'background') => {
    setAppState(next)
  })

  handle('recordPlay', (_e, trackId: number, msPlayed?: number) => {
    const now = Date.now()
    const tx = db.transaction(() => {
      db.prepare('UPDATE tracks SET play_count = play_count + 1, last_played = ? WHERE id = ?').run(
        now,
        trackId
      )
      // One row per completed play (schema v12) so stats/recommendations can
      // reason about recency + frequency beyond the aggregate play_count.
      db.prepare('INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?, ?, ?)').run(
        trackId,
        now,
        msPlayed != null && Number.isFinite(msPlayed) ? Math.round(msPlayed) : null
      )
    })
    tx()
  })

  handle('setRating', (_e, trackId: number, rating: number) => {
    // stats_updated_at is the last-writer-wins clock the Drive sync uses.
    db.prepare('UPDATE tracks SET rating = ?, stats_updated_at = ? WHERE id = ?').run(
      Math.max(0, Math.min(5, Math.round(rating))),
      Date.now(),
      trackId
    )
    markLibraryDirty()
  })

  // ---- Liked Songs (schema v12) ----
  handle('setLiked', (_e, trackId: number, liked: boolean): { liked: boolean } => {
    const on = liked ? 1 : 0
    const now = Date.now()
    db.prepare(
      'UPDATE tracks SET liked = ?, liked_at = ?, stats_updated_at = ? WHERE id = ?'
    ).run(on, on ? now : null, now, trackId)
    // Keep the Android Auto "Liked" node fresh (no-op on desktop).
    scheduleAutoCatalogRefresh()
    markLibraryDirty()
    return { liked: !!on }
  })

  handle('getLikedTracks', (): Track[] => {
    return db
      .prepare('SELECT * FROM tracks WHERE liked = 1 ORDER BY liked_at DESC, title COLLATE NOCASE')
      .all() as Track[]
  })

  // ---- Listening stats (schema v12 play_history) ----
  // Top tracks / artists / genres over the last `periodDays` (0 = all time),
  // plus headline totals. Wrapped-style surface. INNER JOIN tracks drops history
  // rows whose track was deleted, so orphans never leak into the result.
  handle('getListeningStats', (_e, periodDays?: number) => {
    const since =
      periodDays && periodDays > 0 ? Date.now() - periodDays * 24 * 60 * 60 * 1000 : 0
    const totals = db
      .prepare(
        `SELECT COUNT(*) AS plays,
                COUNT(DISTINCT h.track_id) AS unique_tracks,
                COALESCE(SUM(h.ms_played), 0) AS ms_played
         FROM play_history h JOIN tracks t ON t.id = h.track_id
         WHERE h.played_at >= ?`
      )
      .get(since) as { plays: number; unique_tracks: number; ms_played: number }
    const topTracks = db
      .prepare(
        `SELECT t.*, COUNT(*) AS plays FROM play_history h JOIN tracks t ON t.id = h.track_id
         WHERE h.played_at >= ? GROUP BY h.track_id ORDER BY plays DESC, t.title COLLATE NOCASE
         LIMIT 50`
      )
      .all(since) as (Track & { plays: number })[]
    const topArtists = db
      .prepare(
        `SELECT t.artist AS name, COUNT(*) AS plays,
                COUNT(DISTINCT h.track_id) AS tracks
         FROM play_history h JOIN tracks t ON t.id = h.track_id
         WHERE h.played_at >= ? AND t.artist IS NOT NULL AND t.artist <> ''
         GROUP BY t.artist COLLATE NOCASE ORDER BY plays DESC LIMIT 30`
      )
      .all(since) as { name: string; plays: number; tracks: number }[]
    const topGenres = db
      .prepare(
        `SELECT t.genre AS name, COUNT(*) AS plays
         FROM play_history h JOIN tracks t ON t.id = h.track_id
         WHERE h.played_at >= ? AND t.genre IS NOT NULL AND t.genre <> ''
         GROUP BY t.genre COLLATE NOCASE ORDER BY plays DESC LIMIT 20`
      )
      .all(since) as { name: string; plays: number }[]
    return { periodDays: periodDays ?? 0, totals, topTracks, topArtists, topGenres }
  })

  handle('showInFolder', (_e, trackId: number) => {
    const row = db.prepare('SELECT path FROM tracks WHERE id = ?').get(trackId) as
      | { path: string }
      | undefined
    if (row) shell.showItemInFolder(row.path)
  })

  // ---- waveform cache ----
  handle('getWaveform', (_e, trackId: number): number[] | null => {
    const row = db.prepare('SELECT peaks FROM waveforms WHERE track_id = ?').get(trackId) as
      | { peaks: Buffer }
      | undefined
    if (!row) return null
    return [...new Uint8Array(row.peaks)].map((v) => v / 255)
  })

  handle('saveWaveform', (_e, trackId: number, peaks: number[]) => {
    const buf = Buffer.from(peaks.map((p) => Math.max(0, Math.min(255, Math.round(p * 255)))))
    db.prepare('INSERT OR REPLACE INTO waveforms (track_id, peaks) VALUES (?, ?)').run(trackId, buf)
  })

  // ---- playlists ----
  handle('getPlaylists', () => playlistSummaries())

  handle('getPlaylistTracks', (_e, playlistId: number) => {
    if (isSmartPlaylist(playlistId)) return evaluateSmartPlaylist(db, playlistId)
    return db
      .prepare(
        `SELECT t.* FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
         WHERE pt.playlist_id = ? ORDER BY pt.position`
      )
      .all(playlistId)
  })

  // Live preview for the smart-playlist editor: runs the rules without saving.
  handle('previewSmartPlaylist', (_e, rules: SmartPlaylistRules) => {
    const { where, params, orderBy, limitSql } = rulesToSql(rules)
    return db.prepare(`SELECT * FROM tracks ${where} ${orderBy} ${limitSql}`).all(...params) as Track[]
  })

  handle('createSmartPlaylist', (_e, name: string, rules: SmartPlaylistRules) => {
    const validated = validateRules(rules)
    const now = Date.now()
    const res = db
      .prepare(
        `INSERT INTO playlists (name, description, created_at, updated_at, is_smart, rules)
         VALUES (?, NULL, ?, ?, 1, ?)`
      )
      .run(String(name), now, now, JSON.stringify(validated))
    const id = Number(res.lastInsertRowid)
    return playlistSummaries().find((p) => p.id === id)
  })

  handle('setSmartPlaylistRules', (_e, playlistId: number, name: string, rules: SmartPlaylistRules) => {
    const validated = validateRules(rules)
    db.prepare('UPDATE playlists SET name = ?, rules = ?, updated_at = ? WHERE id = ? AND is_smart = 1').run(
      String(name),
      JSON.stringify(validated),
      Date.now(),
      playlistId
    )
  })

  handle('createPlaylist', (_e, name: string, description?: string, trackIds?: number[]) => {
    const now = Date.now()
    // Single transaction: if a track insert fails (e.g. FK violation), the
    // playlist row is rolled back too — no ghost empty playlist left behind.
    const tx = db.transaction(() => {
      const res = db
        .prepare('INSERT INTO playlists (name, description, created_at, updated_at) VALUES (?, ?, ?, ?)')
        .run(name, description ?? null, now, now)
      const id = Number(res.lastInsertRowid)
      if (trackIds?.length) {
        const ins = db.prepare(
          'INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)'
        )
        trackIds.forEach((tid, i) => ins.run(id, tid, i))
      }
      return id
    })
    const id = tx() as number
    markLibraryDirty()
    return playlistSummaries().find((p) => p.id === id)
  })

  handle('renamePlaylist', (_e, playlistId: number, name: string, description?: string) => {
    db.prepare('UPDATE playlists SET name = ?, description = ?, updated_at = ? WHERE id = ?').run(
      name,
      description ?? null,
      Date.now(),
      playlistId
    )
    markLibraryDirty()
  })

  handle('deletePlaylist', (_e, playlistId: number) => {
    // Compute the tombstone key from the name BEFORE the row is gone, so an
    // explicit delete propagates and isn't resurrected by another device's copy.
    const row = db.prepare('SELECT name FROM playlists WHERE id = ?').get(playlistId) as
      | { name: string }
      | undefined
    db.prepare('DELETE FROM playlists WHERE id = ?').run(playlistId)
    if (row) recordTombstone('playlist', playlistKey(row.name))
    markLibraryDirty()
  })

  handle('addToPlaylist', (_e, playlistId: number, trackIds: number[]) => {
    if (isSmartPlaylist(playlistId)) return
    const max = db
      .prepare('SELECT COALESCE(MAX(position), -1) AS m FROM playlist_tracks WHERE playlist_id = ?')
      .get(playlistId) as { m: number }
    const ins = db.prepare(
      'INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)'
    )
    const tx = db.transaction(() => {
      trackIds.forEach((tid, i) => ins.run(playlistId, tid, max.m + 1 + i))
      db.prepare('UPDATE playlists SET updated_at = ? WHERE id = ?').run(Date.now(), playlistId)
    })
    tx()
    markLibraryDirty()
  })

  handle('removeFromPlaylist', (_e, playlistId: number, positions: number[]) => {
    if (isSmartPlaylist(playlistId)) return
    const tx = db.transaction(() => {
      for (const pos of positions) {
        db.prepare('DELETE FROM playlist_tracks WHERE playlist_id = ? AND position = ?').run(
          playlistId,
          pos
        )
      }
      const rows = db
        .prepare(
          'SELECT track_id FROM playlist_tracks WHERE playlist_id = ? ORDER BY position'
        )
        .all(playlistId) as { track_id: number }[]
      db.prepare('DELETE FROM playlist_tracks WHERE playlist_id = ?').run(playlistId)
      const ins = db.prepare(
        'INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)'
      )
      rows.forEach((r, i) => ins.run(playlistId, r.track_id, i))
    })
    tx()
    markLibraryDirty()
  })

  handle('reorderPlaylist', (_e, playlistId: number, trackIdsInOrder: number[]) => {
    if (isSmartPlaylist(playlistId)) return
    const tx = db.transaction(() => {
      db.prepare('DELETE FROM playlist_tracks WHERE playlist_id = ?').run(playlistId)
      const ins = db.prepare(
        'INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)'
      )
      trackIdsInOrder.forEach((tid, i) => ins.run(playlistId, tid, i))
      db.prepare('UPDATE playlists SET updated_at = ? WHERE id = ?').run(Date.now(), playlistId)
    })
    tx()
    markLibraryDirty()
  })

  // ---- queue persistence ----
  handle('getQueueState', (): PersistedQueue | null => readQueueState())

  handle('saveQueueState', (_e, state: PersistedQueue) => {
    if (!state || !Array.isArray(state.trackIds) || !Array.isArray(state.order)) return
    writeQueueState(state)
  })

  // ---- settings ----
  handle('getSettings', (): AppSettings => getSettings())

  handle('setSettings', (_e, patch: Partial<AppSettings>): AppSettings => {
    const before = getSettings()
    const next = setSettings(patch)
    if (patch.watchFolders && JSON.stringify(before.watchFolders) !== JSON.stringify(next.watchFolders)) {
      restartWatcher()
      startLibraryScan()
    }
    if (patch.globalMediaKeys !== undefined && patch.globalMediaKeys !== before.globalMediaKeys) {
      registerMediaKeys()
    }
    if (patch.theme && patch.theme !== before.theme) {
      syncNativeTheme()
    }
    return next
  })

  handle('getSecurityStatus', () => ({ secretsEncrypted: secretsEncryptionAvailable() }))

  handle('pickFolder', async (e): Promise<{ canceled: boolean; path?: string }> => {
    const win = BrowserWindow.fromWebContents(e.sender)
    const res = await dialog.showOpenDialog(win!, { properties: ['openDirectory'] })
    if (res.canceled || res.filePaths.length === 0) return { canceled: true }
    return { canceled: false, path: res.filePaths[0] }
  })
}
