/**
 * Bridges the local SQLite library and the pure {@link mergeSync} function:
 * `buildSnapshot` reads the DB into a SyncFile (keyed by trackKey/playlistKey),
 * and `applyWriteback` writes a merge's decisions back into the DB. Neither the
 * DB rows' own ids nor paths cross the wire — only the derived keys do.
 */
import { getDb } from '../db'
import { readQueueState } from '../queueState'
import { trackKey, playlistKey } from '@shared/trackKey'
import { foldSyncTracks, SYNC_FILE_VERSION } from './schema'
import type { SyncFile, SyncTrack, SyncPlaylist } from './schema'
import type { Writeback } from './merge'

type Db = ReturnType<typeof getDb>

interface TrackRow {
  id: number
  title: string
  artist: string
  album: string
  album_artist: string | null
  year: number | null
  track_number: number | null
  disc_number: number | null
  duration: number
  genre: string | null
  play_count: number
  last_played: number | null
  rating: number
  liked: number
  liked_at: number | null
  stats_updated_at: number | null
  cover_art_hash: string | null
  mb_recording_id: string | null
  date_added: number
}

interface PlaylistRow {
  id: number
  name: string
  description: string | null
  created_at: number
  updated_at: number
  is_smart: number
  rules: string | null
}

function rowToTrack(r: TrackRow): SyncTrack {
  return {
    title: r.title,
    artist: r.artist,
    album: r.album,
    albumArtist: r.album_artist ?? null,
    year: r.year ?? null,
    trackNumber: r.track_number ?? null,
    discNumber: r.disc_number ?? null,
    duration: r.duration,
    genre: r.genre ?? null,
    playCount: r.play_count,
    lastPlayed: r.last_played ?? null,
    rating: r.rating,
    liked: r.liked ? 1 : 0,
    likedAt: r.liked_at ?? null,
    statsUpdatedAt: r.stats_updated_at ?? 0,
    coverArtHash: r.cover_art_hash ?? null,
    mbRecordingId: r.mb_recording_id ?? null,
    addedAt: r.date_added
  }
}

export function buildSnapshot(db: Db, deviceId: string): SyncFile {
  const now = Date.now()
  const trackRows = db
    .prepare(
      `SELECT id, title, artist, album, album_artist, year, track_number, disc_number,
              duration, genre, play_count, last_played, rating, liked, liked_at,
              stats_updated_at, cover_art_hash, mb_recording_id, date_added
       FROM tracks`
    )
    .all() as TrackRow[]

  const tracks: Record<string, SyncTrack> = {}
  const idToKey = new Map<number, string>()
  for (const r of trackRows) {
    const key = trackKey(r)
    idToKey.set(r.id, key)
    const st = rowToTrack(r)
    // two local files fold to the same key (rare, dedupe usually collapses
    // them): keep the richer play stats so nothing regresses when uploaded
    tracks[key] = tracks[key] ? foldSyncTracks(tracks[key], st) : st
  }

  // playlist membership grouped by id, in position order
  const memberRows = db
    .prepare('SELECT playlist_id, track_id FROM playlist_tracks ORDER BY playlist_id, position')
    .all() as { playlist_id: number; track_id: number }[]
  const membership = new Map<number, string[]>()
  for (const m of memberRows) {
    const key = idToKey.get(m.track_id)
    if (!key) continue
    const list = membership.get(m.playlist_id) ?? []
    list.push(key)
    membership.set(m.playlist_id, list)
  }

  const playlistRows = db
    .prepare(
      'SELECT id, name, description, created_at, updated_at, is_smart, rules FROM playlists'
    )
    .all() as PlaylistRow[]
  const playlists: Record<string, SyncPlaylist> = {}
  for (const p of playlistRows) {
    const key = playlistKey(p.name)
    const isSmart = p.is_smart ? 1 : 0
    const sp: SyncPlaylist = {
      name: p.name,
      description: p.description ?? null,
      createdAt: p.created_at,
      updatedAt: p.updated_at,
      isSmart,
      rules: p.rules ?? null,
      // smart playlists derive membership from their rules on each device
      trackKeys: isSmart ? [] : membership.get(p.id) ?? []
    }
    const existing = playlists[key]
    playlists[key] = existing && existing.updatedAt >= sp.updatedAt ? existing : sp
  }

  // tombstones, minus any key that is live again locally (reborn record)
  const tombRows = db
    .prepare('SELECT kind, key, deleted_at FROM sync_tombstones')
    .all() as { kind: string; key: string; deleted_at: number }[]
  const tombTracks: Record<string, number> = {}
  const tombPlaylists: Record<string, number> = {}
  for (const t of tombRows) {
    if (t.kind === 'track') {
      if (!tracks[t.key]) tombTracks[t.key] = t.deleted_at
    } else if (t.kind === 'playlist') {
      if (!playlists[t.key]) tombPlaylists[t.key] = t.deleted_at
    }
  }

  const file: SyncFile = {
    version: SYNC_FILE_VERSION,
    generatedAt: now,
    generatedBy: deviceId,
    tracks,
    playlists,
    tombstones: { tracks: tombTracks, playlists: tombPlaylists }
  }

  const queue = readQueueState()
  if (queue && Array.isArray(queue.trackIds) && Array.isArray(queue.order)) {
    const trackKeys = queue.order
      .map((i) => idToKey.get(queue.trackIds[i]))
      .filter((k): k is string => !!k)
    if (trackKeys.length > 0) {
      file.playback = {
        updatedAt: now,
        deviceId,
        trackKeys,
        orderPos: queue.orderPos,
        shuffle: queue.shuffle,
        repeat: queue.repeat
      }
    }
  }

  return file
}

/** Apply a merge's writeback to the local DB, in a single transaction. */
export function applyWriteback(db: Db, writeback: Writeback): void {
  const keyToId = new Map<string, number>()
  const idRows = db
    .prepare('SELECT id, artist, title, album, duration FROM tracks')
    .all() as { id: number; artist: string; title: string; album: string; duration: number }[]
  for (const r of idRows) keyToId.set(trackKey(r), r.id)

  const playlistIdByKey = new Map<string, number>()
  const plRows = db.prepare('SELECT id, name FROM playlists').all() as {
    id: number
    name: string
  }[]
  for (const r of plRows) playlistIdByKey.set(playlistKey(r.name), r.id)

  const updateStats = db.prepare(
    `UPDATE tracks SET play_count = ?, last_played = ?, rating = ?, liked = ?, liked_at = ?,
            stats_updated_at = ? WHERE id = ?`
  )
  const insertPlaylist = db.prepare(
    `INSERT INTO playlists (name, description, created_at, updated_at, is_smart, rules)
     VALUES (?, ?, ?, ?, ?, ?)`
  )
  const updatePlaylist = db.prepare(
    `UPDATE playlists SET name = ?, description = ?, updated_at = ?, is_smart = ?, rules = ?
     WHERE id = ?`
  )
  const clearMembers = db.prepare('DELETE FROM playlist_tracks WHERE playlist_id = ?')
  const insertMember = db.prepare(
    'INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)'
  )
  const deletePlaylistRow = db.prepare('DELETE FROM playlists WHERE id = ?')

  const tx = db.transaction(() => {
    for (const [key, t] of Object.entries(writeback.tracks)) {
      const id = keyToId.get(key)
      if (id == null) continue
      updateStats.run(t.playCount, t.lastPlayed, t.rating, t.liked, t.likedAt, t.statsUpdatedAt, id)
    }

    for (const [key, p] of Object.entries(writeback.playlists)) {
      let id = playlistIdByKey.get(key)
      if (id == null) {
        const res = insertPlaylist.run(
          p.name,
          p.description,
          p.createdAt,
          p.updatedAt,
          p.isSmart,
          p.rules
        )
        id = Number(res.lastInsertRowid)
        playlistIdByKey.set(key, id)
      } else {
        updatePlaylist.run(p.name, p.description, p.updatedAt, p.isSmart, p.rules, id)
      }
      if (!p.isSmart) {
        clearMembers.run(id)
        let pos = 0
        for (const tk of p.trackKeys) {
          const trackId = keyToId.get(tk)
          if (trackId != null) insertMember.run(id, trackId, pos++)
        }
      }
    }

    for (const key of writeback.deletedPlaylists) {
      const id = playlistIdByKey.get(key)
      if (id != null) deletePlaylistRow.run(id)
    }
  })
  tx()
}
