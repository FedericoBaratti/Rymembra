import { shell } from 'electron'
import { mkdirSync, readdirSync, statSync, renameSync, copyFileSync, unlinkSync } from 'node:fs'
import { join, basename } from 'node:path'
import type {
  Track,
  TrackMetadataUpdate,
  DuplicateGroup,
  LyricsResult
} from '@shared/types'
import { getDb } from './db'
import { storeCover } from './coverArt'
import { broadcast } from './events'
import { logWarn } from './logger'
import { writeTags, verifyTags } from './tagIO'
import { parseLrc } from './enrichment/lrc'
import { fetchLyricsRemote } from './enrichment/services/lrclib'
import { mergeTracksInDb, pickByQuality, type MergeOutcome } from './mergeTracks'
import { rebuildAggregates, isUnder, TRASH_DIR_NAME } from './library'
import { albumGroupKey } from './albumKey'
import { getSettings } from './settings'
import { trackKey, normalizeKey } from '@shared/trackKey'
import { recordTombstones } from './sync/tombstones'

// Enrichment lives in ./enrichment — re-exported so the IPC layer keeps
// importing everything from this module.
export { enrichTrack } from './enrichment/pipeline'
export { autoEnrichMissing, ensureAutoEnrichScheduler } from './enrichment/autoEnrich'
import { requeueEnrichment } from './enrichment/autoEnrich'
export {
  getEnrichmentStats,
  getEnrichmentTracks,
  retryFailedEnrichment,
  backfillCovers,
  recheckCovers
} from './enrichment/maintenance'

function getTrack(id: number): Track | null {
  return (getDb().prepare('SELECT * FROM tracks WHERE id = ?').get(id) as Track | undefined) ?? null
}

function emitTrack(id: number): Track {
  const track = getTrack(id)!
  broadcast('track:updated', track)
  return track
}

// ---------- manual metadata editing ----------

export async function updateTrackMetadata(
  trackId: number,
  update: TrackMetadataUpdate
): Promise<Track> {
  const track = getTrack(trackId)
  if (!track) throw new Error('TRACK_NOT_FOUND')

  const coverBuffer = update.coverImageBase64
    ? Buffer.from(update.coverImageBase64, 'base64')
    : null

  await writeTags(track.path, update, coverBuffer)
  // Manual edits must round-trip: surface a hard error if the file did not
  // accept the tags (locked file, read-only, unsupported field for the codec).
  const mismatches = verifyTags(track.path, update)
  if (mismatches.length > 0) {
    throw new Error(`TAG_VERIFY_FAILED:${mismatches.join(', ')}`)
  }

  let coverHash = track.cover_art_hash
  // A hand-picked cover is also written into the file tags → 'tag' provenance.
  if (coverBuffer) coverHash = (await storeCover(coverBuffer, { source: 'tag' })) ?? coverHash

  getDb()
    .prepare(
      `UPDATE tracks SET
        title = COALESCE(@title, title),
        artist = COALESCE(@artist, artist),
        album = COALESCE(@album, album),
        album_key = @album_key,
        album_artist = COALESCE(@album_artist, album_artist),
        year = COALESCE(@year, year),
        track_number = COALESCE(@track_number, track_number),
        disc_number = COALESCE(@disc_number, disc_number),
        bpm = COALESCE(@bpm, bpm),
        genre = COALESCE(@genre, genre),
        comment = COALESCE(@comment, comment),
        lyrics = COALESCE(@lyrics, lyrics),
        cover_art_hash = @cover_art_hash,
        date_modified = @date_modified
       WHERE id = @id`
    )
    .run({
      id: trackId,
      title: update.title || null,
      artist: update.artist || null,
      album: update.album || null,
      // The album identity is the derived album_key (folder + normalized title);
      // recompute it from the album that will actually be stored (the new value
      // when provided, otherwise the unchanged one), mirroring upsertTrackFromFile.
      album_key: albumGroupKey(update.album || track.album, track.path),
      album_artist: update.album_artist || null,
      year: update.year ?? null,
      track_number: update.track_number ?? null,
      disc_number: update.disc_number ?? null,
      bpm: update.bpm ?? null,
      genre: update.genre || null,
      comment: update.comment ?? null,
      lyrics: update.lyrics ?? null,
      cover_art_hash: coverHash,
      date_modified: Date.now()
    })

  // The materialized albums table keys on album_key, so an edit that changes the
  // album (or artist) must rebuild it, else the track stays under the old card.
  rebuildAggregates()
  broadcast('library:changed', { reason: 'edit' })

  // A better title/artist typed by the user is exactly what an unmatched track
  // needs to finally resolve — re-queue it for background enrichment. Skipped
  // for tracks already matched authoritatively (mb_recording_id) so a manual
  // tweak is never overwritten, and honours the auto-enrich master switch.
  if (
    getSettings().autoEnrichEnabled &&
    track.mb_recording_id == null &&
    ((update.title && update.title !== track.title) ||
      (update.artist && update.artist !== track.artist))
  ) {
    requeueEnrichment([trackId])
  }

  return emitTrack(trackId)
}

// Whitelisted batch-editable fields -> DB columns. Per-track fields (title,
// track_number, lyrics) are intentionally excluded.
const BATCH_COLUMNS: Partial<Record<keyof TrackMetadataUpdate, string>> = {
  artist: 'artist',
  album: 'album',
  album_artist: 'album_artist',
  genre: 'genre',
  year: 'year',
  disc_number: 'disc_number',
  bpm: 'bpm',
  comment: 'comment'
}

/**
 * Applies the provided (only the defined) fields to many tracks: file tags
 * via writeTags + a dynamically-built DB UPDATE. Emits progress events.
 */
export async function updateTracksMetadata(
  trackIds: number[],
  update: TrackMetadataUpdate
): Promise<{ updated: number; errors: number }> {
  const keys = (Object.keys(BATCH_COLUMNS) as (keyof TrackMetadataUpdate)[]).filter(
    (k) => update[k] !== undefined
  )
  const coverBuffer = update.coverImageBase64
    ? Buffer.from(update.coverImageBase64, 'base64')
    : null
  if (keys.length === 0 && !coverBuffer) return { updated: 0, errors: 0 }

  let coverHash: string | null = null
  if (coverBuffer) coverHash = await storeCover(coverBuffer, { source: 'tag' })

  const sets = keys.map((k) => `${BATCH_COLUMNS[k]} = @${k}`)
  // Editing the album must also refresh the derived album_key (computed per-track
  // below, since it depends on each track's path), or the rows keep their old key.
  const albumEdited = keys.includes('album')
  if (albumEdited) sets.push('album_key = @album_key')
  if (coverHash) sets.push('cover_art_hash = @cover_art_hash')
  sets.push('date_modified = @date_modified')
  const stmt = getDb().prepare(`UPDATE tracks SET ${sets.join(', ')} WHERE id = @id`)

  const tagUpdate: TrackMetadataUpdate = {}
  for (const k of keys) (tagUpdate as Record<string, unknown>)[k] = update[k]

  const total = trackIds.length
  let done = 0
  let errors = 0
  for (const id of trackIds) {
    const track = getTrack(id)
    if (!track) {
      errors++
      done++
      continue
    }
    try {
      await writeTags(track.path, tagUpdate, coverBuffer)
      const params: Record<string, unknown> = {
        id,
        date_modified: Date.now(),
        cover_art_hash: coverHash
      }
      for (const k of keys) params[k] = update[k] === '' ? null : (update[k] ?? null)
      if (albumEdited) params.album_key = albumGroupKey(String(update.album ?? ''), track.path)
      stmt.run(params)
    } catch (err) {
      errors++
      logWarn('batch-edit', `Modifica fallita: ${track.path}`, err)
    }
    done++
    broadcast('batch-metadata:progress', { done, total, errors })
  }

  // Album/artist edits change album_key grouping and the canonical album artist,
  // so the materialized albums/artists tables must be rebuilt to match.
  if (keys.some((k) => k === 'album' || k === 'artist' || k === 'album_artist' || k === 'year')) {
    rebuildAggregates()
  }
  broadcast('library:changed', { reason: 'batch-edit' })
  return { updated: total - errors, errors }
}

// ---------- duplicates ----------

export function findDuplicates(): DuplicateGroup[] {
  const db = getDb()
  const groups: DuplicateGroup[] = []

  const tagDupes = db
    .prepare(
      `SELECT LOWER(title) AS t, LOWER(artist) AS a FROM tracks
       GROUP BY LOWER(title), LOWER(artist) HAVING COUNT(*) > 1`
    )
    .all() as { t: string; a: string }[]
  for (const { t, a } of tagDupes) {
    const tracks = db
      .prepare('SELECT * FROM tracks WHERE LOWER(title) = ? AND LOWER(artist) = ? ORDER BY duration')
      .all(t, a) as Track[]
    // split by duration proximity (within 2 seconds)
    let group: Track[] = []
    for (const tr of tracks) {
      if (group.length === 0 || Math.abs(tr.duration - group[0].duration) <= 2) {
        group.push(tr)
      } else {
        if (group.length > 1) groups.push({ reason: 'tags', tracks: group })
        group = [tr]
      }
    }
    if (group.length > 1) groups.push({ reason: 'tags', tracks: group })
  }

  const fpDupes = db
    .prepare(
      `SELECT acoustid_fingerprint AS fp FROM tracks
       WHERE acoustid_fingerprint IS NOT NULL
       GROUP BY acoustid_fingerprint HAVING COUNT(*) > 1`
    )
    .all() as { fp: string }[]
  const seen = new Set(groups.flatMap((g) => g.tracks.map((tr) => tr.id)))
  for (const { fp } of fpDupes) {
    const tracks = (
      db.prepare('SELECT * FROM tracks WHERE acoustid_fingerprint = ?').all(fp) as Track[]
    ).filter((tr) => !seen.has(tr.id))
    if (tracks.length > 1) groups.push({ reason: 'fingerprint', tracks })
  }

  return groups
}

/**
 * Merges duplicate tracks into the survivor (playlists, stats, metadata
 * fill — see mergeTracksInDb), optionally trashing the victim files.
 */
export async function mergeDuplicates(
  survivorId: number,
  victimIds: number[],
  deleteFiles: boolean
): Promise<MergeOutcome> {
  const db = getDb()
  const victims = victimIds.filter((id) => id !== survivorId)
  const paths = victims
    .map((id) => getTrack(id)?.path)
    .filter((p): p is string => typeof p === 'string')

  const outcome = db.transaction(() => mergeTracksInDb(db, survivorId, victims))()

  if (deleteFiles) {
    for (const path of paths) {
      try {
        await shell.trashItem(path)
      } catch {
        // file may already be gone
      }
    }
  }

  rebuildAggregates()
  broadcast('library:changed', { reason: 'merge' })
  return outcome
}

let autoDedupeRunning = false

export interface DedupePlan {
  survivor: Track
  victims: Track[]
}

/**
 * Decide what the automatic dedupe is allowed to remove. Pure (no DB, no fs)
 * so it is unit-testable. Much stricter than the interactive duplicates view:
 *  - 'tags' groups (matched on title+artist only) are re-partitioned by
 *    normalized album — different releases of the same song are NOT duplicates;
 *    'fingerprint' groups are audio-identical and pass through unchanged;
 *  - only files inside the download folder are eligible victims. Pre-existing /
 *    imported files are never touched, and a group whose losers all live
 *    outside the download folder is skipped entirely.
 */
export function planAutoDedupe(
  groups: DuplicateGroup[],
  downloadFolder: string,
  keep: 'higher' | 'lower'
): DedupePlan[] {
  const plans: DedupePlan[] = []
  if (!downloadFolder) return plans

  const subgroups: Track[][] = []
  for (const group of groups) {
    if (group.reason === 'fingerprint') {
      subgroups.push(group.tracks)
      continue
    }
    const byAlbum = new Map<string, Track[]>()
    for (const tr of group.tracks) {
      const k = normalizeKey(tr.album)
      const list = byAlbum.get(k) ?? []
      list.push(tr)
      byAlbum.set(k, list)
    }
    for (const list of byAlbum.values()) {
      if (list.length > 1) subgroups.push(list)
    }
  }

  for (const tracks of subgroups) {
    const survivor = pickByQuality(tracks, keep)
    const victims = tracks.filter(
      (tr) =>
        tr.id !== survivor.id && typeof tr.path === 'string' && isUnder(tr.path, downloadFolder)
    )
    if (victims.length === 0) continue
    plans.push({ survivor, victims })
  }
  return plans
}

// Auto-dedupe never deletes permanently: victims move to a local trash folder
// inside the download folder, purged after 7 days. The folder is excluded from
// scans/watching so trashed files don't re-enter the library.
const TRASH_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000

function moveToLocalTrash(path: string, downloadFolder: string): void {
  const dir = join(downloadFolder, TRASH_DIR_NAME)
  mkdirSync(dir, { recursive: true })
  const dest = join(dir, `${Date.now()}-${basename(path)}`)
  try {
    renameSync(path, dest)
  } catch {
    // rename fails across mounts (EXDEV) — fall back to copy + delete
    copyFileSync(path, dest)
    unlinkSync(path)
  }
}

/** Delete trashed duplicates older than 7 days. Called after each auto-dedupe
 *  pass and once at boot (recoverStaleDownloads). Missing dir → no-op. */
export function purgeTrashDir(): void {
  const folder = getSettings().downloadFolder
  if (!folder) return
  const dir = join(folder, TRASH_DIR_NAME)
  let names: string[]
  try {
    names = readdirSync(dir)
  } catch {
    return // nothing trashed yet
  }
  const cutoff = Date.now() - TRASH_MAX_AGE_MS
  for (const name of names) {
    const full = join(dir, name)
    try {
      if (statSync(full).mtimeMs < cutoff) unlinkSync(full)
    } catch {
      // racing deletion — ignore
    }
  }
}

/**
 * Automatic, non-interactive duplicate removal. What may be removed is decided
 * by {@link planAutoDedupe} (same album or same fingerprint only, victims only
 * inside the download folder); victims are merged into the survivor (playlists,
 * stats and missing metadata consolidated by mergeTracksInDb) and their files
 * moved to the recoverable `.trash` folder instead of being deleted.
 *
 * Fire-and-forget: guarded so overlapping scan/download triggers don't run it
 * twice; returns the number of removed duplicates. A no-op when disabled.
 */
export async function autoDedupe(): Promise<number> {
  if (autoDedupeRunning) return 0
  const settings = getSettings()
  if (!settings.dedupeAutoRemove) return 0

  autoDedupeRunning = true
  try {
    const db = getDb()
    const plans = planAutoDedupe(findDuplicates(), settings.downloadFolder, settings.dedupeKeep)
    if (plans.length === 0) return 0

    // Collect victim paths BEFORE deleting the rows (getTrack would miss them
    // after the merge transaction), mirroring mergeDuplicates.
    const victimPaths: string[] = []
    let removed = 0

    for (const { survivor, victims } of plans) {
      for (const v of victims) victimPaths.push(v.path)
      db.transaction(() =>
        mergeTracksInDb(db, survivor.id, victims.map((v) => v.id))
      )()
      removed += victims.length
    }

    for (const path of victimPaths) {
      try {
        moveToLocalTrash(path, settings.downloadFolder)
      } catch (err) {
        // File may be locked or already gone; the DB row is already merged,
        // and a later rescan re-attempts the move.
        logWarn('dedupe', `Impossibile cestinare il file duplicato: ${path}`, err)
      }
    }
    purgeTrashDir()

    rebuildAggregates()
    broadcast('library:changed', { reason: 'dedupe' })
    broadcast('duplicates:removed', { count: removed })
    return removed
  } finally {
    autoDedupeRunning = false
  }
}

export async function deleteTracks(trackIds: number[], deleteFiles: boolean): Promise<void> {
  const db = getDb()
  // Explicit deletion → tombstone (computed from tags BEFORE the row is gone) so
  // the delete propagates instead of the track reappearing from another device's
  // copy. A file that merely vanished from disk goes through removeTrackByPath,
  // which deliberately does NOT tombstone.
  const deletedKeys: string[] = []
  for (const id of trackIds) {
    const track = getTrack(id)
    if (!track) continue
    deletedKeys.push(trackKey(track))
    if (deleteFiles) {
      try {
        await shell.trashItem(track.path)
      } catch {
        // file may already be gone
      }
    }
    db.prepare('DELETE FROM tracks WHERE id = ?').run(id)
  }
  if (deletedKeys.length) recordTombstones('track', deletedKeys, db)
  broadcast('library:changed', { reason: 'delete' })
}

// ---------- lyrics (lrclib.net via enrichment/services) ----------

/**
 * Writes only the lyrics tag + DB column. updateTrackMetadata is not
 * partial-safe (it overwrites every column), so the lyrics editor goes
 * through this dedicated path.
 */
export async function saveLyrics(trackId: number, lyrics: string): Promise<Track> {
  const track = getTrack(trackId)
  if (!track) throw new Error('TRACK_NOT_FOUND')

  await writeTags(track.path, { lyrics }, null)
  const mismatches = verifyTags(track.path, { lyrics })
  if (mismatches.length > 0) {
    throw new Error(`TAG_VERIFY_FAILED:${mismatches.join(', ')}`)
  }

  getDb()
    .prepare('UPDATE tracks SET lyrics = ?, date_modified = ? WHERE id = ?')
    .run(lyrics, Date.now(), trackId)
  return emitTrack(trackId)
}

export async function getLyrics(trackId: number, force = false): Promise<LyricsResult> {
  const track = getTrack(trackId)
  if (!track) return { synced: null, plain: null }

  if (track.lyrics && !force) {
    const synced = parseLrc(track.lyrics)
    if (synced) return { synced, plain: null }
    return { synced: null, plain: track.lyrics }
  }

  const remote = await fetchLyricsRemote({
    artist: track.artist,
    title: track.title,
    album: track.album,
    duration: track.duration
  })
  if (!remote) return { synced: null, plain: null }

  const stored = remote.synced || remote.plain
  if (stored) {
    getDb().prepare('UPDATE tracks SET lyrics = ? WHERE id = ?').run(stored, trackId)
  }
  if (remote.synced) {
    return { synced: parseLrc(remote.synced), plain: remote.plain ?? null }
  }
  return { synced: null, plain: remote.plain ?? null }
}
