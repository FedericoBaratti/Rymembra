/**
 * Schema of the gzipped JSON file exchanged through Google Drive's appDataFolder
 * (`aether-library.json.gz`). Only library metadata is synced — never audio.
 *
 * `parseSyncFile` is deliberately lenient: unknown/extra fields are ignored
 * (forward compatibility) and individual malformed track/playlist records are
 * dropped rather than failing the whole file — otherwise one bad entry would
 * discard the other device's pass-through data. It returns `null` only when the
 * input is not a usable object at all, which the caller treats as a corrupt
 * remote (renamed aside, then recreated from local).
 */
import { z } from 'zod'
import { upgradeLegacyTrackKey } from '@shared/trackKey'

const zeroOne = z.union([z.literal(0), z.literal(1)])

export const syncTrackSchema = z.object({
  title: z.string(),
  artist: z.string(),
  album: z.string(),
  albumArtist: z.string().nullable(),
  year: z.number().nullable(),
  trackNumber: z.number().nullable(),
  discNumber: z.number().nullable(),
  duration: z.number(),
  genre: z.string().nullable(),
  playCount: z.number(),
  lastPlayed: z.number().nullable(),
  rating: z.number(),
  liked: zeroOne,
  likedAt: z.number().nullable(),
  statsUpdatedAt: z.number(),
  coverArtHash: z.string().nullable(),
  mbRecordingId: z.string().nullable(),
  addedAt: z.number()
})

export const syncPlaylistSchema = z.object({
  name: z.string(),
  description: z.string().nullable(),
  createdAt: z.number(),
  updatedAt: z.number(),
  isSmart: zeroOne,
  rules: z.string().nullable(),
  trackKeys: z.array(z.string())
})

export const syncPlaybackSchema = z.object({
  updatedAt: z.number(),
  deviceId: z.string(),
  trackKeys: z.array(z.string()),
  orderPos: z.number(),
  shuffle: z.boolean(),
  repeat: z.enum(['off', 'one', 'all'])
})

export type SyncTrack = z.infer<typeof syncTrackSchema>
export type SyncPlaylist = z.infer<typeof syncPlaylistSchema>
export type SyncPlayback = z.infer<typeof syncPlaybackSchema>

export interface SyncTombstones {
  tracks: Record<string, number> // trackKey → deletedAt
  playlists: Record<string, number> // playlistKey → deletedAt
}

export interface SyncFile {
  version: number
  generatedAt: number
  generatedBy: string // syncDeviceId of the writer
  tracks: Record<string, SyncTrack> // key = trackKey
  playlists: Record<string, SyncPlaylist> // key = playlistKey
  tombstones: SyncTombstones
  playback?: SyncPlayback // only written, never applied
}

export const SYNC_FILE_NAME = 'aether-library.json.gz'

// v2: track keys dropped the duration segment (artist|title|album). Files with
// v1 keys — including mixed-key files a v1 device produced after merging a v2
// remote — are healed on parse by `upgradeLegacyTrackKey`.
export const SYNC_FILE_VERSION = 2

/** An empty sync file for the given writer — used when the remote is absent. */
export function emptySyncFile(generatedBy: string): SyncFile {
  return {
    version: SYNC_FILE_VERSION,
    generatedAt: Date.now(),
    generatedBy,
    tracks: {},
    playlists: {},
    tombstones: { tracks: {}, playlists: {} }
  }
}

function maxNullable(a: number | null, b: number | null): number | null {
  if (a == null) return b
  if (b == null) return a
  return a > b ? a : b
}

/**
 * Fold two SyncTracks that share the same key: keep the richer play stats so
 * nothing regresses, the rating/liked state from the most recent stats update,
 * and the earliest addedAt. Used both when two local files collapse to one key
 * (buildSnapshot) and when the v1→v2 key migration merges near-duplicates.
 */
export function foldSyncTracks(a: SyncTrack, b: SyncTrack): SyncTrack {
  const base = b.playCount > a.playCount ? b : a
  const stats = b.statsUpdatedAt > a.statsUpdatedAt ? b : a
  return {
    ...base,
    playCount: Math.max(a.playCount, b.playCount),
    lastPlayed: maxNullable(a.lastPlayed, b.lastPlayed),
    rating: stats.rating,
    liked: stats.liked,
    likedAt: stats.likedAt,
    statsUpdatedAt: stats.statsUpdatedAt,
    addedAt: Math.min(a.addedAt, b.addedAt)
  }
}

function pickValidRecords<T>(input: unknown, schema: z.ZodType<T>): Record<string, T> {
  const out: Record<string, T> = {}
  if (input && typeof input === 'object') {
    for (const [key, value] of Object.entries(input as Record<string, unknown>)) {
      const parsed = schema.safeParse(value)
      if (parsed.success) out[key] = parsed.data
    }
  }
  return out
}

function pickFiniteNumbers(input: unknown): Record<string, number> {
  const out: Record<string, number> = {}
  if (input && typeof input === 'object') {
    for (const [key, value] of Object.entries(input as Record<string, unknown>)) {
      if (typeof value === 'number' && Number.isFinite(value)) out[key] = value
    }
  }
  return out
}

/** Re-key a tracks record through `upgradeLegacyTrackKey`, folding collisions. */
function upgradeTrackRecord(tracks: Record<string, SyncTrack>): Record<string, SyncTrack> {
  const out: Record<string, SyncTrack> = {}
  for (const [key, t] of Object.entries(tracks)) {
    const k = upgradeLegacyTrackKey(key)
    out[k] = out[k] ? foldSyncTracks(out[k], t) : t
  }
  return out
}

/** Re-key track tombstones, keeping the most recent deletion on collision. */
function upgradeTombstoneRecord(tombs: Record<string, number>): Record<string, number> {
  const out: Record<string, number> = {}
  for (const [key, deletedAt] of Object.entries(tombs)) {
    const k = upgradeLegacyTrackKey(key)
    out[k] = out[k] != null ? Math.max(out[k], deletedAt) : deletedAt
  }
  return out
}

/** Map a trackKeys list to v2 keys, dropping duplicates (first occurrence wins). */
function upgradeKeyList(keys: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const key of keys) {
    const k = upgradeLegacyTrackKey(key)
    if (seen.has(k)) continue
    seen.add(k)
    out.push(k)
  }
  return out
}

/**
 * Parse an already-JSON-decoded value into a SyncFile, salvaging as much as
 * possible. Returns null only when the input is not an object (corrupt remote).
 *
 * Track keys are always passed through the v1→v2 upgrade: it is idempotent on
 * v2 keys, migrates old files in place, and heals mixed-key files regardless of
 * what the `version` field claims.
 */
export function parseSyncFile(raw: unknown): SyncFile | null {
  if (!raw || typeof raw !== 'object') return null
  const obj = raw as Record<string, unknown>
  const tomb = (obj.tombstones ?? {}) as Record<string, unknown>
  const playback = syncPlaybackSchema.safeParse(obj.playback)
  const playlists = pickValidRecords(obj.playlists, syncPlaylistSchema)
  for (const p of Object.values(playlists)) p.trackKeys = upgradeKeyList(p.trackKeys)
  return {
    version: SYNC_FILE_VERSION,
    generatedAt: typeof obj.generatedAt === 'number' ? obj.generatedAt : Date.now(),
    generatedBy: typeof obj.generatedBy === 'string' ? obj.generatedBy : '',
    tracks: upgradeTrackRecord(pickValidRecords(obj.tracks, syncTrackSchema)),
    playlists,
    tombstones: {
      tracks: upgradeTombstoneRecord(pickFiniteNumbers(tomb.tracks)),
      playlists: pickFiniteNumbers(tomb.playlists)
    },
    ...(playback.success
      ? { playback: { ...playback.data, trackKeys: playback.data.trackKeys.map(upgradeLegacyTrackKey) } }
      : {})
  }
}
