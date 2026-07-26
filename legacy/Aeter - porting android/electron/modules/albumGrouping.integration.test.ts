import { describe, expect, it, beforeEach } from 'vitest'
import { DatabaseSync } from 'node:sqlite'
import { albumGroupKey, aggregateAlbums, buildAlbumGroups, type AlbumAggInput } from './albumKey'

// Real-SQLite integration test for the album-grouping wiring (NOT just the pure
// helpers): drives split-album data through the same shape of SQL used by the v10
// migration and rebuildAggregates — album_key UNIQUE, ON CONFLICT upsert, the
// getAlbumTracks join, orphan deletion — to prove the end-to-end invariant.

const DDL = `
  CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    album TEXT NOT NULL DEFAULT '',
    artist TEXT NOT NULL DEFAULT '',
    album_artist TEXT,
    year INTEGER,
    cover_art_hash TEXT,
    album_key TEXT,
    mb_release_group_id TEXT,
    mb_release_id TEXT,
    spotify_album_id TEXT
  );
  CREATE TABLE albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    album_key TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    year INTEGER,
    total_tracks INTEGER NOT NULL DEFAULT 0,
    cover_art_hash TEXT,
    mb_album_id TEXT,
    spotify_id TEXT
  );
`

let db: DatabaseSync

interface T {
  path: string
  album: string
  artist: string
  album_artist?: string | null
  year?: number | null
  cover_art_hash?: string | null
  mb_release_group_id?: string | null
  mb_release_id?: string | null
  spotify_album_id?: string | null
}

function insert(t: T): void {
  db.prepare(
    `INSERT INTO tracks (path, album, artist, album_artist, year, cover_art_hash, album_key,
       mb_release_group_id, mb_release_id, spotify_album_id) VALUES (?,?,?,?,?,?,?,?,?,?)`
  ).run(
    t.path,
    t.album,
    t.artist,
    t.album_artist ?? null,
    t.year ?? null,
    t.cover_art_hash ?? null,
    albumGroupKey(t.album, t.path), // computed exactly as upsertTrackFromFile does
    t.mb_release_group_id ?? null,
    t.mb_release_id ?? null,
    t.spotify_album_id ?? null
  )
}

/** Mirror of the v11 rebuildAggregates album step: merge-aware (buildAlbumGroups) +
 *  canonical key write-back + orphan prune. */
function rebuildMerged(): void {
  const rows = db
    .prepare(
      `SELECT album_key, album, album_artist, artist, year, cover_art_hash,
              mb_release_group_id, mb_release_id, spotify_album_id FROM tracks`
    )
    .all() as unknown as AlbumAggInput[]
  const { albums, remap } = buildAlbumGroups(rows)
  const reKey = db.prepare('UPDATE tracks SET album_key = ? WHERE album_key = ?')
  for (const [base, canonical] of remap) if (base !== canonical) reKey.run(canonical, base)
  const upsert = db.prepare(
    `INSERT INTO albums (album_key, title, artist, year, total_tracks, cover_art_hash, mb_album_id, spotify_id)
     VALUES (?,?,?,?,?,?,?,?)
     ON CONFLICT(album_key) DO UPDATE SET
       title = excluded.title, artist = excluded.artist, year = excluded.year,
       total_tracks = excluded.total_tracks,
       cover_art_hash = COALESCE(albums.cover_art_hash, excluded.cover_art_hash),
       mb_album_id = COALESCE(excluded.mb_album_id, albums.mb_album_id),
       spotify_id = COALESCE(excluded.spotify_id, albums.spotify_id)`
  )
  for (const a of albums) {
    upsert.run(a.album_key, a.title, a.artist, a.year, a.total_tracks, a.cover_art_hash, a.mb_album_id, a.spotify_id)
  }
  db.prepare(
    'DELETE FROM albums WHERE album_key NOT IN (SELECT album_key FROM tracks WHERE album_key IS NOT NULL)'
  ).run()
}

/** Mirror of rebuildAggregates' album step (positional params for binding portability). */
function rebuild(): void {
  const rows = db
    .prepare('SELECT album_key, album, album_artist, artist, year, cover_art_hash FROM tracks')
    .all() as unknown as AlbumAggInput[]
  const upsert = db.prepare(
    `INSERT INTO albums (album_key, title, artist, year, total_tracks, cover_art_hash)
     VALUES (?,?,?,?,?,?)
     ON CONFLICT(album_key) DO UPDATE SET
       title = excluded.title, artist = excluded.artist, year = excluded.year,
       total_tracks = excluded.total_tracks,
       cover_art_hash = COALESCE(albums.cover_art_hash, excluded.cover_art_hash)`
  )
  for (const a of aggregateAlbums(rows)) {
    upsert.run(a.album_key, a.title, a.artist, a.year, a.total_tracks, a.cover_art_hash)
  }
  db.prepare(
    'DELETE FROM albums WHERE album_key NOT IN (SELECT album_key FROM tracks WHERE album_key IS NOT NULL)'
  ).run()
}

const D = '/sd/Music/'

beforeEach(() => {
  db = new DatabaseSync(':memory:')
  db.exec(DDL)
})

describe('album grouping (integration)', () => {
  it('merges a split release and keeps same-titled albums in different folders apart', () => {
    // One Thriller release whose tracks have inconsistent album_artist + an edition
    // suffix on one file — must collapse to a SINGLE album.
    insert({ path: D + 'MJ/Thriller/01.mp3', album: 'Thriller', artist: 'Michael Jackson', album_artist: 'Michael Jackson', year: 1982, cover_art_hash: 'h1' })
    insert({ path: D + 'MJ/Thriller/02.mp3', album: 'Thriller', artist: 'Michael Jackson, Paul McCartney', album_artist: null })
    insert({ path: D + 'MJ/Thriller/03.mp3', album: 'Thriller (Deluxe Edition)', artist: 'Michael Jackson', album_artist: 'Michael Jackson feat. Vincent Price', year: 1983 })
    // Two genuinely different "Greatest Hits" in different artist folders — stay apart.
    insert({ path: D + 'Queen/Greatest Hits/01.mp3', album: 'Greatest Hits', artist: 'Queen', album_artist: 'Queen' })
    insert({ path: D + 'ABBA/Greatest Hits/01.mp3', album: 'Greatest Hits', artist: 'ABBA', album_artist: 'ABBA' })

    rebuild()

    const albums = db.prepare('SELECT * FROM albums ORDER BY title, artist').all() as Array<{
      id: number; album_key: string; title: string; artist: string; total_tracks: number; year: number | null; cover_art_hash: string | null
    }>
    expect(albums).toHaveLength(3)

    const thriller = albums.find((a) => a.title === 'Thriller')!
    expect(thriller.artist).toBe('Michael Jackson') // dominant, drops feat./guest
    expect(thriller.total_tracks).toBe(3)
    expect(thriller.year).toBe(1983)
    expect(thriller.cover_art_hash).toBe('h1')

    // getAlbumTracks join returns all 3 tracks of the merged album.
    const tracks = db
      .prepare('SELECT t.* FROM tracks t JOIN albums a ON t.album_key = a.album_key WHERE a.id = ?')
      .all(thriller.id)
    expect(tracks).toHaveLength(3)

    // The two Greatest Hits remain separate.
    expect(albums.filter((a) => a.title === 'Greatest Hits')).toHaveLength(2)
  })

  it('merges the SAME release scattered across folders via a shared MusicBrainz id', () => {
    // Two copies of one release in different folders (e.g. a download scattered, or two
    // rips) carrying the same MusicBrainz release-group id → must collapse to ONE album,
    // and the join must still reach both tracks (album_key rewritten to the canonical id).
    insert({ path: D + 'Various/Disc1/01.mp3', album: 'Comp', artist: 'A', album_artist: 'VA', mb_release_group_id: 'RGZ' })
    insert({ path: D + 'Various/Elsewhere/02.mp3', album: 'Comp', artist: 'B', album_artist: 'VA', mb_release_group_id: 'RGZ' })

    // Sanity: without the id merge these are two distinct base keys (different folders).
    expect(albumGroupKey('Comp', D + 'Various/Disc1/01.mp3')).not.toBe(
      albumGroupKey('Comp', D + 'Various/Elsewhere/02.mp3')
    )

    rebuildMerged()

    const albums = db.prepare('SELECT * FROM albums').all() as Array<{
      id: number; album_key: string; total_tracks: number
    }>
    expect(albums).toHaveLength(1)
    expect(albums[0].album_key).toBe('mbrg:RGZ')
    expect(albums[0].total_tracks).toBe(2)

    const tracks = db
      .prepare('SELECT t.* FROM tracks t JOIN albums a ON t.album_key = a.album_key WHERE a.id = ?')
      .all(albums[0].id)
    expect(tracks).toHaveLength(2)
  })

  it('is idempotent and removes orphaned albums after a track is deleted', () => {
    insert({ path: D + 'A/X/1.mp3', album: 'X', artist: 'A', album_artist: 'A' })
    insert({ path: D + 'A/X/2.mp3', album: 'X', artist: 'A', album_artist: 'A' })
    insert({ path: D + 'B/Y/1.mp3', album: 'Y', artist: 'B', album_artist: 'B' })

    rebuild()
    rebuild() // second run must not duplicate rows (ON CONFLICT)
    expect((db.prepare('SELECT COUNT(*) n FROM albums').get() as { n: number }).n).toBe(2)

    // Remove every track of album Y → its album row must be pruned.
    db.prepare("DELETE FROM tracks WHERE album = 'Y'").run()
    rebuild()
    const albums = db.prepare('SELECT title FROM albums').all() as Array<{ title: string }>
    expect(albums).toHaveLength(1)
    expect(albums[0].title).toBe('X')
  })

  it('re-groups a track after its album is edited (album_key recomputed + rebuild)', () => {
    // Regression for the bug where updateTrackMetadata/updateTracksMetadata/enrichTrack
    // changed tracks.album but left the derived album_key stale: the track stayed under
    // the old album card and no new album row appeared until a full rescan.
    insert({ path: D + 'A/Old/1.mp3', album: 'Old Name', artist: 'A', album_artist: 'A' })
    rebuild()
    expect((db.prepare('SELECT title FROM albums').all() as Array<{ title: string }>).map((a) => a.title)).toEqual(['Old Name'])

    // Simulate the editor's UPDATE: it must set BOTH album and album_key (the fix),
    // exactly as updateTrackMetadata now does.
    const path = D + 'A/Old/1.mp3'
    db.prepare('UPDATE tracks SET album = ?, album_key = ? WHERE path = ?').run(
      'New Name',
      albumGroupKey('New Name', path),
      path
    )
    rebuild() // the fix also calls rebuildAggregates() after the edit

    const albums = db.prepare('SELECT title FROM albums').all() as Array<{ title: string }>
    expect(albums).toHaveLength(1)
    expect(albums[0].title).toBe('New Name')

    // The track is reachable through the getAlbumTracks join under the NEW album.
    const tracks = db
      .prepare(
        "SELECT t.* FROM tracks t JOIN albums a ON t.album_key = a.album_key WHERE a.title = 'New Name'"
      )
      .all()
    expect(tracks).toHaveLength(1)
  })
})
