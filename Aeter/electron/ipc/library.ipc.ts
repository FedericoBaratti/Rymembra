import { dialog, shell, BrowserWindow } from 'electron'
import { handle } from './handle'
import type {
  Track,
  TrackQuery,
  SearchResults,
  AppSettings,
  PersistedQueue,
  SmartPlaylistRules
} from '@shared/types'
import { playlistKey } from '@shared/trackKey'
import { getDb } from '../modules/db'
import { recordTombstone } from '../modules/sync/tombstones'
import { markLibraryDirty } from '../modules/sync/dirty'
import { scanFolders } from '../modules/library'
import { repairSplitAlbums } from '../modules/albumRepair'
import { watchFolders } from '../modules/watcher'
import { getSettings, setSettings, syncNativeTheme, flushSettingsSync } from '../modules/settings'
import { logError } from '../modules/logger'
import { secretsEncryptionAvailable, flushSecretsSync } from '../modules/secrets'
import { broadcast } from '../modules/events'
import { registerMediaKeys } from '../modules/mediaKeys'
import { autoEnrichMissing } from '../modules/metadata'
import { readQueueState, writeQueueState, flushQueueStateSync } from '../modules/queueState'
import { validateRules, rulesToSql } from '../modules/smartPlaylists'
import { startLanServer, stopLanServer } from '../modules/lan/server'
import {
  queryTracks,
  getTrackCount,
  getTrackById,
  getTracksByIds,
  getAlbums,
  getAlbumTracks,
  getArtists,
  getArtistAlbums,
  getLibraryStats,
  searchLibrary,
  getPlaylists,
  getPlaylistTracks,
  isSmartPlaylist,
  recordPlay,
  setRating,
  setLiked,
  getLikedTracks
} from '../modules/libraryQueries'

export function startLibraryScan(): void {
  const settings = getSettings()
  scanFolders(settings.watchFolders, (p) => {
    broadcast('scan:progress', p)
    if (p.phase === 'done') {
      broadcast('library:changed', { reason: 'scan' })
      // low-priority background enrichment once the scan settles (opt-out via
      // the autoEnrichEnabled master switch)
      if (getSettings().autoEnrichEnabled) setTimeout(() => void autoEnrichMissing(), 5000)
    }
  }).catch((err) => logError('scan', 'Scansione libreria fallita', err))
}

export function restartWatcher(): void {
  watchFolders(getSettings().watchFolders, () => broadcast('library:changed', { reason: 'watcher' }))
}

export function registerLibraryIpc(): void {
  const db = getDb()

  handle('getTracks', (_e, query?: TrackQuery): Track[] => queryTracks(query))

  handle('getTrackCount', () => getTrackCount())

  handle('getTrackById', (_e, id: number) => getTrackById(id))

  handle('getTracksByIds', (_e, ids: number[]): Track[] => getTracksByIds(ids))

  handle('getAlbums', () => getAlbums())

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

  handle('getAlbumTracks', (_e, albumId: number) => getAlbumTracks(albumId))

  handle('getArtists', () => getArtists())

  handle('getArtistAlbums', (_e, artistName: string) => getArtistAlbums(artistName))

  handle('getLibraryStats', () => getLibraryStats())

  handle('search', (_e, term: string): SearchResults => searchLibrary(term))

  handle('rescanLibrary', () => {
    startLibraryScan()
  })

  // Persist all pending debounced writes now (renderer calls this when the app
  // is backgrounded). On desktop better-sqlite3 has no flushNow (WAL persists
  // immediately) and the JSON flushes are cheap no-ops when nothing is dirty.
  handle('flushNow', () => {
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

  handle('recordPlay', (_e, trackId: number, msPlayed?: number) => recordPlay(trackId, msPlayed))

  handle('setRating', (_e, trackId: number, rating: number) => setRating(trackId, rating))

  handle('showInFolder', (_e, trackId: number) => {
    const row = db.prepare('SELECT path FROM tracks WHERE id = ?').get(trackId) as
      | { path: string }
      | undefined
    if (row) shell.showItemInFolder(row.path)
  })

  // ---- Liked Songs (columns from schema v7) ----
  handle('setLiked', (_e, trackId: number, liked: boolean): { liked: boolean } => setLiked(trackId, liked))

  handle('getLikedTracks', (): Track[] => getLikedTracks())

  // ---- Listening stats (schema v13 play_history) ----
  // Top tracks / artists / genres over the last `periodDays` (0 = all time),
  // plus headline totals. INNER JOIN tracks drops history rows whose track was
  // deleted, so orphans never leak into the result.
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
  handle('getPlaylists', () => getPlaylists())

  handle('getPlaylistTracks', (_e, playlistId: number) => getPlaylistTracks(playlistId))

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
    return getPlaylists().find((p) => p.id === id)
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
    return getPlaylists().find((p) => p.id === id)
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
    // Capture the name before deleting so the tombstone carries its sync key.
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
    if (
      (patch.lanServerEnabled !== undefined && patch.lanServerEnabled !== before.lanServerEnabled) ||
      (patch.lanServerPort !== undefined && patch.lanServerPort !== before.lanServerPort)
    ) {
      stopLanServer()
      startLanServer()
    }
    return next
  })

  handle('getSecurityStatus', () => ({ secretsEncrypted: secretsEncryptionAvailable() }))

  handle('pickFolder', async (e): Promise<string | null> => {
    const win = BrowserWindow.fromWebContents(e.sender)
    const res = await dialog.showOpenDialog(win!, { properties: ['openDirectory'] })
    return res.canceled ? null : res.filePaths[0]
  })
}
