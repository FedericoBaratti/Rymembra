import { describe, it, expect, beforeEach, vi } from 'vitest'
import Database from 'better-sqlite3'

// snapshot.ts only uses getDb/readQueueState as fallbacks; the functions under
// test take the db explicitly, so stub those imports to avoid pulling electron.
vi.mock('../db', () => ({ getDb: () => { throw new Error('unused in test') } }))
vi.mock('../queueState', () => ({ readQueueState: () => null }))

import { buildSnapshot, applyWriteback } from './snapshot'
import type { Writeback } from './merge'
import { trackKey, playlistKey } from '@shared/trackKey'

type Db = Database.Database

// better-sqlite3 is a native module built against Electron's ABI; under a plain
// `vitest` run whose Node ABI differs it can't load. Skip (don't fail) there —
// snapshot/writeback also get exercised end-to-end by the real app in Phase 5.
const nativeSqliteOk = (() => {
  try {
    new Database(':memory:').close()
    return true
  } catch {
    return false
  }
})()

function makeDb(): Db {
  const db = new Database(':memory:')
  db.exec(`
    CREATE TABLE tracks (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      path TEXT, title TEXT, artist TEXT, album TEXT, album_artist TEXT,
      year INTEGER, track_number INTEGER, disc_number INTEGER,
      duration REAL NOT NULL DEFAULT 0, genre TEXT,
      play_count INTEGER NOT NULL DEFAULT 0, last_played INTEGER,
      rating INTEGER NOT NULL DEFAULT 0, liked INTEGER NOT NULL DEFAULT 0, liked_at INTEGER,
      stats_updated_at INTEGER, cover_art_hash TEXT, mb_recording_id TEXT,
      date_added INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE playlists (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL, description TEXT,
      created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
      is_smart INTEGER NOT NULL DEFAULT 0, rules TEXT
    );
    CREATE TABLE playlist_tracks (
      playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
      track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
      position INTEGER NOT NULL, PRIMARY KEY (playlist_id, position)
    );
    CREATE TABLE sync_tombstones (
      kind TEXT NOT NULL, key TEXT NOT NULL, deleted_at INTEGER NOT NULL,
      PRIMARY KEY (kind, key)
    );
  `)
  db.pragma('foreign_keys = ON')
  return db
}

function insertTrack(db: Db, over: Record<string, unknown> = {}): number {
  const t = {
    path: '/m/x.mp3', title: 'Song', artist: 'Artist', album: 'Album',
    duration: 200, play_count: 0, rating: 0, liked: 0, date_added: 1000,
    stats_updated_at: null as number | null, ...over
  }
  const res = db
    .prepare(
      `INSERT INTO tracks (path, title, artist, album, duration, play_count, rating, liked,
        liked_at, stats_updated_at, date_added)
       VALUES (@path, @title, @artist, @album, @duration, @play_count, @rating, @liked,
        @liked_at, @stats_updated_at, @date_added)`
    )
    .run({ liked_at: null, ...t })
  return Number(res.lastInsertRowid)
}

describe.skipIf(!nativeSqliteOk)('buildSnapshot', () => {
  let db: Db
  beforeEach(() => {
    db = makeDb()
  })

  it('keys tracks by trackKey and maps columns/nulls', () => {
    insertTrack(db, { title: 'Déjà', artist: 'A', album: 'B', duration: 200.4, play_count: 7 })
    const snap = buildSnapshot(db, 'dev1')
    const key = trackKey({ artist: 'A', title: 'Déjà', album: 'B' })
    expect(snap.tracks[key]).toBeDefined()
    expect(snap.tracks[key].playCount).toBe(7)
    expect(snap.tracks[key].statsUpdatedAt).toBe(0) // NULL → 0
    expect(snap.generatedBy).toBe('dev1')
  })

  it('drops a tombstone whose key is live again, keeps a dead one', () => {
    insertTrack(db, { title: 'Alive', duration: 100 })
    const liveKey = trackKey({ artist: 'Artist', title: 'Alive', album: 'Album' })
    db.prepare('INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES (?, ?, ?)').run(
      'track', liveKey, 5
    )
    db.prepare('INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES (?, ?, ?)').run(
      'track', 'dead|dead||1', 9
    )
    const snap = buildSnapshot(db, 'dev1')
    expect(snap.tombstones.tracks[liveKey]).toBeUndefined()
    expect(snap.tombstones.tracks['dead|dead||1']).toBe(9)
  })

  it('resolves playlist membership to ordered trackKeys; smart → empty', () => {
    const a = insertTrack(db, { title: 'One', duration: 10 })
    const b = insertTrack(db, { title: 'Two', duration: 20 })
    const kA = trackKey({ artist: 'Artist', title: 'One', album: 'Album' })
    const kB = trackKey({ artist: 'Artist', title: 'Two', album: 'Album' })
    const pl = db
      .prepare('INSERT INTO playlists (name, created_at, updated_at) VALUES (?, ?, ?)')
      .run('My Mix', 1, 2)
    const pid = Number(pl.lastInsertRowid)
    db.prepare('INSERT INTO playlist_tracks VALUES (?, ?, 0)').run(pid, b) // b first
    db.prepare('INSERT INTO playlist_tracks VALUES (?, ?, 1)').run(pid, a)
    db.prepare('INSERT INTO playlists (name, created_at, updated_at, is_smart, rules) VALUES (?, ?, ?, 1, ?)').run(
      'Smart', 1, 2, '{"any":true}'
    )
    const snap = buildSnapshot(db, 'dev1')
    expect(snap.playlists[playlistKey('My Mix')].trackKeys).toEqual([kB, kA])
    expect(snap.playlists[playlistKey('Smart')].trackKeys).toEqual([])
    expect(snap.playlists[playlistKey('Smart')].isSmart).toBe(1)
  })
})

describe.skipIf(!nativeSqliteOk)('applyWriteback', () => {
  let db: Db
  beforeEach(() => {
    db = makeDb()
  })

  it('updates stats on the track matching a writeback key', () => {
    insertTrack(db, { title: 'Song', duration: 200, play_count: 1, rating: 0 })
    const key = trackKey({ artist: 'Artist', title: 'Song', album: 'Album' })
    const wb: Writeback = {
      tracks: {
        [key]: {
          title: 'Song', artist: 'Artist', album: 'Album', albumArtist: null,
          year: null, trackNumber: null, discNumber: null, duration: 200, genre: null,
          playCount: 9, lastPlayed: 123, rating: 4, liked: 1, likedAt: 456,
          statsUpdatedAt: 789, coverArtHash: null, mbRecordingId: null, addedAt: 1000
        }
      },
      playlists: {},
      deletedPlaylists: []
    }
    applyWriteback(db, wb)
    const row = db.prepare('SELECT play_count, rating, liked, stats_updated_at FROM tracks').get() as {
      play_count: number; rating: number; liked: number; stats_updated_at: number
    }
    expect(row).toMatchObject({ play_count: 9, rating: 4, liked: 1, stats_updated_at: 789 })
  })

  it('creates a remote playlist with resolvable members and deletes another', () => {
    const a = insertTrack(db, { title: 'One', duration: 10 })
    const kA = trackKey({ artist: 'Artist', title: 'One', album: 'Album' })
    db.prepare('INSERT INTO playlists (name, created_at, updated_at) VALUES (?, ?, ?)').run('Old', 1, 1)
    const wb: Writeback = {
      tracks: {},
      playlists: {
        [playlistKey('Fresh')]: {
          name: 'Fresh', description: null, createdAt: 5, updatedAt: 6,
          isSmart: 0, rules: null, trackKeys: [kA, 'missing|x||9']
        }
      },
      deletedPlaylists: [playlistKey('Old')]
    }
    applyWriteback(db, wb)
    const names = db.prepare('SELECT name FROM playlists ORDER BY name').all() as { name: string }[]
    expect(names.map((n) => n.name)).toEqual(['Fresh'])
    const members = db
      .prepare('SELECT track_id FROM playlist_tracks ORDER BY position')
      .all() as { track_id: number }[]
    expect(members.map((m) => m.track_id)).toEqual([a]) // only the resolvable key
  })
})
