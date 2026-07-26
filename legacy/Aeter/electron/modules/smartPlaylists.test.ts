import { describe, expect, it, beforeEach } from 'vitest'
import { DatabaseSync } from 'node:sqlite'
import type { SmartPlaylistRules, Track } from '@shared/types'
import {
  evaluateSmartPlaylist,
  rulesToSql,
  smartPlaylistSummary,
  validateRules,
  type SmartDb
} from './smartPlaylists'

// Minimal DDL mirroring migration v1 (+ v2) for the columns rules can touch.
const DDL = `
  CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    artist TEXT NOT NULL DEFAULT '',
    album TEXT NOT NULL DEFAULT '',
    genre TEXT,
    year INTEGER,
    duration REAL NOT NULL DEFAULT 0,
    disc_number INTEGER,
    track_number INTEGER,
    date_added INTEGER NOT NULL,
    play_count INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER,
    rating INTEGER NOT NULL DEFAULT 0,
    cover_art_hash TEXT
  );
  CREATE TABLE playlists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    is_smart INTEGER NOT NULL DEFAULT 0,
    rules TEXT
  );
`

let db: DatabaseSync

function addTrack(over: Record<string, unknown> = {}): void {
  const row = {
    path: `C:\\music\\${Math.random()}.mp3`,
    title: 'Song',
    artist: 'Artist',
    album: 'Album',
    genre: null,
    year: null,
    duration: 100,
    date_added: 1000,
    play_count: 0,
    last_played: null,
    rating: 0,
    cover_art_hash: null,
    ...over
  }
  const cols = Object.keys(row)
  db.prepare(`INSERT INTO tracks (${cols.join(',')}) VALUES (${cols.map(() => '?').join(',')})`).run(
    ...(Object.values(row) as never[])
  )
}

function evalRules(rules: SmartPlaylistRules): Track[] {
  const { where, params, orderBy, limitSql } = rulesToSql(rules)
  return db
    .prepare(`SELECT * FROM tracks ${where} ${orderBy} ${limitSql}`)
    .all(...(params as never[])) as unknown as Track[]
}

beforeEach(() => {
  db = new DatabaseSync(':memory:')
  db.exec(DDL)
})

describe('validateRules', () => {
  it('rejects unknown fields and ops (SQL-injection probe)', () => {
    expect(() =>
      validateRules({ combinator: 'and', rules: [{ field: 'id; DROP TABLE tracks' as never, op: 'eq', value: 1 }] })
    ).toThrow()
    expect(() =>
      validateRules({ combinator: 'and', rules: [{ field: 'title', op: '= 1 OR 1=1 --' as never, value: 1 }] })
    ).toThrow()
  })

  it('clamps the limit into 1..10000', () => {
    const base: SmartPlaylistRules = { combinator: 'and', rules: [] }
    expect(validateRules({ ...base, limit: 0 }).limit).toBe(1)
    expect(validateRules({ ...base, limit: 99_999 }).limit).toBe(10_000)
    expect(validateRules({ ...base, limit: -3 }).limit).toBe(1)
  })

  it('normalizes a bad combinator and sort field', () => {
    const v = validateRules({
      combinator: 'xor' as never,
      rules: [],
      sortBy: 'path' as never,
      sortDir: 'sideways' as never
    })
    expect(v.combinator).toBe('and')
    expect(v.sortBy).toBeUndefined()
    expect(v.sortDir).toBeUndefined()
  })
})

describe('rule evaluation', () => {
  it('eq / neq with NULL semantics', () => {
    addTrack({ title: 'Alpha', genre: 'rock' })
    addTrack({ title: 'Beta', genre: 'jazz' })
    addTrack({ title: 'Gamma', genre: null })
    const eq = evalRules({ combinator: 'and', rules: [{ field: 'genre', op: 'eq', value: 'rock' }] })
    expect(eq.map((t) => t.title)).toEqual(['Alpha'])
    // neq must also match rows where the column is NULL
    const neq = evalRules({ combinator: 'and', rules: [{ field: 'genre', op: 'neq', value: 'rock' }] })
    expect(neq.map((t) => t.title).sort()).toEqual(['Beta', 'Gamma'])
  })

  it('contains / not_contains, with NULL matching not_contains', () => {
    addTrack({ artist: 'The Beatles', genre: 'rock classico' })
    addTrack({ artist: 'Beach Boys', genre: 'surf' })
    addTrack({ artist: 'Orphan', genre: null })
    const c = evalRules({ combinator: 'and', rules: [{ field: 'genre', op: 'contains', value: 'rock' }] })
    expect(c.map((t) => t.artist)).toEqual(['The Beatles'])
    const nc = evalRules({ combinator: 'and', rules: [{ field: 'genre', op: 'not_contains', value: 'rock' }] })
    expect(nc.map((t) => t.artist).sort()).toEqual(['Beach Boys', 'Orphan'])
  })

  it('numeric comparisons gt/gte/lt/lte coerce values', () => {
    addTrack({ year: 1999 })
    addTrack({ year: 2005 })
    addTrack({ year: 2020 })
    expect(evalRules({ combinator: 'and', rules: [{ field: 'year', op: 'gt', value: '2000' }] })).toHaveLength(2)
    expect(evalRules({ combinator: 'and', rules: [{ field: 'year', op: 'gte', value: 2005 }] })).toHaveLength(2)
    expect(evalRules({ combinator: 'and', rules: [{ field: 'year', op: 'lt', value: 2005 }] })).toHaveLength(1)
    expect(evalRules({ combinator: 'and', rules: [{ field: 'year', op: 'lte', value: 2005 }] })).toHaveLength(2)
  })

  it('in_last_days converts to a timestamp window', () => {
    const now = Date.now()
    addTrack({ title: 'Recent', last_played: now - 86_400_000 })
    addTrack({ title: 'Old', last_played: now - 30 * 86_400_000 })
    addTrack({ title: 'Never', last_played: null })
    const got = evalRules({
      combinator: 'and',
      rules: [{ field: 'last_played', op: 'in_last_days', value: 7 }]
    })
    expect(got.map((t) => t.title)).toEqual(['Recent'])
  })

  it('or combinator unions clauses', () => {
    addTrack({ genre: 'rock', rating: 0 })
    addTrack({ genre: 'jazz', rating: 5 })
    addTrack({ genre: 'pop', rating: 1 })
    const got = evalRules({
      combinator: 'or',
      rules: [
        { field: 'genre', op: 'eq', value: 'rock' },
        { field: 'rating', op: 'gte', value: 5 }
      ]
    })
    expect(got).toHaveLength(2)
  })

  it('sortBy + sortDir + limit', () => {
    addTrack({ title: 'A', year: 2001 })
    addTrack({ title: 'B', year: 2003 })
    addTrack({ title: 'C', year: 2002 })
    const got = evalRules({ combinator: 'and', rules: [], sortBy: 'year', sortDir: 'desc', limit: 2 })
    expect(got.map((t) => t.title)).toEqual(['B', 'C'])
  })

  it('random sort still returns every row', () => {
    addTrack({})
    addTrack({})
    addTrack({})
    const got = evalRules({ combinator: 'and', rules: [], sortBy: 'random' })
    expect(got).toHaveLength(3)
  })
})

describe('evaluateSmartPlaylist / smartPlaylistSummary', () => {
  it('evaluates rules stored on the playlist row', () => {
    addTrack({ title: 'Hit', rating: 5 })
    addTrack({ title: 'Filler', rating: 2 })
    db.prepare(`INSERT INTO playlists (name, is_smart, rules) VALUES ('Top', 1, ?)`).run(
      JSON.stringify({ combinator: 'and', rules: [{ field: 'rating', op: 'gte', value: 4 }] })
    )
    const got = evaluateSmartPlaylist(db as unknown as SmartDb, 1)
    expect(got.map((t) => t.title)).toEqual(['Hit'])
  })

  it('returns [] for corrupt or missing rules', () => {
    db.prepare(`INSERT INTO playlists (name, is_smart, rules) VALUES ('Broken', 1, 'not json')`).run()
    expect(evaluateSmartPlaylist(db as unknown as SmartDb, 1)).toEqual([])
    expect(evaluateSmartPlaylist(db as unknown as SmartDb, 999)).toEqual([])
  })

  it('summary counts, sums duration and collects up to 4 covers', () => {
    for (let i = 0; i < 6; i++) {
      addTrack({ rating: 5, duration: 60, cover_art_hash: `h${i}` })
    }
    addTrack({ rating: 1, duration: 999 })
    const summary = smartPlaylistSummary(
      db as unknown as SmartDb,
      JSON.stringify({ combinator: 'and', rules: [{ field: 'rating', op: 'gte', value: 4 }] })
    )
    expect(summary.track_count).toBe(6)
    expect(summary.total_duration).toBe(360)
    expect(summary.cover_hashes).toHaveLength(4)
  })

  it('summary respects the limit', () => {
    for (let i = 0; i < 5; i++) addTrack({ duration: 10 })
    const summary = smartPlaylistSummary(
      db as unknown as SmartDb,
      JSON.stringify({ combinator: 'and', rules: [], limit: 2 })
    )
    expect(summary.track_count).toBe(2)
    expect(summary.total_duration).toBe(20)
  })
})
