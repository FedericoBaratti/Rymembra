import { getDb } from './db'
import { writeTags } from './tagIO'
import { rebuildAggregates } from './library'
import { logWarn } from './logger'
import { albumGroupKey, canonicalAlbumArtist, pickCanonical } from './albumKey'

// Re-exported so existing tests (and any other importer) keep importing it from here.
export { pickCanonical }

// One-shot repair for albums whose tracks carry an inconsistent `album_artist` (the
// historical Spotify-migration bug: only `album` was tagged, so upsertTrackFromFile
// fell back to the per-track artist and the album split per guest credit).
//
// The album IDENTITY is already healed in the DB by the normalized `album_key`
// (folder + normalized title, see albumKey.ts), so the split no longer shows in the
// UI regardless of this repair. This pass exists to make the fix PORTABLE: it writes
// a single canonical `album_artist` back into the files so other players (and a
// future re-tag) group them as one release too.
//
// SAFETY GATE: tracks are only merged when they share the same `album_key`, i.e. the
// same normalized album title AND the same containing folder. Two genuinely different
// releases live in different folders, so they are never merged.
//
// IDEMPOTENT: once a group's files all carry the canonical album_artist it is skipped
// on subsequent runs — safe to call on every boot, no flag needed. Best-effort: a
// file that cannot be re-tagged (e.g. read-only / Android scoped storage) is skipped
// (logged); the DB grouping via album_key still holds.

interface TrackRow {
  id: number
  path: string
  album: string
  artist: string | null
  album_artist: string | null
}

/** The effective grouping artist SQLite uses for the artists table: COALESCE(album_artist, artist). */
function effectiveArtist(r: TrackRow): string {
  return (r.album_artist ?? r.artist ?? '').trim()
}

/**
 * Repair split albums in place. Returns how many album groups were merged and how
 * many files were re-tagged. Always rebuilds the aggregates at the end so existing
 * libraries pick up the album_key grouping even when nothing needed re-tagging.
 */
export async function repairSplitAlbums(): Promise<{ groups: number; retagged: number }> {
  const db = getDb()
  const rows = db
    .prepare(
      `SELECT id, path, album, artist, album_artist
       FROM tracks
       WHERE album IS NOT NULL AND album != ''`
    )
    .all() as TrackRow[]

  // Group by the normalized album_key (folder + normalized title): the folder is the
  // safety gate, so different releases that happen to share a title stay apart.
  const groups = new Map<string, TrackRow[]>()
  for (const r of rows) {
    const key = albumGroupKey(r.album, r.path)
    const arr = groups.get(key)
    if (arr) arr.push(r)
    else groups.set(key, [r])
  }

  const update = db.prepare('UPDATE tracks SET album_artist = ? WHERE id = ?')
  let repairedGroups = 0
  let retagged = 0

  for (const members of groups.values()) {
    if (members.length < 2) continue
    const distinct = new Set(members.map(effectiveArtist).filter(Boolean))
    if (distinct.size < 2) continue // already consistent — nothing to merge

    const canonical = canonicalAlbumArtist(members)
    if (!canonical) continue

    let touched = false
    for (const r of members) {
      if ((r.album_artist ?? '').trim() === canonical) continue // already aligned
      try {
        // Re-tag the FILE first (a rescan re-reads the tag): only mirror to the DB
        // once the file write succeeds.
        await writeTags(r.path, { album_artist: canonical }, null)
        update.run(canonical, r.id)
        retagged++
        touched = true
      } catch (err) {
        logWarn('repair', `Re-tag album_artist fallito: ${r.path}`, err)
      }
    }
    if (touched) repairedGroups++
  }

  // Always rebuild: the album_key grouping must be reflected in the albums table even
  // when no file was re-tagged (e.g. first boot after the v10 migration on desktop).
  rebuildAggregates()
  return { groups: repairedGroups, retagged }
}
