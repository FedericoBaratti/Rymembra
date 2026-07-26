import { shell } from 'electron'
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
import { mergeTracksInDb, type MergeOutcome } from './mergeTracks'
import { rebuildAggregates } from './library'
import { albumGroupKey } from './albumKey'
import { trackKey } from '@shared/trackKey'
import { recordTombstones } from './sync/tombstones'

// Enrichment lives in ./enrichment — re-exported so the IPC layer keeps
// importing everything from this module.
export { enrichTrack } from './enrichment/pipeline'
export { autoEnrichMissing } from './enrichment/autoEnrich'
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

  writeTags(track.path, update, coverBuffer)
  // Manual edits must round-trip: surface a hard error if the file did not
  // accept the tags (locked file, read-only, unsupported field for the codec).
  const mismatches = verifyTags(track.path, update)
  if (mismatches.length > 0) {
    throw new Error(`TAG_VERIFY_FAILED:${mismatches.join(', ')}`)
  }

  let coverHash = track.cover_art_hash
  if (coverBuffer) coverHash = (await storeCover(coverBuffer, { source: 'tag' })) ?? coverHash

  getDb()
    .prepare(
      `UPDATE tracks SET
        title = COALESCE(@title, title),
        artist = COALESCE(@artist, artist),
        album = COALESCE(@album, album),
        album_key = @album_key,
        album_artist = @album_artist,
        year = @year, track_number = @track_number, disc_number = @disc_number,
        bpm = @bpm, genre = @genre, comment = @comment, lyrics = @lyrics,
        cover_art_hash = @cover_art_hash,
        date_modified = @date_modified
       WHERE id = @id`
    )
    .run({
      id: trackId,
      title: update.title || null,
      artist: update.artist || null,
      album: update.album || null,
      // Recompute the grouping key so an album rename moves the track to the
      // right album card; without this the row kept its old album_key until a
      // full rescan (the albums table is aggregated by album_key, not text).
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

  // Rebuild the materialized albums/artists tables so the edit is reflected in
  // the library grouping immediately (mirrors the enrichTrack IPC handler).
  rebuildAggregates()
  broadcast('library:changed', { reason: 'edit' })
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

  const albumEdited = keys.includes('album')
  const sets = keys.map((k) => `${BATCH_COLUMNS[k]} = @${k}`)
  // An album rename must move the affected tracks to the right album card, which
  // is keyed by album_key (folder+title), not the album text — recompute it.
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
      writeTags(track.path, tagUpdate, coverBuffer)
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

  // Grouping fields changed → rebuild the materialized albums/artists tables so
  // the edit shows up in the library grouping without waiting for a rescan.
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

export async function deleteTracks(trackIds: number[], deleteFiles: boolean): Promise<void> {
  const db = getDb()
  // Explicit deletion → tombstone (computed from tags BEFORE the row is gone) so
  // the delete propagates instead of the track reappearing from another device's
  // copy. A file that merely vanished from disk goes through the watcher, which
  // deliberately does NOT tombstone.
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

  writeTags(track.path, { lyrics }, null)
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
