/**
 * Pure, I/O-free merge of the local library snapshot against the remote sync
 * file. Deterministic given its inputs so it can be unit-tested in isolation.
 *
 * Track rule (per trackKey): the local side owns liveness — if a local record
 * exists the file is present on this device, so the track is alive and its
 * metadata wins; only the play stats merge (playCount/lastPlayed = max, and
 * rating/liked resolved by the newer statsUpdatedAt). A tombstone never deletes
 * a track that still has a local file — the record is reborn. A record with no
 * local row and only a remote entry passes through untouched; a record that is
 * dead everywhere keeps just its tombstone.
 *
 * Playlist rule (per playlistKey): whole-record last-writer-wins by updatedAt;
 * a tombstone wins only when its deletedAt is newer than both sides. Playlists
 * are not tied to local files, so a deletion propagates (writeback deletes the
 * local row).
 */
import { SYNC_FILE_VERSION } from './schema'
import type { SyncFile, SyncTrack, SyncPlaylist } from './schema'

export interface Writeback {
  /** trackKey → resolved track whose stats must be persisted to the local row. */
  tracks: Record<string, SyncTrack>
  /** playlistKey → playlist to upsert + rebuild membership from (remote won). */
  playlists: Record<string, SyncPlaylist>
  /** playlistKeys whose local row must be deleted (remote tombstone won). */
  deletedPlaylists: string[]
}

export interface MergeResult {
  merged: SyncFile
  writeback: Writeback
  /** True when the writeback would change the local DB. */
  changed: boolean
}

function maxNullable(a: number | null, b: number | null): number | null {
  if (a == null) return b
  if (b == null) return a
  return a > b ? a : b
}

/** Merge remote stats into a live local track. Returns the resolved track and
 *  whether anything the local DB stores changed (so it needs writing back). */
function resolveLiveTrack(local: SyncTrack, remote: SyncTrack | undefined): {
  out: SyncTrack
  remoteWon: boolean
} {
  const out: SyncTrack = { ...local }
  if (!remote) return { out, remoteWon: false }
  let remoteWon = false

  if (remote.playCount > out.playCount) {
    out.playCount = remote.playCount
    remoteWon = true
  }
  const lastPlayed = maxNullable(out.lastPlayed, remote.lastPlayed)
  if (lastPlayed !== out.lastPlayed) {
    out.lastPlayed = lastPlayed
    remoteWon = true
  }
  // rating/liked/likedAt move together under the single statsUpdatedAt LWW clock.
  if (remote.statsUpdatedAt > local.statsUpdatedAt) {
    out.rating = remote.rating
    out.liked = remote.liked
    out.likedAt = remote.likedAt
    out.statsUpdatedAt = remote.statsUpdatedAt
    remoteWon = true // adopt remote's newer stats clock so the two sides converge
  }
  return { out, remoteWon }
}

function unionKeys(...records: Array<Record<string, unknown> | undefined>): string[] {
  const set = new Set<string>()
  for (const rec of records) if (rec) for (const k of Object.keys(rec)) set.add(k)
  return [...set]
}

export function mergeSync(local: SyncFile, remote: SyncFile | null): MergeResult {
  const writeback: Writeback = { tracks: {}, playlists: {}, deletedPlaylists: [] }
  const merged: SyncFile = {
    version: SYNC_FILE_VERSION,
    generatedAt: local.generatedAt,
    generatedBy: local.generatedBy,
    tracks: {},
    playlists: {},
    tombstones: { tracks: {}, playlists: {} }
  }

  // ---- tracks ----------------------------------------------------------------
  const trackKeys = unionKeys(
    local.tracks,
    remote?.tracks,
    local.tombstones.tracks,
    remote?.tombstones.tracks
  )
  for (const key of trackKeys) {
    const L = local.tracks[key]
    const R = remote?.tracks[key]
    const tomb = maxNullable(
      local.tombstones.tracks[key] ?? null,
      remote?.tombstones.tracks[key] ?? null
    )
    if (L) {
      // File exists here → alive; local metadata wins, stats merge, tombstone cleared.
      const { out, remoteWon } = resolveLiveTrack(L, R)
      merged.tracks[key] = out
      if (remoteWon) writeback.tracks[key] = out
    } else if (R && (tomb == null || tomb < R.statsUpdatedAt)) {
      // Available only on the other device → pass through, no local writeback.
      merged.tracks[key] = R
    } else if (tomb != null) {
      // Dead everywhere → keep the tombstone only.
      merged.tombstones.tracks[key] = tomb
    }
  }

  // ---- playlists -------------------------------------------------------------
  const playlistKeys = unionKeys(
    local.playlists,
    remote?.playlists,
    local.tombstones.playlists,
    remote?.tombstones.playlists
  )
  for (const key of playlistKeys) {
    const L = local.playlists[key]
    const R = remote?.playlists[key]
    const tomb = maxNullable(
      local.tombstones.playlists[key] ?? null,
      remote?.tombstones.playlists[key] ?? null
    )
    const lUpdated = L?.updatedAt ?? -1
    const rUpdated = R?.updatedAt ?? -1

    if (tomb != null && tomb > lUpdated && tomb > rUpdated) {
      // Deletion is the freshest event → drop the record, keep the tombstone.
      merged.tombstones.playlists[key] = tomb
      if (L) writeback.deletedPlaylists.push(key) // propagate deletion locally
      continue
    }
    if (!L && !R) continue // only a stale tombstone, already superseded
    if (rUpdated > lUpdated) {
      // Remote record wins → upsert locally.
      merged.playlists[key] = R as SyncPlaylist
      writeback.playlists[key] = R as SyncPlaylist
    } else {
      // Local wins (or ties) → keep local; it round-trips unchanged.
      merged.playlists[key] = (L ?? R) as SyncPlaylist
    }
  }

  // ---- playback (write-only in v1): keep the freshest ------------------------
  const playback = pickFreshestPlayback(local.playback, remote?.playback)
  if (playback) merged.playback = playback

  const changed =
    Object.keys(writeback.tracks).length > 0 ||
    Object.keys(writeback.playlists).length > 0 ||
    writeback.deletedPlaylists.length > 0
  return { merged, writeback, changed }
}

function pickFreshestPlayback(
  a: SyncFile['playback'],
  b: SyncFile['playback']
): SyncFile['playback'] {
  if (!a) return b
  if (!b) return a
  return b.updatedAt > a.updatedAt ? b : a
}
