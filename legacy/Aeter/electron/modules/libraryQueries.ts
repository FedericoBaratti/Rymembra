/**
 * Pure library read/stat-write functions, extracted from electron/ipc/library.ipc.ts
 * so both the Electron IPC surface and the LAN HTTP server (electron/modules/lan/)
 * can call the exact same SQL without duplicating it.
 */
import type { Track, TrackQuery, SearchResults, Playlist, Album, Artist, LibraryStats } from '@shared/types'
import { foldText } from '@shared/text'
import { getDb } from './db'
import { markLibraryDirty } from './sync/dirty'
import { logWarn } from './logger'
import { evaluateSmartPlaylist, smartPlaylistSummary } from './smartPlaylists'

const SORTABLE = new Set([
  'title', 'artist', 'album', 'year', 'duration', 'rating', 'date_added', 'play_count'
])

export function queryTracks(query?: TrackQuery): Track[] {
  const db = getDb()
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
}

export function getTrackCount(): number {
  return (getDb().prepare('SELECT COUNT(*) AS n FROM tracks').get() as { n: number }).n
}

export function getTrackById(id: number): Track | null {
  return (getDb().prepare('SELECT * FROM tracks WHERE id = ?').get(id) as Track | undefined) ?? null
}

export function getTracksByIds(ids: number[]): Track[] {
  if (!Array.isArray(ids) || ids.length === 0) return []
  const db = getDb()
  const byId = new Map<number, Track>()
  for (let i = 0; i < ids.length; i += 500) {
    const chunk = ids.slice(i, i + 500)
    const rows = db
      .prepare(`SELECT * FROM tracks WHERE id IN (${chunk.map(() => '?').join(',')})`)
      .all(...chunk) as Track[]
    for (const row of rows) byId.set(row.id, row)
  }
  return ids.map((id) => byId.get(id)).filter((t): t is Track => t != null)
}

export function getAlbums(): Album[] {
  return getDb()
    .prepare('SELECT * FROM albums ORDER BY artist COLLATE NOCASE, year, title')
    .all() as Album[]
}

export function getAlbumTracks(albumId: number): Track[] {
  return getDb()
    .prepare(
      `SELECT t.* FROM tracks t JOIN albums a ON t.album_key = a.album_key
       WHERE a.id = ? ORDER BY t.disc_number, t.track_number, t.title`
    )
    .all(albumId) as Track[]
}

export function getArtists(): Artist[] {
  return getDb()
    .prepare(
      `SELECT a.*,
        (SELECT COUNT(DISTINCT t.album_key) FROM tracks t
          WHERE COALESCE(t.album_artist, t.artist) = a.name) AS album_count,
        (SELECT COUNT(*) FROM tracks t
          WHERE t.artist = a.name OR t.album_artist = a.name) AS track_count
       FROM artists a
       ORDER BY a.name COLLATE NOCASE`
    )
    .all() as Artist[]
}

export function getArtistAlbums(artistName: string): Album[] {
  return getDb()
    .prepare('SELECT * FROM albums WHERE artist = ? ORDER BY year DESC, title')
    .all(artistName) as Album[]
}

export function getLibraryStats(): LibraryStats {
  const db = getDb()
  return {
    tracks: (db.prepare('SELECT COUNT(*) n FROM tracks').get() as { n: number }).n,
    albums: (db.prepare('SELECT COUNT(*) n FROM albums').get() as { n: number }).n,
    artists: (db.prepare('SELECT COUNT(*) n FROM artists').get() as { n: number }).n,
    totalDuration: (
      db.prepare('SELECT COALESCE(SUM(duration),0) n FROM tracks').get() as { n: number }
    ).n
  }
}

export function searchLibrary(term: string): SearchResults {
  const db = getDb()
  const cleaned = term.trim()
  if (!cleaned) return { tracks: [], albums: [], artists: [] }

  // Tokens, diacritic-insensitively folded. Every token must match (AND).
  const tokens = cleaned.split(/\s+/).map(foldText).filter(Boolean)
  if (tokens.length === 0) return { tracks: [], albums: [], artists: [] }
  const matches = (haystack: string): boolean => {
    const hay = foldText(haystack)
    return tokens.every((tk) => hay.includes(tk))
  }

  // Fast path: FTS5 (better-sqlite3). A malformed query throws → fall through
  // to the JS-side fold+filter below.
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
    // malformed FTS query — fall through
  }

  // Universal fallback: fold + match in plain JS. Engine-independent and never
  // rejects towards the caller (a bare search bar with no feedback).
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
    logWarn('search', `query fallito per "${cleaned}"`, err)
    return { tracks, albums: [], artists: [] }
  }
}

export function getPlaylists(): Playlist[] {
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

export function isSmartPlaylist(playlistId: number): boolean {
  const row = getDb().prepare('SELECT is_smart FROM playlists WHERE id = ?').get(playlistId) as
    | { is_smart: number }
    | undefined
  return row?.is_smart === 1
}

export function getPlaylistTracks(playlistId: number): Track[] {
  const db = getDb()
  if (isSmartPlaylist(playlistId)) return evaluateSmartPlaylist(db, playlistId) as Track[]
  return db
    .prepare(
      `SELECT t.* FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
       WHERE pt.playlist_id = ? ORDER BY pt.position`
    )
    .all(playlistId) as Track[]
}

export function recordPlay(trackId: number, msPlayed?: number): void {
  const db = getDb()
  const now = Date.now()
  const tx = db.transaction(() => {
    db.prepare('UPDATE tracks SET play_count = play_count + 1, last_played = ? WHERE id = ?').run(
      now,
      trackId
    )
    // One row per completed play (schema v13) so stats/recommendations can
    // reason about recency + frequency beyond the aggregate play_count.
    db.prepare('INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?, ?, ?)').run(
      trackId,
      now,
      msPlayed != null && Number.isFinite(msPlayed) ? Math.round(msPlayed) : null
    )
  })
  tx()
}

export function setRating(trackId: number, rating: number): void {
  // stats_updated_at is the last-writer-wins clock the Drive sync uses.
  getDb()
    .prepare('UPDATE tracks SET rating = ?, stats_updated_at = ? WHERE id = ?')
    .run(Math.max(0, Math.min(5, Math.round(rating))), Date.now(), trackId)
  markLibraryDirty()
}

export function setLiked(trackId: number, liked: boolean): { liked: boolean } {
  const on = liked ? 1 : 0
  const now = Date.now()
  getDb()
    .prepare('UPDATE tracks SET liked = ?, liked_at = ?, stats_updated_at = ? WHERE id = ?')
    .run(on, on ? now : null, now, trackId)
  markLibraryDirty()
  return { liked: !!on }
}

export function getLikedTracks(): Track[] {
  return getDb()
    .prepare('SELECT * FROM tracks WHERE liked = 1 ORDER BY liked_at DESC, title COLLATE NOCASE')
    .all() as Track[]
}
