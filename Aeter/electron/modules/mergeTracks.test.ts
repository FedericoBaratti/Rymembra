import { describe, expect, it, beforeEach } from 'vitest'
import { DatabaseSync } from 'node:sqlite'
import { mergeTracksInDb, pickBestQuality, type MergeDb } from './mergeTracks'

// Minimal DDL mirroring migration v1 (+ v2 genre) for the tables the merge touches.
const DDL = `
  CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    artist TEXT NOT NULL DEFAULT '',
    album TEXT NOT NULL DEFAULT '',
    year INTEGER,
    duration REAL NOT NULL DEFAULT 0,
    bitrate INTEGER,
    sample_rate INTEGER,
    codec TEXT,
    file_size INTEGER NOT NULL DEFAULT 0,
    date_added INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    play_count INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER,
    rating INTEGER NOT NULL DEFAULT 0,
    lyrics TEXT,
    cover_art_hash TEXT,
    genre TEXT,
    acoustid_fingerprint TEXT,
    mb_recording_id TEXT
  );
  CREATE TABLE playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
  );
  CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, position)
  );
`

let db: DatabaseSync

function addTrack(over: Record<string, unknown> = {}): number {
  const row = {
    path: `C:\\music\\${Math.random()}.mp3`,
    title: 'Song',
    artist: 'Artist',
    date_added: 1000,
    date_modified: 1000,
    play_count: 0,
    rating: 0,
    last_played: null,
    lyrics: null,
    cover_art_hash: null,
    genre: null,
    year: null,
    acoustid_fingerprint: null,
    mb_recording_id: null,
    ...over
  }
  const keys = Object.keys(row)
  const res = db
    .prepare(`INSERT INTO tracks (${keys.join(', ')}) VALUES (${keys.map(() => '?').join(', ')})`)
    .run(...(keys.map((k) => (row as Record<string, unknown>)[k]) as never[]))
  return Number(res.lastInsertRowid)
}

function addPlaylist(trackIds: number[]): number {
  const res = db
    .prepare('INSERT INTO playlists (name, created_at, updated_at) VALUES (?, ?, ?)')
    .run('p', 0, 0)
  const pid = Number(res.lastInsertRowid)
  trackIds.forEach((tid, i) =>
    db
      .prepare('INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)')
      .run(pid, tid, i)
  )
  return pid
}

function playlistOrder(pid: number): { track_id: number; position: number }[] {
  return db
    .prepare('SELECT track_id, position FROM playlist_tracks WHERE playlist_id = ? ORDER BY position')
    .all(pid) as { track_id: number; position: number }[]
}

beforeEach(() => {
  db = new DatabaseSync(':memory:')
  db.exec(DDL)
})

describe('mergeTracksInDb', () => {
  it('reparents victim playlist rows to the survivor', () => {
    const a = addTrack()
    const b = addTrack()
    const other = addTrack({ title: 'Other' })
    const pid = addPlaylist([other, b])

    const outcome = mergeTracksInDb(db as unknown as MergeDb, a, [b])

    expect(outcome).toEqual({ merged: 1, playlistsUpdated: 1 })
    expect(playlistOrder(pid)).toEqual([
      { track_id: other, position: 0 },
      { track_id: a, position: 1 }
    ])
  })

  it('dedupes survivor+victim in the same playlist keeping the earliest position', () => {
    const a = addTrack()
    const b = addTrack()
    const other = addTrack({ title: 'Other' })
    const pid = addPlaylist([b, other, a])

    mergeTracksInDb(db as unknown as MergeDb, a, [b])

    // the victim sat first, so the merged entry takes its slot
    expect(playlistOrder(pid)).toEqual([
      { track_id: a, position: 0 },
      { track_id: other, position: 1 }
    ])
  })

  it('renumbers positions gaplessly across multiple victims', () => {
    const a = addTrack()
    const b = addTrack()
    const c = addTrack()
    const other = addTrack({ title: 'Other' })
    const pid = addPlaylist([b, other, c, a])

    mergeTracksInDb(db as unknown as MergeDb, a, [b, c])

    const order = playlistOrder(pid)
    expect(order.map((r) => r.position)).toEqual([0, 1])
    expect(order.map((r) => r.track_id)).toEqual([a, other])
  })

  it('aggregates stats: sum play_count, max rating/last_played, min date_added', () => {
    const a = addTrack({ play_count: 3, rating: 2, last_played: 100, date_added: 2000 })
    const b = addTrack({ play_count: 5, rating: 4, last_played: 900, date_added: 1500 })

    mergeTracksInDb(db as unknown as MergeDb, a, [b])

    const row = db.prepare('SELECT * FROM tracks WHERE id = ?').get(a) as Record<string, unknown>
    expect(row.play_count).toBe(8)
    expect(row.rating).toBe(4)
    expect(row.last_played).toBe(900)
    expect(row.date_added).toBe(1500)
  })

  it('fills missing survivor metadata from victims (survivor wins when set)', () => {
    const a = addTrack({ genre: 'Rock', lyrics: null, cover_art_hash: null, year: null })
    const b = addTrack({ genre: 'Pop', lyrics: '[00:01.00]x', cover_art_hash: 'abc', year: 1999 })

    mergeTracksInDb(db as unknown as MergeDb, a, [b])

    const row = db.prepare('SELECT * FROM tracks WHERE id = ?').get(a) as Record<string, unknown>
    expect(row.genre).toBe('Rock') // survivor already had it
    expect(row.lyrics).toBe('[00:01.00]x')
    expect(row.cover_art_hash).toBe('abc')
    expect(row.year).toBe(1999)
  })

  it('deletes victim rows and their playlist memberships', () => {
    const a = addTrack()
    const b = addTrack()
    addPlaylist([b])

    mergeTracksInDb(db as unknown as MergeDb, a, [b])

    expect(db.prepare('SELECT COUNT(*) AS n FROM tracks WHERE id = ?').get(b)).toEqual({ n: 0 })
    expect(
      db.prepare('SELECT COUNT(*) AS n FROM playlist_tracks WHERE track_id = ?').get(b)
    ).toEqual({ n: 0 })
  })

  it('ignores a survivor accidentally listed among victims and empty victim lists', () => {
    const a = addTrack({ play_count: 1 })
    expect(mergeTracksInDb(db as unknown as MergeDb, a, [a])).toEqual({
      merged: 0,
      playlistsUpdated: 0
    })
    expect(db.prepare('SELECT play_count FROM tracks WHERE id = ?').get(a)).toEqual({
      play_count: 1
    })
  })
})

describe('pickBestQuality', () => {
  const base = { bitrate: null, sample_rate: null, codec: null, file_size: 0 }

  it('prefers lossless codecs over higher-bitrate lossy', () => {
    const flac = { ...base, codec: 'FLAC', bitrate: 900 }
    const mp3 = { ...base, codec: 'MPEG 1 Layer 3', bitrate: 320 }
    expect(pickBestQuality([mp3, flac])).toBe(flac)
  })

  it('falls back to bitrate, then sample rate, then file size', () => {
    const hi = { ...base, codec: 'mp3', bitrate: 320 }
    const lo = { ...base, codec: 'mp3', bitrate: 128 }
    expect(pickBestQuality([lo, hi])).toBe(hi)

    const sr48 = { ...base, codec: 'mp3', bitrate: 320, sample_rate: 48000 }
    const sr44 = { ...base, codec: 'mp3', bitrate: 320, sample_rate: 44100 }
    expect(pickBestQuality([sr44, sr48])).toBe(sr48)

    const big = { ...base, codec: 'mp3', file_size: 9000 }
    const small = { ...base, codec: 'mp3', file_size: 100 }
    expect(pickBestQuality([small, big])).toBe(big)
  })
})
