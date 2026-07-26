import { describe, expect, it, beforeEach } from 'vitest'
import { DatabaseSync } from 'node:sqlite'
import { computeEnrichmentStats, listEnrichmentTracks, type StatsDb } from './stats'

// Minimal tracks DDL + the v4/v6 migrations applied on top.
const DDL = `
  CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    artist TEXT NOT NULL DEFAULT '',
    album TEXT NOT NULL DEFAULT '',
    cover_art_hash TEXT,
    mb_recording_id TEXT,
    date_added INTEGER NOT NULL DEFAULT 0
  );
  CREATE TABLE downloads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_type TEXT NOT NULL,
    file_path TEXT
  );
`

const MIGRATION_V4 = `
  ALTER TABLE tracks ADD COLUMN enrich_status TEXT;
  ALTER TABLE tracks ADD COLUMN enrich_attempted_at INTEGER;
  UPDATE tracks SET enrich_status = 'ok' WHERE mb_recording_id IS NOT NULL;
  CREATE INDEX idx_tracks_enrich ON tracks(enrich_status);
`

// Mirrors db.ts v6: track provenance + backfill from completed downloads.
const MIGRATION_V6 = `
  ALTER TABLE tracks ADD COLUMN source TEXT;
  UPDATE tracks SET source = 'youtube' WHERE path IN (
    SELECT file_path FROM downloads
    WHERE source_type LIKE 'youtube%' AND file_path IS NOT NULL
  );
`

let db: DatabaseSync

function addTrack(over: Record<string, unknown> = {}): number {
  const row = {
    path: `C:\\music\\${Math.random()}.mp3`,
    title: 'Song',
    artist: 'Artist',
    album: 'Album',
    cover_art_hash: 'hash',
    mb_recording_id: null,
    enrich_status: null,
    ...over
  }
  const keys = Object.keys(row)
  const res = db
    .prepare(`INSERT INTO tracks (${keys.join(', ')}) VALUES (${keys.map(() => '?').join(', ')})`)
    .run(...(keys.map((k) => (row as Record<string, unknown>)[k]) as never[]))
  return Number(res.lastInsertRowid)
}

beforeEach(() => {
  db = new DatabaseSync(':memory:')
  db.exec(DDL)
  db.exec(MIGRATION_V4)
  db.exec(MIGRATION_V6)
})

describe('migration v4 backfill', () => {
  it('marks already-matched tracks as ok', () => {
    const fresh = new DatabaseSync(':memory:')
    fresh.exec(DDL)
    fresh
      .prepare('INSERT INTO tracks (path, mb_recording_id) VALUES (?, ?)')
      .run('a.mp3', 'mbid-1')
    fresh.prepare('INSERT INTO tracks (path) VALUES (?)').run('b.mp3')
    fresh.exec(MIGRATION_V4)
    const rows = fresh.prepare('SELECT path, enrich_status FROM tracks ORDER BY path').all()
    expect(rows).toEqual([
      { path: 'a.mp3', enrich_status: 'ok' },
      { path: 'b.mp3', enrich_status: null }
    ])
  })
})

describe('migration v6 backfill', () => {
  it('marks tracks downloaded from youtube via the downloads table', () => {
    const fresh = new DatabaseSync(':memory:')
    fresh.exec(DDL)
    fresh.exec(MIGRATION_V4)
    fresh.prepare('INSERT INTO tracks (path) VALUES (?)').run('yt.mp3')
    fresh.prepare('INSERT INTO tracks (path) VALUES (?)').run('local.mp3')
    fresh
      .prepare('INSERT INTO downloads (source_type, file_path) VALUES (?, ?)')
      .run('youtube-video', 'yt.mp3')
    fresh.exec(MIGRATION_V6)
    const rows = fresh.prepare('SELECT path, source FROM tracks ORDER BY path').all()
    expect(rows).toEqual([
      { path: 'local.mp3', source: null },
      { path: 'yt.mp3', source: 'youtube' }
    ])
  })
})

describe('computeEnrichmentStats', () => {
  it('counts buckets independently', () => {
    addTrack({ enrich_status: 'ok', mb_recording_id: 'x' })
    addTrack({ enrich_status: 'no-match' })
    addTrack({ enrich_status: 'no-match' })
    addTrack({ enrich_status: 'needs-review' })
    addTrack({ enrich_status: 'error' })
    // pending: candidate (missing cover, no mbid, never attempted)
    addTrack({ cover_art_hash: null })
    // not pending: cover present and known artist/album
    addTrack({})

    const stats = computeEnrichmentStats(db as unknown as StatsDb)
    expect(stats).toEqual({
      total: 7,
      ok: 1,
      noMatch: 2,
      needsReview: 1,
      error: 1,
      pending: 1,
      missingCovers: 1
    })
  })

  it('treats unknown artist/album as pending candidates', () => {
    addTrack({ artist: 'Artista sconosciuto' })
    addTrack({ album: 'Album sconosciuto' })
    const stats = computeEnrichmentStats(db as unknown as StatsDb)
    expect(stats.pending).toBe(2)
  })

  it('treats never-attempted youtube tracks as pending even with complete metadata', () => {
    addTrack({ source: 'youtube' })
    expect(computeEnrichmentStats(db as unknown as StatsDb).pending).toBe(1)
  })

  it('excludes youtube tracks already attempted or matched', () => {
    addTrack({ source: 'youtube', enrich_status: 'no-match' })
    addTrack({ source: 'youtube', enrich_status: 'ok', mb_recording_id: 'x' })
    expect(computeEnrichmentStats(db as unknown as StatsDb).pending).toBe(0)
  })

  it('returns zeros on an empty library', () => {
    expect(computeEnrichmentStats(db as unknown as StatsDb)).toEqual({
      total: 0,
      ok: 0,
      noMatch: 0,
      needsReview: 0,
      error: 0,
      pending: 0,
      missingCovers: 0
    })
  })
})

describe('listEnrichmentTracks', () => {
  it('lists tracks per bucket with paging', () => {
    addTrack({ enrich_status: 'no-match', title: 'B' })
    addTrack({ enrich_status: 'no-match', title: 'A' })
    addTrack({ enrich_status: 'error', title: 'C' })

    const noMatch = listEnrichmentTracks(db as unknown as StatsDb, 'no-match')
    expect(noMatch.map((tr) => tr.title)).toEqual(['A', 'B'])

    const paged = listEnrichmentTracks(db as unknown as StatsDb, 'no-match', 1, 1)
    expect(paged.map((tr) => tr.title)).toEqual(['B'])

    expect(listEnrichmentTracks(db as unknown as StatsDb, 'error')).toHaveLength(1)
  })

  it('needs-review bucket lists abstained tracks', () => {
    addTrack({ enrich_status: 'needs-review', title: 'R' })
    addTrack({ enrich_status: 'no-match' })
    const rows = listEnrichmentTracks(db as unknown as StatsDb, 'needs-review')
    expect(rows.map((tr) => tr.title)).toEqual(['R'])
  })

  it('pending bucket only returns never-attempted candidates', () => {
    addTrack({ cover_art_hash: null }) // candidate
    addTrack({ cover_art_hash: null, enrich_status: 'no-match' }) // already attempted
    addTrack({ cover_art_hash: null, mb_recording_id: 'x' }) // matched
    expect(listEnrichmentTracks(db as unknown as StatsDb, 'pending')).toHaveLength(1)
  })
})
