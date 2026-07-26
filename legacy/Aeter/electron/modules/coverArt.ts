import { createHash } from 'node:crypto'
import { existsSync, writeFileSync } from 'node:fs'
import sharp from 'sharp'
import { getDb } from './db'
import { logWarn } from './logger'
import { MIN_COVER_PX, isAcceptableCoverRatio, validateCoverBuffer } from './coverValidation'
import { coverPath } from './coverPaths'

export { validateCoverBuffer } from './coverValidation'

/** Cover provenance (schema v14): drives the canonical album-cover pick. */
export type CoverSource = 'tag' | 'provider' | 'spotify' | 'caa' | 'unknown'

/** Records where a cover came from; a known origin upgrades a legacy 'unknown' row. */
function persistCoverSource(db: ReturnType<typeof getDb>, hash: string, source: CoverSource): void {
  if (source === 'unknown') return
  db.prepare(`UPDATE cover_art SET source = ? WHERE hash = ? AND source = 'unknown'`).run(
    source,
    hash
  )
}

/**
 * Stores cover art as files (512px main + 64px thumb, webp) under the covers
 * dir, keyed by content hash; the DB keeps only a lightweight index row (no
 * BLOBs — see coverPaths.ts for why). Returns the content hash, or null if the
 * image is invalid/unprocessable.
 */
export async function storeCover(
  buffer: Buffer,
  opts: { source?: CoverSource } = {}
): Promise<string | null> {
  const source = opts.source ?? 'unknown'
  if (!validateCoverBuffer(buffer)) {
    logWarn('cover', `Copertina scartata (non valida, ${buffer?.length ?? 0} byte)`)
    return null
  }
  try {
    const hash = createHash('sha1').update(buffer).digest('hex')
    const db = getDb()
    // Fast path: known hash AND the file is already on disk → nothing to do.
    const exists = db.prepare('SELECT 1 FROM cover_art WHERE hash = ?').get(hash)
    if (exists && existsSync(coverPath(hash))) {
      persistCoverSource(db, hash, source)
      return hash
    }

    const img = sharp(buffer)
    const meta = await img.metadata()
    // Dimensions come from sharp on desktop and the native probe on Android
    // (sharp-shim metadata(), fail-open {} on a probe hiccup); when present,
    // reject tiny images and non-square-ish ratios (video thumbnails, banners).
    if (meta.width && meta.height) {
      if (
        meta.width < MIN_COVER_PX ||
        meta.height < MIN_COVER_PX ||
        !isAcceptableCoverRatio(meta.width, meta.height)
      ) {
        logWarn('cover', `Copertina scartata (${meta.width}x${meta.height})`)
        return null
      }
    }
    const main = await img
      .resize(512, 512, { fit: 'cover', withoutEnlargement: true })
      .webp({ quality: 82 })
      .toBuffer()
    // thumb from the already-resized 512px buffer: skips a second decode of
    // the full-size original
    const thumb = await sharp(main)
      .resize(64, 64, { fit: 'cover' })
      .webp({ quality: 70 })
      .toBuffer()

    // Write the files first, then record the index row. Idempotent: only write
    // if absent (dedup by hash).
    if (!existsSync(coverPath(hash))) writeFileSync(coverPath(hash), main)
    if (!existsSync(coverPath(hash, true))) writeFileSync(coverPath(hash, true), thumb)

    db.prepare(
      'INSERT OR IGNORE INTO cover_art (hash, width, height, mime_type, source) VALUES (?, ?, ?, ?, ?)'
    ).run(hash, meta.width ?? 512, meta.height ?? 512, 'image/webp', source)
    // The INSERT may have been ignored (row existed with a missing file):
    // still upgrade a legacy 'unknown' provenance.
    persistCoverSource(db, hash, source)
    invalidateCoverLookup(hash)
    return hash
  } catch (err) {
    logWarn('cover', 'Copertina non processabile', err)
    return null
  }
}

/**
 * Cover lookups are hot on grid pages: every art request used to cost one SQL
 * hit + stat calls. Results are content-addressed (hash → immutable file), so
 * a small LRU makes repeats free. Negatives are NOT cached (the file may land
 * on disk moments later via enrichment); storeCover invalidates its hash so a
 * late-written thumb replaces the main-image fallback.
 */
const coverLookupCache = new Map<string, { path: string; mime: string }>()
const COVER_CACHE_MAX = 500

function invalidateCoverLookup(hash: string): void {
  coverLookupCache.delete(`${hash}|0`)
  coverLookupCache.delete(`${hash}|1`)
}

/**
 * Resolve a stored cover to a file path the caller can stream. The thumb falls
 * back to the main image when its file is missing (older covers, partial writes).
 */
export function getCover(hash: string, thumb = false): { path: string; mime: string } | null {
  const key = `${hash}|${thumb ? 1 : 0}`
  const hit = coverLookupCache.get(key)
  if (hit) {
    // refresh recency (Map iterates in insertion order)
    coverLookupCache.delete(key)
    coverLookupCache.set(key, hit)
    return hit
  }

  const row = getDb()
    .prepare('SELECT mime_type FROM cover_art WHERE hash = ?')
    .get(hash) as { mime_type: string } | undefined
  if (!row) return null
  const wanted = coverPath(hash, thumb)
  let result: { path: string; mime: string } | null = null
  if (existsSync(wanted)) {
    result = { path: wanted, mime: row.mime_type }
  } else if (thumb) {
    const main = coverPath(hash, false)
    if (existsSync(main)) result = { path: main, mime: row.mime_type }
  }
  if (result) {
    coverLookupCache.set(key, result)
    if (coverLookupCache.size > COVER_CACHE_MAX) {
      coverLookupCache.delete(coverLookupCache.keys().next().value!)
    }
  }
  return result
}
