// Dashboard maintenance actions: retry failed enrichment and bulk cover
// backfill. Electron-bound wrappers around the pure queries in stats.ts.

import PQueue from 'p-queue'
import type { EnrichmentBucket, EnrichmentStats, Track } from '@shared/types'
import { getDb } from '../db'
import { broadcast } from '../events'
import { logWarn } from '../logger'
import { storeCover } from '../coverArt'
import { getApiCache } from '../net/apiCacheSingleton'
import { autoEnrichMissing, resetAttempted, SKIP_SERVICE } from './autoEnrich'
import { mbGetRecording } from './services/musicbrainz'
import { fetchCoverArt, resolveCover } from './services/coverart'
import { resolveMetadata } from './resolve'
import { COVER_REPLACE_CONFIDENCE } from './pipeline'
import { computeEnrichmentStats, listEnrichmentTracks } from './stats'
import { thermalManager } from '../adaptiveConcurrency'

export function getEnrichmentStats(): EnrichmentStats {
  return computeEnrichmentStats(getDb())
}

export function getEnrichmentTracks(
  bucket: EnrichmentBucket,
  offset = 0,
  limit = 50
): Track[] {
  return listEnrichmentTracks(getDb(), bucket, offset, limit)
}

/**
 * Clears the negative caches and the per-session attempted set for tracks
 * that ended in no-match/error, then kicks a new auto-enrich pass.
 */
export function retryFailedEnrichment(): { reset: number } {
  const db = getDb()
  const rows = db
    .prepare(`SELECT id FROM tracks WHERE enrich_status IN ('no-match', 'error')`)
    .all() as { id: number }[]

  const cache = getApiCache()
  for (const { id } of rows) cache.invalidate(SKIP_SERVICE, String(id))
  db.prepare(`UPDATE tracks SET enrich_status = NULL WHERE enrich_status IN ('no-match', 'error')`).run()
  resetAttempted()

  void autoEnrichMissing()
  return { reset: rows.length }
}

let coversRunning = false

/**
 * Fetches Cover Art Archive images for already-matched tracks that still
 * lack a cover. Throughput is bounded by the global MB rate limiter.
 */
export async function backfillCovers(): Promise<{ updated: number; total: number }> {
  if (coversRunning) return { updated: 0, total: 0 }
  coversRunning = true
  try {
    const db = getDb()
    const rows = db
      .prepare(
        `SELECT id, mb_recording_id FROM tracks
         WHERE mb_recording_id IS NOT NULL AND cover_art_hash IS NULL`
      )
      .all() as { id: number; mb_recording_id: string }[]

    const total = rows.length
    let done = 0
    let updated = 0
    // Short-lived pass: thermal level sampled once at start, no live retune.
    const queue = new PQueue({ concurrency: thermalManager.getConcurrency(3) })

    for (const row of rows) {
      void queue.add(async () => {
        try {
          const rec = await mbGetRecording(row.mb_recording_id)
          const release = rec?.releases?.[0]
          if (release) {
            const buf = await fetchCoverArt(release.id)
            const hash = buf ? await storeCover(buf, { source: 'caa' }) : null
            if (hash) {
              db.prepare('UPDATE tracks SET cover_art_hash = ? WHERE id = ?').run(hash, row.id)
              const track = db.prepare('SELECT * FROM tracks WHERE id = ?').get(row.id) as Track
              broadcast('track:updated', track)
              updated++
            }
          }
        } catch (err) {
          logWarn('enrich', `Backfill cover fallito per la traccia ${row.id}`, err)
        } finally {
          done++
          broadcast('enrichment:progress', { phase: 'covers', done, total })
        }
      })
    }
    await queue.onIdle()

    if (updated > 0) broadcast('library:changed', { reason: 'covers' })
    return { updated, total }
  } finally {
    coversRunning = false
  }
}

let recheckRunning = false

/**
 * Re-resolves cover art for tracks that already have one and replaces it when a
 * high-confidence, cross-provider match yields a *different* valid image. Fixes
 * covers that were previously fetched from the wrong release. Idempotent.
 */
export async function recheckCovers(): Promise<{ updated: number; total: number }> {
  if (recheckRunning) return { updated: 0, total: 0 }
  recheckRunning = true
  try {
    const db = getDb()
    const rows = db
      .prepare(
        `SELECT id, title, artist, album, duration, cover_art_hash
         FROM tracks WHERE cover_art_hash IS NOT NULL`
      )
      .all() as {
      id: number
      title: string
      artist: string
      album: string | null
      duration: number
      cover_art_hash: string
    }[]

    const total = rows.length
    let done = 0
    let updated = 0
    // Short-lived pass: thermal level sampled once at start, no live retune.
    const queue = new PQueue({ concurrency: thermalManager.getConcurrency(3) })

    for (const row of rows) {
      void queue.add(async () => {
        try {
          const resolved = await resolveMetadata({
            title: row.title,
            artist: row.artist,
            duration: row.duration,
            album: row.album
          })
          if (resolved && resolved.verdict === 'apply' && resolved.confidence >= COVER_REPLACE_CONFIDENCE) {
            const cover = await resolveCover({
              coverUrl: resolved.best.coverUrl,
              mbReleaseGroupId: resolved.best.mbReleaseGroupId,
              mbReleaseIds: resolved.best.mbReleaseIds
            })
            const hash = cover ? await storeCover(cover.buffer, { source: cover.origin }) : null
            if (hash && hash !== row.cover_art_hash) {
              db.prepare('UPDATE tracks SET cover_art_hash = ? WHERE id = ?').run(hash, row.id)
              const track = db.prepare('SELECT * FROM tracks WHERE id = ?').get(row.id) as Track
              broadcast('track:updated', track)
              updated++
            }
          }
        } catch (err) {
          logWarn('enrich', `Ricontrollo cover fallito per la traccia ${row.id}`, err)
        } finally {
          done++
          broadcast('enrichment:progress', { phase: 'covers', done, total })
        }
      })
    }
    await queue.onIdle()

    if (updated > 0) broadcast('library:changed', { reason: 'covers' })
    return { updated, total }
  } finally {
    recheckRunning = false
  }
}
