import { describe, expect, it, beforeEach } from 'vitest'
import { DatabaseSync } from 'node:sqlite'
import { buildAutoCatalog, type AutoCatalog } from './autoCatalogBuild'

// Real-SQLite (node:sqlite, no native better-sqlite binding) test for the Android
// Auto browse-catalog builder: proves the browse tree shape, playable-leaf file
// paths / mediaIds, normalized track map, ReplayGain factor and search folding.

const DDL = `
  CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    artist TEXT NOT NULL DEFAULT '',
    album TEXT NOT NULL DEFAULT '',
    album_artist TEXT,
    album_key TEXT,
    year INTEGER,
    disc_number INTEGER,
    track_number INTEGER,
    duration REAL NOT NULL DEFAULT 0,
    cover_art_hash TEXT,
    play_count INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER,
    liked INTEGER NOT NULL DEFAULT 0,
    liked_at INTEGER,
    replaygain_track_gain REAL
  );
  CREATE TABLE albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    album_key TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    year INTEGER,
    cover_art_hash TEXT
  );
  CREATE TABLE playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    cover_art_hash TEXT
  );
  CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL,
    track_id INTEGER NOT NULL,
    position INTEGER NOT NULL
  );
  CREATE TABLE play_history (
    track_id INTEGER NOT NULL,
    played_at INTEGER NOT NULL,
    ms_played INTEGER
  );
`

let db: DatabaseSync

interface T {
  id: number
  title: string
  artist: string
  album: string
  album_key: string
  cover?: string | null
  disc?: number
  track?: number
  dur?: number
  liked?: number
  likedAt?: number | null
  rgDb?: number | null
}

function insertTrack(t: T): void {
  db.prepare(
    `INSERT INTO tracks (id, path, title, artist, album, album_key, cover_art_hash,
       disc_number, track_number, duration, liked, liked_at, replaygain_track_gain)
     VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)`
  ).run(
    t.id,
    `/music/${t.id}.mp3`,
    t.title,
    t.artist,
    t.album,
    t.album_key,
    t.cover ?? null,
    t.disc ?? 1,
    t.track ?? t.id,
    t.dur ?? 200,
    t.liked ?? 0,
    t.likedAt ?? null,
    t.rgDb ?? null
  )
}

function build(rgEnabled = false): AutoCatalog {
  return buildAutoCatalog(db as unknown as Parameters<typeof buildAutoCatalog>[0], {
    rgEnabled,
    rgTargetDb: -18
  })
}

beforeEach(() => {
  db = new DatabaseSync(':memory:')
  db.exec(DDL)

  // Album A by "Björk" (2 tracks), album B by "The Cure" (1 track).
  insertTrack({ id: 1, title: 'Hyperballad', artist: 'Björk', album: 'Post', album_key: 'kA', cover: 'ha', track: 1, rgDb: -6 })
  insertTrack({ id: 2, title: 'Army of Me', artist: 'Björk', album: 'Post', album_key: 'kA', cover: 'ha', track: 2, liked: 1, likedAt: 100 })
  insertTrack({ id: 3, title: 'Lullaby', artist: 'The Cure', album: 'Disintegration', album_key: 'kB', cover: 'hb', track: 1, liked: 1, likedAt: 200 })

  db.prepare(`INSERT INTO albums (album_key, title, artist, year, cover_art_hash) VALUES (?,?,?,?,?)`).run('kA', 'Post', 'Björk', 1995, 'ha')
  db.prepare(`INSERT INTO albums (album_key, title, artist, year, cover_art_hash) VALUES (?,?,?,?,?)`).run('kB', 'Disintegration', 'The Cure', 1989, 'hb')

  db.prepare(`INSERT INTO playlists (id, name, cover_art_hash) VALUES (?,?,?)`).run(7, 'Roadtrip', 'hp')
  db.prepare(`INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?,?,?)`).run(7, 3, 0)
  db.prepare(`INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?,?,?)`).run(7, 1, 1)

  // Track 3 played most recently.
  db.prepare(`INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?,?,?)`).run(1, 1000, 200000)
  db.prepare(`INSERT INTO play_history (track_id, played_at, ms_played) VALUES (?,?,?)`).run(3, 5000, 200000)
})

describe('buildAutoCatalog', () => {
  it('produces the expected root sections', () => {
    const c = build()
    expect(c.version).toBe(1)
    expect(c.root).toEqual(['recent', 'liked', 'albums', 'artists', 'playlists'])
  })

  it('normalizes tracks once with real file paths and mediaIds', () => {
    const c = build()
    expect(Object.keys(c.tracks).sort()).toEqual(['track:1', 'track:2', 'track:3'])
    expect(c.tracks['track:1'].path).toBe('/music/1.mp3')
    expect(c.tracks['track:1'].durationMs).toBe(200000)
    expect(c.tracks['track:1'].coverHash).toBe('ha')
    expect(c.tracks['track:1'].subtitle).toBe('Björk')
  })

  it('orders album tracks by disc/track and links them', () => {
    const c = build()
    expect(c.nodes['albums'].children).toEqual(['album:kB', 'album:kA']) // Disintegration < Post
    expect(c.nodes['album:kA'].trackIds).toEqual(['track:1', 'track:2']) // Army of Me is track 2
    expect(c.nodes['album:kA'].coverHash).toBe('ha')
  })

  it('builds Artists → Albums hierarchy', () => {
    const c = build()
    expect(c.nodes['artists'].children).toEqual(['artist:Björk', 'artist:The Cure'])
    expect(c.nodes['artist:Björk'].children).toEqual(['album:kA'])
  })

  it('orders Liked by liked_at desc and recent by played_at desc', () => {
    const c = build()
    expect(c.nodes['liked'].trackIds).toEqual(['track:3', 'track:2']) // likedAt 200 > 100
    expect(c.nodes['recent'].trackIds).toEqual(['track:3', 'track:1']) // played_at 5000 > 1000
  })

  it('preserves playlist order', () => {
    const c = build()
    expect(c.nodes['playlist:7'].title).toBe('Roadtrip')
    expect(c.nodes['playlist:7'].trackIds).toEqual(['track:3', 'track:1'])
  })

  it('folds title/artist/album for accent-insensitive voice search', () => {
    const c = build()
    // "Björk" folds to ascii "bjork"
    expect(c.tracks['track:1'].fold).toContain('bjork')
    expect(c.tracks['track:1'].fold).toContain('hyperballad')
  })

  it('computes ReplayGain only when enabled', () => {
    expect(build(false).tracks['track:1'].replayGain).toBe(1)
    // enabled: -6 dB track gain, target -18 → gainDb = -6 + (-18 - -18) = -6 → 10^(-6/20)
    const rg = build(true).tracks['track:1'].replayGain
    expect(rg).toBeCloseTo(Math.pow(10, -6 / 20), 5)
    expect(build(true).tracks['track:2'].replayGain).toBe(1) // no gain tag → unity
  })
})
