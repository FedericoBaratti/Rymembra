import { describe, it, expect, beforeEach, vi } from 'vitest'
import Database from 'better-sqlite3'
import type { SyncFile, SyncTrack } from './schema'

// fetchMissing pulls a lot of electron-touching modules; the table-reconciliation
// logic under test only needs getDb, so stub the rest. getDb returns the shared
// in-memory `testDb`, swapped per test.
let testDb: Database.Database

vi.mock('../db', () => ({ getDb: () => testDb }))
vi.mock('../settings', () => ({ getSettings: () => ({ autoFetchMissing: true, autoFetchNetwork: 'any' }) }))
vi.mock('../events', () => ({ onBroadcast: () => () => {}, broadcast: () => {} }))
vi.mock('../logger', () => ({ logWarn: () => {} }))
vi.mock('../download/externalDownload', () => ({ downloadExternalTrack: vi.fn() }))
vi.mock('../library', () => ({ upsertTrackFromFile: vi.fn(), rebuildAggregates: vi.fn() }))
vi.mock('../tagIO', () => ({ writeTags: vi.fn() }))

import { syncMissingSet, getMissingFetchStatus } from './fetchMissing'

// better-sqlite3 is a native module built against Electron's ABI; under a plain
// `vitest` run whose Node ABI differs it can't load. Skip (don't fail) there.
const nativeSqliteOk = (() => {
  try {
    new Database(':memory:').close()
    return true
  } catch {
    return false
  }
})()

function makeDb(): Database.Database {
  const db = new Database(':memory:')
  db.exec(`
    CREATE TABLE library_fetch (
      track_key TEXT PRIMARY KEY,
      title TEXT NOT NULL DEFAULT '', artist TEXT NOT NULL DEFAULT '',
      album TEXT NOT NULL DEFAULT '', duration REAL NOT NULL DEFAULT 0,
      status TEXT NOT NULL DEFAULT 'pending', attempts INTEGER NOT NULL DEFAULT 0,
      next_retry_at INTEGER, download_id INTEGER, error TEXT,
      updated_at INTEGER NOT NULL
    );
  `)
  return db
}

function track(over: Partial<SyncTrack> = {}): SyncTrack {
  return {
    title: 't',
    artist: 'a',
    album: 'al',
    albumArtist: null,
    year: null,
    trackNumber: null,
    discNumber: null,
    duration: 200,
    genre: null,
    playCount: 0,
    lastPlayed: null,
    rating: 0,
    liked: 0,
    likedAt: null,
    statsUpdatedAt: 0,
    coverArtHash: null,
    mbRecordingId: null,
    addedAt: 0,
    ...over
  }
}

function file(tracks: Record<string, SyncTrack>): SyncFile {
  return {
    version: 1,
    generatedAt: 0,
    generatedBy: 'dev',
    tracks,
    playlists: {},
    tombstones: { tracks: {}, playlists: {} }
  }
}

function rows(): Record<string, { status: string; attempts: number; title: string }> {
  const out: Record<string, { status: string; attempts: number; title: string }> = {}
  for (const r of testDb.prepare('SELECT track_key, status, attempts, title FROM library_fetch').all() as {
    track_key: string
    status: string
    attempts: number
    title: string
  }[]) {
    out[r.track_key] = { status: r.status, attempts: r.attempts, title: r.title }
  }
  return out
}

describe.skipIf(!nativeSqliteOk)('syncMissingSet', () => {
  beforeEach(() => {
    testDb = makeDb()
  })

  it('records tracks present remotely but absent locally', () => {
    const local = file({ 'a|have|al': track() })
    const merged = file({
      'a|have|al': track(),
      'b|missing|al': track({ artist: 'b', title: 'missing', duration: 180 })
    })
    syncMissingSet(local, merged)
    const r = rows()
    expect(Object.keys(r)).toEqual(['b|missing|al'])
    expect(r['b|missing|al'].status).toBe('pending')
    expect(r['b|missing|al'].title).toBe('missing')
  })

  it('does not record tracks that exist locally', () => {
    const local = file({ 'a|have|al': track() })
    const merged = file({ 'a|have|al': track() })
    syncMissingSet(local, merged)
    expect(Object.keys(rows())).toEqual([])
  })

  it('removes a row once the track becomes local (downloaded)', () => {
    const missingKey = 'b|missing|al'
    syncMissingSet(file({}), file({ [missingKey]: track() }))
    expect(Object.keys(rows())).toEqual([missingKey])
    // next sync: the track is now local → row is cleaned up
    syncMissingSet(file({ [missingKey]: track() }), file({ [missingKey]: track() }))
    expect(Object.keys(rows())).toEqual([])
  })

  it('removes a row when the track leaves the library entirely', () => {
    const missingKey = 'b|missing|al'
    syncMissingSet(file({}), file({ [missingKey]: track() }))
    expect(Object.keys(rows())).toEqual([missingKey])
    syncMissingSet(file({}), file({})) // gone from merged → dropped
    expect(Object.keys(rows())).toEqual([])
  })

  // The fuzzy guard: a remote key that misses only because the ALBUM segment
  // drifted (e.g. 'Album sconosciuto' vs the real album) must not be re-downloaded
  // when a local artist|title match has a compatible duration.
  it('skips a remote track whose artist|title exists locally with duration within tolerance', () => {
    const local = file({ 'a|t|real album': track({ duration: 200 }) })
    const merged = file({
      'a|t|real album': track({ duration: 200 }),
      'a|t|album sconosciuto': track({ duration: 202 }) // same song, drifted album tag
    })
    syncMissingSet(local, merged)
    expect(Object.keys(rows())).toEqual([])
  })

  it('still records a same-titled track whose duration differs beyond tolerance', () => {
    const local = file({ 'a|t|album': track({ duration: 200 }) })
    const merged = file({
      'a|t|album': track({ duration: 200 }),
      'a|t|live album': track({ duration: 260 }) // live version → genuinely different
    })
    syncMissingSet(local, merged)
    expect(Object.keys(rows())).toEqual(['a|t|live album'])
  })

  it('treats an unknown duration (remote or local 0) as a fuzzy match', () => {
    const local = file({
      'a|t|album': track({ duration: 0 }),
      'b|u|album': track({ duration: 180 })
    })
    const merged = file({
      ...local.tracks,
      'a|t|other': track({ duration: 500 }), // local duration unknown → present
      'b|u|other': track({ duration: 0 }) // remote duration unknown → present
    })
    syncMissingSet(local, merged)
    expect(Object.keys(rows())).toEqual([])
  })

  it('cleans up a previously-queued row once the fuzzy guard covers it', () => {
    const key = 'a|t|album sconosciuto'
    syncMissingSet(file({}), file({ [key]: track({ duration: 201 }) }))
    expect(Object.keys(rows())).toEqual([key])
    // next sync: the same recording exists locally under its real album tag
    const local = file({ 'a|t|real album': track({ duration: 200 }) })
    syncMissingSet(local, file({ ...local.tracks, [key]: track({ duration: 201 }) }))
    expect(Object.keys(rows())).toEqual([])
  })

  it('preserves attempt/backoff state of an existing row across re-runs', () => {
    const key = 'b|missing|al'
    syncMissingSet(file({}), file({ [key]: track({ title: 'old' }) }))
    // simulate the worker having failed twice on it
    testDb.prepare("UPDATE library_fetch SET status='failed', attempts=2 WHERE track_key=?").run(key)
    // a later sync with refreshed metadata must not reset the failure state
    syncMissingSet(file({}), file({ [key]: track({ title: 'new' }) }))
    const r = rows()
    expect(r[key].status).toBe('failed')
    expect(r[key].attempts).toBe(2)
    expect(r[key].title).toBe('new') // metadata snapshot is refreshed
  })
})

describe.skipIf(!nativeSqliteOk)('getMissingFetchStatus', () => {
  beforeEach(() => {
    testDb = makeDb()
  })

  it('counts pending/active/failed and excludes done', () => {
    const now = Date.now()
    const ins = testDb.prepare(
      "INSERT INTO library_fetch (track_key, status, updated_at) VALUES (?, ?, ?)"
    )
    ins.run('k1', 'pending', now)
    ins.run('k2', 'searching', now)
    ins.run('k3', 'queued', now)
    ins.run('k4', 'failed', now)
    ins.run('k5', 'done', now)
    const s = getMissingFetchStatus()
    expect(s.pending).toBe(2) // pending + searching
    expect(s.active).toBe(1) // queued
    expect(s.failed).toBe(1)
    expect(s.total).toBe(4) // excludes done
  })
})
