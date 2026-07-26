import type Database from 'better-sqlite3'
import { foldText } from '@shared/text'

/**
 * Pure builder for the Android Auto browse-catalog snapshot. Kept free of any
 * fs/electron/better-sqlite value imports so it is unit-testable against a
 * node:sqlite fixture DB (all backend tests avoid the native better-sqlite
 * binding, which can't load under the test runner). The `Database` import is
 * type-only and erased at compile time.
 *
 * See autoCatalog.ts for the runtime writer/scheduler that persists this.
 */

const RECENT_LIMIT = 60

export interface AutoTrackEntry {
  title: string
  subtitle: string
  path: string
  durationMs: number
  coverHash: string | null
  /** Linear ReplayGain factor (matches the warm-path value sent to setQueue). */
  replayGain: number
  /** Folded "title artist album" for native voice/search matching. */
  fold: string
}

export interface AutoNode {
  title: string
  coverHash?: string | null
  /** Child node ids (a browsable node). */
  children?: string[]
  /** Playable leaf mediaIds ("track:<id>"). */
  trackIds?: string[]
}

export interface AutoCatalog {
  version: 1
  generatedAt: number
  /** Ordered top-level node ids shown at the Auto root. */
  root: string[]
  nodes: Record<string, AutoNode>
  tracks: Record<string, AutoTrackEntry>
}

interface TrackRow {
  id: number
  title: string
  artist: string
  album: string
  album_key: string | null
  album_artist: string | null
  disc_number: number | null
  track_number: number | null
  duration: number
  cover_art_hash: string | null
  path: string
  replaygain_track_gain: number | null
}

const TRACK_COLS =
  'id, title, artist, album, album_key, album_artist, disc_number, track_number, duration, cover_art_hash, path, replaygain_track_gain'

/** REPLAYGAIN_TRACK_GAIN (dB) → linear multiplier — mirrors replayGainLinear() in
 *  src/lib/player.ts so car playback matches the in-app ReplayGain exactly. */
export function replayGainLinear(
  trackGainDb: number | null,
  enabled: boolean,
  targetDb: number
): number {
  let gainDb = 0
  if (enabled && trackGainDb != null) {
    gainDb = Math.max(-24, Math.min(12, trackGainDb + (targetDb - -18)))
  }
  return Math.pow(10, gainDb / 20)
}

/**
 * Build the catalog object from a DB handle. Pure (reads the DB + the passed RG
 * options only), so it is unit-testable against a fixture database.
 */
export function buildAutoCatalog(
  db: Database.Database,
  opts: { rgEnabled: boolean; rgTargetDb: number }
): AutoCatalog {
  const nodes: Record<string, AutoNode> = {}
  const tracks: Record<string, AutoTrackEntry> = {}

  const addTrack = (r: TrackRow): string => {
    const mediaId = `track:${r.id}`
    if (!tracks[mediaId]) {
      const subtitle = r.artist || r.album_artist || ''
      tracks[mediaId] = {
        title: r.title || '',
        subtitle,
        path: r.path,
        durationMs: Math.round((r.duration || 0) * 1000),
        coverHash: r.cover_art_hash,
        replayGain: replayGainLinear(r.replaygain_track_gain, opts.rgEnabled, opts.rgTargetDb),
        fold: foldText(`${r.title || ''} ${subtitle} ${r.album || ''}`)
      }
    }
    return mediaId
  }

  const root: string[] = []

  // --- Recently played (distinct tracks, newest first) -----------------------
  try {
    const recent = db
      .prepare(
        `SELECT ${TRACK_COLS} FROM tracks t
         JOIN (SELECT track_id, MAX(played_at) AS mp FROM play_history GROUP BY track_id) h
           ON h.track_id = t.id
         ORDER BY h.mp DESC LIMIT ?`
      )
      .all(RECENT_LIMIT) as TrackRow[]
    if (recent.length > 0) {
      nodes['recent'] = { title: 'Ascoltati di recente', trackIds: recent.map(addTrack) }
      root.push('recent')
    }
  } catch {
    // play_history exists from schema v12; ignore if a partial/older DB lacks it.
  }

  // --- Liked -----------------------------------------------------------------
  const liked = db
    .prepare(
      `SELECT ${TRACK_COLS} FROM tracks WHERE liked = 1 ORDER BY liked_at DESC, title COLLATE NOCASE`
    )
    .all() as TrackRow[]
  if (liked.length > 0) {
    nodes['liked'] = { title: 'Brani che ti piacciono', trackIds: liked.map(addTrack) }
    root.push('liked')
  }

  // --- Albums → tracks -------------------------------------------------------
  const albums = db
    .prepare(
      `SELECT album_key, title, artist, cover_art_hash FROM albums
       WHERE album_key IS NOT NULL ORDER BY title COLLATE NOCASE`
    )
    .all() as { album_key: string; title: string; artist: string; cover_art_hash: string | null }[]
  if (albums.length > 0) {
    const albumTracksStmt = db.prepare(
      `SELECT ${TRACK_COLS} FROM tracks WHERE album_key = ?
       ORDER BY disc_number, track_number, title COLLATE NOCASE`
    )
    const albumChildren: string[] = []
    for (const al of albums) {
      const nodeId = `album:${al.album_key}`
      const rows = albumTracksStmt.all(al.album_key) as TrackRow[]
      if (rows.length === 0) continue
      nodes[nodeId] = {
        title: al.title || '',
        coverHash: al.cover_art_hash,
        trackIds: rows.map(addTrack)
      }
      albumChildren.push(nodeId)
    }
    if (albumChildren.length > 0) {
      nodes['albums'] = { title: 'Album', children: albumChildren }
      root.push('albums')
    }
  }

  // --- Artists → albums → tracks ---------------------------------------------
  const artists = db
    .prepare(`SELECT DISTINCT artist FROM albums WHERE artist <> '' ORDER BY artist COLLATE NOCASE`)
    .all() as { artist: string }[]
  if (artists.length > 0) {
    const artistAlbumsStmt = db.prepare(
      `SELECT album_key FROM albums WHERE artist = ? ORDER BY year, title COLLATE NOCASE`
    )
    const artistChildren: string[] = []
    for (const ar of artists) {
      const albumKeys = artistAlbumsStmt.all(ar.artist) as { album_key: string | null }[]
      const childNodeIds: string[] = []
      for (const a of albumKeys) {
        if (a.album_key && nodes[`album:${a.album_key}`]) childNodeIds.push(`album:${a.album_key}`)
      }
      if (childNodeIds.length === 0) continue
      const nodeId = `artist:${ar.artist}`
      nodes[nodeId] = { title: ar.artist, children: childNodeIds }
      artistChildren.push(nodeId)
    }
    if (artistChildren.length > 0) {
      nodes['artists'] = { title: 'Artisti', children: artistChildren }
      root.push('artists')
    }
  }

  // --- Playlists → tracks ----------------------------------------------------
  const playlists = db
    .prepare(`SELECT id, name, cover_art_hash FROM playlists ORDER BY name COLLATE NOCASE`)
    .all() as { id: number; name: string; cover_art_hash: string | null }[]
  if (playlists.length > 0) {
    const plCols = TRACK_COLS.split(', ')
      .map((c) => `t.${c}`)
      .join(', ')
    const plTracksStmt = db.prepare(
      `SELECT ${plCols} FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
       WHERE pt.playlist_id = ? ORDER BY pt.position`
    )
    const plChildren: string[] = []
    for (const pl of playlists) {
      const rows = plTracksStmt.all(pl.id) as TrackRow[]
      if (rows.length === 0) continue
      const nodeId = `playlist:${pl.id}`
      nodes[nodeId] = {
        title: pl.name || '',
        coverHash: pl.cover_art_hash,
        trackIds: rows.map(addTrack)
      }
      plChildren.push(nodeId)
    }
    if (plChildren.length > 0) {
      nodes['playlists'] = { title: 'Playlist', children: plChildren }
      root.push('playlists')
    }
  }

  return { version: 1, generatedAt: Date.now(), root, nodes, tracks }
}
