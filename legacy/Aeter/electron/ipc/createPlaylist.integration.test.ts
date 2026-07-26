import { beforeEach, describe, expect, it } from 'vitest'
import { DatabaseSync } from 'node:sqlite'

// Real-SQLite integration test for the createPlaylist transaction shape
// (library.ipc.ts): the `playlists` INSERT now lives INSIDE the same
// transaction as the `playlist_tracks` INSERTs, so an FK violation (e.g. a
// podcast episode's synthetic negative id sneaking into the queue-save path)
// rolls back the playlist row too — no ghost empty playlist left behind.
// The handler itself can't be imported here (db.ts pulls the native
// better-sqlite3 addon, unloadable on this machine — repo-wide convention),
// so this drives the exact same SQL through node:sqlite with FKs ON.

const DDL = `
  CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL
  );
  CREATE TABLE playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
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

/** Mirrors the createPlaylist handler body: one tx around BOTH inserts. */
function createPlaylistTx(name: string, trackIds: number[]): number {
  const now = Date.now()
  db.exec('BEGIN')
  try {
    const res = db
      .prepare('INSERT INTO playlists (name, description, created_at, updated_at) VALUES (?, ?, ?, ?)')
      .run(name, null, now, now)
    const id = Number(res.lastInsertRowid)
    const ins = db.prepare(
      'INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)'
    )
    trackIds.forEach((tid, i) => ins.run(id, tid, i))
    db.exec('COMMIT')
    return id
  } catch (err) {
    db.exec('ROLLBACK')
    throw err
  }
}

beforeEach(() => {
  db = new DatabaseSync(':memory:')
  db.exec('PRAGMA foreign_keys = ON')
  db.exec(DDL)
  db.prepare('INSERT INTO tracks (title) VALUES (?)').run('Brano 1')
  db.prepare('INSERT INTO tracks (title) VALUES (?)').run('Brano 2')
})

describe('createPlaylist transaction (FK ON)', () => {
  it('creates playlist + rows for valid track ids', () => {
    const id = createPlaylistTx('Coda 01/07/2026', [1, 2])
    const rows = db
      .prepare('SELECT track_id, position FROM playlist_tracks WHERE playlist_id = ? ORDER BY position')
      .all(id) as { track_id: number; position: number }[]
    expect(rows).toEqual([
      { track_id: 1, position: 0 },
      { track_id: 2, position: 1 }
    ])
  })

  it('leaves NO ghost playlist when a track id violates the FK', () => {
    // -7 = a podcast episode's synthetic id: no tracks row → FK violation.
    expect(() => createPlaylistTx('Coda fantasma', [1, -7])).toThrow()
    const playlists = db.prepare('SELECT COUNT(*) AS n FROM playlists').get() as { n: number }
    const links = db.prepare('SELECT COUNT(*) AS n FROM playlist_tracks').get() as { n: number }
    expect(playlists.n).toBe(0) // the rollback removed the playlist row too
    expect(links.n).toBe(0)
  })
})
