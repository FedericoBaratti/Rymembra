// Enrichment dashboard queries. Pure SQL against a structural DB interface
// (better-sqlite3 or node:sqlite in tests) — no electron imports.

import type { EnrichmentBucket, EnrichmentStats, Track } from '@shared/types'

/**
 * Auto-enrichment candidate WHERE clause, shared with autoEnrich.ts.
 * Lives here (electron-free module) so dashboard queries stay unit-testable.
 */
export const CANDIDATE_WHERE = `((artist = 'Artista sconosciuto' OR album = 'Album sconosciuto' OR cover_art_hash IS NULL)
           OR (source = 'youtube' AND enrich_status IS NULL))
         AND mb_recording_id IS NULL`

export interface StatsDb {
  prepare(sql: string): {
    get(...params: unknown[]): unknown
    all(...params: unknown[]): unknown[]
  }
}

export function computeEnrichmentStats(db: StatsDb): EnrichmentStats {
  const row = db
    .prepare(
      `SELECT
         COUNT(*) AS total,
         SUM(CASE WHEN enrich_status = 'ok' THEN 1 ELSE 0 END) AS ok,
         SUM(CASE WHEN enrich_status = 'no-match' THEN 1 ELSE 0 END) AS noMatch,
         SUM(CASE WHEN enrich_status = 'needs-review' THEN 1 ELSE 0 END) AS needsReview,
         SUM(CASE WHEN enrich_status = 'error' THEN 1 ELSE 0 END) AS error,
         SUM(CASE WHEN enrich_status IS NULL AND ${CANDIDATE_WHERE.replaceAll('\n', ' ')} THEN 1 ELSE 0 END) AS pending,
         SUM(CASE WHEN cover_art_hash IS NULL THEN 1 ELSE 0 END) AS missingCovers
       FROM tracks`
    )
    .get() as Record<keyof EnrichmentStats, number | null>

  return {
    total: row.total ?? 0,
    ok: row.ok ?? 0,
    noMatch: row.noMatch ?? 0,
    needsReview: row.needsReview ?? 0,
    error: row.error ?? 0,
    pending: row.pending ?? 0,
    missingCovers: row.missingCovers ?? 0
  }
}

const BUCKET_WHERE: Record<EnrichmentBucket, string> = {
  'no-match': "enrich_status = 'no-match'",
  'needs-review': "enrich_status = 'needs-review'",
  error: "enrich_status = 'error'",
  pending: `enrich_status IS NULL AND ${CANDIDATE_WHERE}`
}

export function listEnrichmentTracks(
  db: StatsDb,
  bucket: EnrichmentBucket,
  offset = 0,
  limit = 50
): Track[] {
  return db
    .prepare(
      `SELECT * FROM tracks WHERE ${BUCKET_WHERE[bucket]}
       ORDER BY artist, album, title LIMIT ? OFFSET ?`
    )
    .all(limit, offset) as Track[]
}
