import { describe, it, expect } from 'vitest'
import { parseSyncFile, foldSyncTracks, SYNC_FILE_VERSION } from './schema'
import type { SyncTrack } from './schema'

function track(over: Partial<SyncTrack> = {}): SyncTrack {
  return {
    title: 'T',
    artist: 'A',
    album: 'Al',
    albumArtist: null,
    year: null,
    trackNumber: null,
    discNumber: null,
    duration: 200,
    genre: null,
    playCount: 0,
    lastPlayed: null,
    rating: 0,
    liked: 0,
    likedAt: null,
    statsUpdatedAt: 0,
    coverArtHash: null,
    mbRecordingId: null,
    addedAt: 0,
    ...over
  }
}

function rawFile(over: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    version: 1,
    generatedAt: 111,
    generatedBy: 'dev1',
    tracks: {},
    playlists: {},
    tombstones: { tracks: {}, playlists: {} },
    ...over
  }
}

describe('parseSyncFile — v1 → v2 key migration', () => {
  it('re-keys v1 track keys by dropping the duration tail', () => {
    const parsed = parseSyncFile(rawFile({ tracks: { 'a|b|c|200': track() } }))!
    expect(parsed.tracks['a|b|c']).toBeDefined()
    expect(parsed.tracks['a|b|c|200']).toBeUndefined()
    expect(parsed.version).toBe(SYNC_FILE_VERSION)
  })

  it('folds two v1 keys that collide on the same v2 key', () => {
    const parsed = parseSyncFile(
      rawFile({
        tracks: {
          'a|b|c|200': track({ playCount: 5, rating: 3, statsUpdatedAt: 100, addedAt: 50 }),
          'a|b|c|203': track({ playCount: 2, rating: 5, statsUpdatedAt: 200, addedAt: 10 })
        }
      })
    )!
    const t = parsed.tracks['a|b|c']
    expect(Object.keys(parsed.tracks)).toEqual(['a|b|c'])
    expect(t.playCount).toBe(5) // max
    expect(t.rating).toBe(5) // newer statsUpdatedAt wins
    expect(t.statsUpdatedAt).toBe(200)
    expect(t.addedAt).toBe(10) // earliest
  })

  it('re-keys track tombstones, keeping the newest deletion on collision', () => {
    const parsed = parseSyncFile(
      rawFile({ tombstones: { tracks: { 'a|b|c|200': 100, 'a|b|c|205': 300 }, playlists: {} } })
    )!
    expect(parsed.tombstones.tracks).toEqual({ 'a|b|c': 300 })
  })

  it('re-keys playlist trackKeys and playback trackKeys, deduping', () => {
    const parsed = parseSyncFile(
      rawFile({
        playlists: {
          p: {
            name: 'P', description: null, createdAt: 0, updatedAt: 0,
            isSmart: 0, rules: null, trackKeys: ['a|b|c|200', 'a|b|c|203', 'x|y|z']
          }
        },
        playback: {
          updatedAt: 1, deviceId: 'dev1', trackKeys: ['a|b|c|200'],
          orderPos: 0, shuffle: false, repeat: 'off'
        }
      })
    )!
    expect(parsed.playlists.p.trackKeys).toEqual(['a|b|c', 'x|y|z'])
    expect(parsed.playback?.trackKeys).toEqual(['a|b|c'])
  })

  it('heals a mixed-key file regardless of the version field', () => {
    const parsed = parseSyncFile(
      rawFile({
        version: 2, // lies: still contains a v1 key
        tracks: { 'a|b|c': track({ playCount: 1 }), 'a|b|c|200': track({ playCount: 7 }) }
      })
    )!
    expect(Object.keys(parsed.tracks)).toEqual(['a|b|c'])
    expect(parsed.tracks['a|b|c'].playCount).toBe(7)
  })

  it('passes v2 keys through unchanged', () => {
    const parsed = parseSyncFile(rawFile({ version: 2, tracks: { 'a|b|c': track() } }))!
    expect(parsed.tracks['a|b|c']).toBeDefined()
  })

  it('returns null for a non-object input (corrupt remote)', () => {
    expect(parseSyncFile(null)).toBeNull()
    expect(parseSyncFile('garbage')).toBeNull()
  })

  it('salvages valid records and drops malformed ones', () => {
    const parsed = parseSyncFile(
      rawFile({ tracks: { 'a|b|c|200': track(), bad: { title: 42 } } })
    )!
    expect(Object.keys(parsed.tracks)).toEqual(['a|b|c'])
  })
})

describe('foldSyncTracks', () => {
  it('keeps richer play stats, newer rating clock, earliest addedAt', () => {
    const a = track({ playCount: 9, lastPlayed: 100, rating: 2, statsUpdatedAt: 50, addedAt: 5 })
    const b = track({ playCount: 3, lastPlayed: 700, rating: 4, statsUpdatedAt: 90, addedAt: 20 })
    const out = foldSyncTracks(a, b)
    expect(out.playCount).toBe(9)
    expect(out.lastPlayed).toBe(700)
    expect(out.rating).toBe(4)
    expect(out.statsUpdatedAt).toBe(90)
    expect(out.addedAt).toBe(5)
    expect(out.title).toBe(a.title) // base = higher playCount side
  })
})
