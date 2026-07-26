// Smart-playlist rule engine. Pure SQL logic against a structural DB
// interface (better-sqlite3 or node:sqlite in tests) — no electron imports.

import type { SmartField, SmartOp, SmartPlaylistRules, SmartRule, Track } from '@shared/types'

export interface SmartDb {
  prepare(sql: string): {
    get(...params: unknown[]): unknown
    all(...params: unknown[]): unknown[]
  }
}

const COLUMNS: Record<SmartField, string> = {
  title: 'title',
  artist: 'artist',
  album: 'album',
  genre: 'genre',
  year: 'year',
  rating: 'rating',
  play_count: 'play_count',
  last_played: 'last_played',
  date_added: 'date_added'
}

const NUMERIC_FIELDS = new Set<SmartField>([
  'year', 'rating', 'play_count', 'last_played', 'date_added'
])

const OPS: Record<SmartOp, (col: string) => string> = {
  eq: (c) => `${c} = ?`,
  neq: (c) => `(${c} IS NULL OR ${c} != ?)`,
  contains: (c) => `${c} LIKE ?`,
  not_contains: (c) => `(${c} IS NULL OR ${c} NOT LIKE ?)`,
  gt: (c) => `${c} > ?`,
  gte: (c) => `${c} >= ?`,
  lt: (c) => `${c} < ?`,
  lte: (c) => `${c} <= ?`,
  in_last_days: (c) => `${c} >= ?`
}

function ruleParam(rule: SmartRule): unknown {
  if (rule.op === 'contains' || rule.op === 'not_contains') return `%${String(rule.value)}%`
  if (rule.op === 'in_last_days') {
    const days = Math.max(0, Number(rule.value) || 0)
    return Date.now() - days * 86_400_000
  }
  if (NUMERIC_FIELDS.has(rule.field)) return Number(rule.value) || 0
  return String(rule.value)
}

/** Validates the shape of rules coming over IPC; throws on unknown fields/ops. */
export function validateRules(rules: SmartPlaylistRules): SmartPlaylistRules {
  if (!rules || !Array.isArray(rules.rules)) throw new Error('SMART_RULES_INVALID')
  const combinator = rules.combinator === 'or' ? 'or' : 'and'
  const clean: SmartRule[] = rules.rules.map((r) => {
    if (!(r.field in COLUMNS)) throw new Error(`SMART_FIELD_INVALID:${r.field}`)
    if (!(r.op in OPS)) throw new Error(`SMART_OP_INVALID:${r.op}`)
    return { field: r.field, op: r.op, value: r.value }
  })
  const sortBy =
    rules.sortBy && (rules.sortBy === 'random' || rules.sortBy in COLUMNS)
      ? rules.sortBy
      : undefined
  const limit =
    rules.limit != null ? Math.max(1, Math.min(10_000, Math.round(Number(rules.limit) || 1))) : undefined
  return {
    combinator,
    rules: clean,
    limit,
    sortBy,
    sortDir: rules.sortDir === 'desc' ? 'desc' : rules.sortDir === 'asc' ? 'asc' : undefined
  }
}

export function rulesToSql(rules: SmartPlaylistRules): {
  where: string
  params: unknown[]
  orderBy: string
  limitSql: string
} {
  const validated = validateRules(rules)
  const clauses = validated.rules.map((r) => OPS[r.op](COLUMNS[r.field]))
  const params = validated.rules.map(ruleParam)
  const where = clauses.length
    ? `WHERE ${clauses.join(validated.combinator === 'or' ? ' OR ' : ' AND ')}`
    : ''
  let orderBy = 'ORDER BY artist COLLATE NOCASE, album, disc_number, track_number'
  if (validated.sortBy === 'random') {
    orderBy = 'ORDER BY RANDOM()'
  } else if (validated.sortBy) {
    const dir = validated.sortDir === 'desc' ? 'DESC' : 'ASC'
    orderBy = `ORDER BY ${COLUMNS[validated.sortBy]} ${dir}`
  }
  const limitSql = validated.limit != null ? `LIMIT ${validated.limit}` : ''
  return { where, params, orderBy, limitSql }
}

function parseStoredRules(json: string | null): SmartPlaylistRules | null {
  if (!json) return null
  try {
    return validateRules(JSON.parse(json) as SmartPlaylistRules)
  } catch {
    return null
  }
}

export function evaluateSmartPlaylist(db: SmartDb, playlistId: number): Track[] {
  const row = db.prepare('SELECT rules FROM playlists WHERE id = ? AND is_smart = 1').get(playlistId) as
    | { rules: string | null }
    | undefined
  const rules = parseStoredRules(row?.rules ?? null)
  if (!rules) return []
  const { where, params, orderBy, limitSql } = rulesToSql(rules)
  return db.prepare(`SELECT * FROM tracks ${where} ${orderBy} ${limitSql}`).all(...params) as Track[]
}

export function smartPlaylistSummary(db: SmartDb, rulesJson: string | null): {
  track_count: number
  total_duration: number
  cover_hashes: string[]
} {
  const empty = { track_count: 0, total_duration: 0, cover_hashes: [] as string[] }
  const rules = parseStoredRules(rulesJson)
  if (!rules) return empty
  const { where, params, orderBy, limitSql } = rulesToSql(rules)
  const stats = db
    .prepare(
      `SELECT COUNT(*) AS n, COALESCE(SUM(duration), 0) AS d FROM (
         SELECT duration FROM tracks ${where} ${limitSql ? `${orderBy} ${limitSql}` : ''}
       )`
    )
    .get(...params) as { n: number; d: number }
  const covers = db
    .prepare(
      `SELECT DISTINCT cover_art_hash FROM (
         SELECT cover_art_hash, duration FROM tracks ${where} ${orderBy} ${limitSql}
       ) WHERE cover_art_hash IS NOT NULL LIMIT 4`
    )
    .all(...params) as { cover_art_hash: string }[]
  return {
    track_count: stats.n,
    total_duration: stats.d,
    cover_hashes: covers.map((c) => c.cover_art_hash)
  }
}
