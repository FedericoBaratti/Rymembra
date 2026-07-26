// Duplicate-merge core. Pure SQL logic against a structural DB interface
// (better-sqlite3 or node:sqlite in tests) — no electron imports. The
// caller is responsible for wrapping mergeTracksInDb in a transaction.

import type { MergeOutcome } from '@shared/types'

export type { MergeOutcome }

export interface MergeDb {
  prepare(sql: string): {
    get(...params: unknown[]): unknown
    all(...params: unknown[]): unknown[]
    run(...params: unknown[]): unknown
  }
}

interface QualityFields {
  bitrate: number | null
  sample_rate: number | null
  codec: string | null
  file_size: number
}

const LOSSLESS = ['flac', 'alac', 'wav', 'aiff', 'pcm']

function losslessRank(t: QualityFields): number {
  const codec = (t.codec ?? '').toLowerCase()
  return LOSSLESS.some((c) => codec.includes(c)) ? 1 : 0
}

/**
 * Picks the survivor by quality. 'higher' keeps the best copy (lossless first,
 * then bitrate, sample rate, size); 'lower' inverts every criterion to keep the
 * worst. Ties fall through deterministically to the input order.
 */
export function pickByQuality<T extends QualityFields>(
  tracks: T[],
  keep: 'higher' | 'lower' = 'higher'
): T {
  const dir = keep === 'lower' ? -1 : 1
  return [...tracks].sort(
    (a, b) =>
      dir *
      (losslessRank(b) - losslessRank(a) ||
        (b.bitrate ?? 0) - (a.bitrate ?? 0) ||
        (b.sample_rate ?? 0) - (a.sample_rate ?? 0) ||
        b.file_size - a.file_size)
  )[0]
}

/** Suggests the survivor: lossless first, then bitrate, sample rate, size. */
export function pickBestQuality<T extends QualityFields>(tracks: T[]): T {
  return pickByQuality(tracks, 'higher')
}

// Survivor-first fill for metadata the victims may carry and the survivor lacks.
const FILL_COLUMNS = [
  'lyrics',
  'cover_art_hash',
  'mb_recording_id',
  'acoustid_fingerprint',
  'genre',
  'year'
] as const

/**
 * Merges victim tracks into the survivor inside one unit of work:
 * playlist memberships are reparented (keeping the earliest position when
 * survivor and victim share a playlist) and renumbered gaplessly, stats are
 * aggregated (sum play_count, max rating/last_played, min date_added),
 * missing survivor metadata is filled from the victims, then victim rows
 * are deleted (FTS triggers and FK cascades clean up the rest).
 */
export function mergeTracksInDb(
  db: MergeDb,
  survivorId: number,
  victimIds: number[]
): MergeOutcome {
  const victims = [...new Set(victimIds)].filter((id) => id !== survivorId)
  if (victims.length === 0) return { merged: 0, playlistsUpdated: 0 }

  const ids = [survivorId, ...victims]
  const idPh = ids.map(() => '?').join(', ')
  const victimPh = victims.map(() => '?').join(', ')

  // stats over the whole group
  const agg = db
    .prepare(
      `SELECT SUM(play_count) AS play_count, MAX(rating) AS rating,
              MAX(last_played) AS last_played, MIN(date_added) AS date_added
       FROM tracks WHERE id IN (${idPh})`
    )
    .get(...ids) as {
    play_count: number
    rating: number
    last_played: number | null
    date_added: number
  }

  // survivor-first metadata fill
  const rows = db
    .prepare(`SELECT id, ${FILL_COLUMNS.join(', ')} FROM tracks WHERE id IN (${idPh})`)
    .all(...ids) as ({ id: number } & Record<(typeof FILL_COLUMNS)[number], unknown>)[]
  const byId = new Map(rows.map((r) => [r.id, r]))
  const ordered = ids.map((id) => byId.get(id)).filter((r) => r != null)
  const fill: Record<string, unknown> = {}
  for (const col of FILL_COLUMNS) {
    fill[col] = ordered.map((r) => r[col]).find((v) => v != null && v !== '') ?? null
  }

  db.prepare(
    `UPDATE tracks SET
       play_count = ?, rating = ?, last_played = ?, date_added = ?,
       ${FILL_COLUMNS.map((c) => `${c} = ?`).join(', ')}
     WHERE id = ?`
  ).run(
    agg.play_count,
    agg.rating,
    agg.last_played,
    agg.date_added,
    ...FILL_COLUMNS.map((c) => fill[c]),
    survivorId
  )

  // playlist reparenting + gapless renumbering per affected playlist
  const affected = db
    .prepare(
      `SELECT DISTINCT playlist_id AS id FROM playlist_tracks WHERE track_id IN (${victimPh})`
    )
    .all(...victims) as { id: number }[]
  const victimSet = new Set(victims)

  for (const { id: playlistId } of affected) {
    const entries = db
      .prepare(
        'SELECT track_id FROM playlist_tracks WHERE playlist_id = ? ORDER BY position'
      )
      .all(playlistId) as { track_id: number }[]

    let survivorPlaced = false
    const next: number[] = []
    for (const { track_id } of entries) {
      const mapped = victimSet.has(track_id) ? survivorId : track_id
      if (mapped === survivorId) {
        if (survivorPlaced) continue // dedupe: keep the earliest position
        survivorPlaced = true
      }
      next.push(mapped)
    }

    db.prepare('DELETE FROM playlist_tracks WHERE playlist_id = ?').run(playlistId)
    const ins = db.prepare(
      'INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?, ?, ?)'
    )
    next.forEach((tid, i) => ins.run(playlistId, tid, i))
  }

  db.prepare(`DELETE FROM tracks WHERE id IN (${victimPh})`).run(...victims)

  return { merged: victims.length, playlistsUpdated: affected.length }
}
