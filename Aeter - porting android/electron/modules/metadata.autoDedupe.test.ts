import { describe, it, expect, vi } from 'vitest'
import type { Track, DuplicateGroup } from '@shared/types'

// planAutoDedupe is pure, but metadata.ts pulls electron-touching modules at
// import time — stub everything it doesn't need for the plan logic.
vi.mock('electron', () => ({ shell: { trashItem: vi.fn() } }))
vi.mock('./db', () => ({ getDb: () => { throw new Error('unused in test') } }))
vi.mock('./coverArt', () => ({ storeCover: vi.fn() }))
vi.mock('./events', () => ({ broadcast: vi.fn() }))
vi.mock('./logger', () => ({ logWarn: vi.fn() }))
vi.mock('./tagIO', () => ({ writeTags: vi.fn(), verifyTags: vi.fn() }))
vi.mock('./settings', () => ({ getSettings: vi.fn() }))
vi.mock('./sync/tombstones', () => ({ recordTombstones: vi.fn() }))
vi.mock('./enrichment/lrc', () => ({ parseLrc: vi.fn() }))
vi.mock('./enrichment/services/lrclib', () => ({ fetchLyricsRemote: vi.fn() }))
vi.mock('./enrichment/pipeline', () => ({ enrichTrack: vi.fn() }))
vi.mock('./enrichment/autoEnrich', () => ({
  autoEnrichMissing: vi.fn(),
  ensureAutoEnrichScheduler: vi.fn(),
  requeueEnrichment: vi.fn(),
  enqueueEnrichment: vi.fn()
}))
vi.mock('./enrichment/maintenance', () => ({
  getEnrichmentStats: vi.fn(),
  getEnrichmentTracks: vi.fn(),
  retryFailedEnrichment: vi.fn(),
  backfillCovers: vi.fn(),
  recheckCovers: vi.fn()
}))
// isUnder/TRASH_DIR_NAME come from library.ts, which also can't be imported
// under vitest (music-metadata/db chain) — mirror the real boundary-safe logic.
vi.mock('./library', () => ({
  rebuildAggregates: vi.fn(),
  TRASH_DIR_NAME: '.trash',
  isUnder: (p: string, folder: string): boolean => {
    const a = p.toLowerCase()
    const b = folder.toLowerCase().replace(/[\\/]+$/, '')
    return a === b || a.startsWith(b + '/') || a.startsWith(b + '\\')
  }
}))

import { planAutoDedupe } from './metadata'

const DL = '/sdcard/Music/Aether'

let nextId = 1
function track(over: Partial<Track> = {}): Track {
  return {
    id: nextId++,
    path: `${DL}/song-${nextId}.mp3`,
    title: 'Song',
    artist: 'Artist',
    album: 'Album',
    duration: 200,
    bitrate: 320_000,
    sample_rate: 44_100,
    codec: 'mp3',
    file_size: 8_000_000,
    ...over
  } as Track
}

function group(reason: 'tags' | 'fingerprint', tracks: Track[]): DuplicateGroup {
  return { reason, tracks }
}

describe('planAutoDedupe', () => {
  it('removes a true duplicate (same album) inside the download folder', () => {
    const keep = track({ bitrate: 320_000 })
    const lose = track({ bitrate: 128_000 })
    const plans = planAutoDedupe([group('tags', [keep, lose])], DL, 'higher')
    expect(plans).toHaveLength(1)
    expect(plans[0].survivor.id).toBe(keep.id)
    expect(plans[0].victims.map((v) => v.id)).toEqual([lose.id])
  })

  it('re-partitions tags groups by album: different releases are NOT duplicates', () => {
    // findDuplicates groups by title+artist only; the same song on the studio
    // album and on a compilation must both survive.
    const studio = track({ album: 'Studio Album' })
    const compilation = track({ album: 'Greatest Hits' })
    expect(planAutoDedupe([group('tags', [studio, compilation])], DL, 'higher')).toEqual([])
  })

  it('folds album tags that differ only by case/punctuation into one subgroup', () => {
    const a = track({ album: "B'Day", bitrate: 320_000 })
    const b = track({ album: 'B’Day', bitrate: 128_000 })
    const plans = planAutoDedupe([group('tags', [a, b])], DL, 'higher')
    expect(plans).toHaveLength(1)
    expect(plans[0].survivor.id).toBe(a.id)
  })

  it('passes fingerprint groups through without album partitioning', () => {
    // audio-identical files are duplicates whatever their album tags say
    const a = track({ album: 'X', bitrate: 320_000 })
    const b = track({ album: 'Y', bitrate: 128_000 })
    const plans = planAutoDedupe([group('fingerprint', [a, b])], DL, 'higher')
    expect(plans).toHaveLength(1)
    expect(plans[0].victims.map((v) => v.id)).toEqual([b.id])
  })

  it('never victimizes files outside the download folder', () => {
    const inside = track({ bitrate: 320_000 })
    const outside = track({ path: '/sdcard/MyOldCollection/song.mp3', bitrate: 128_000 })
    // the loser lives outside → no eligible victims → group skipped entirely
    expect(planAutoDedupe([group('tags', [inside, outside])], DL, 'higher')).toEqual([])
  })

  it('does not match sibling folders as inside (boundary-safe isUnder)', () => {
    const a = track({ path: '/sdcard/Music/AetherOld/a.mp3', bitrate: 320_000 })
    const b = track({ path: '/sdcard/Music/AetherOld/b.mp3', bitrate: 128_000 })
    expect(planAutoDedupe([group('tags', [a, b])], DL, 'higher')).toEqual([])
  })

  it('victimizes only the in-folder losers of a mixed group', () => {
    const survivor = track({ bitrate: 320_000 })
    const inFolder = track({ bitrate: 192_000 })
    const preExisting = track({ path: '/sdcard/Imported/song.mp3', bitrate: 128_000 })
    const plans = planAutoDedupe([group('tags', [survivor, inFolder, preExisting])], DL, 'higher')
    expect(plans).toHaveLength(1)
    expect(plans[0].victims.map((v) => v.id)).toEqual([inFolder.id])
  })

  it('respects the keep=lower preference', () => {
    const hi = track({ bitrate: 320_000 })
    const lo = track({ bitrate: 128_000 })
    const plans = planAutoDedupe([group('tags', [hi, lo])], DL, 'lower')
    expect(plans[0].survivor.id).toBe(lo.id)
    expect(plans[0].victims.map((v) => v.id)).toEqual([hi.id])
  })

  it('returns no plans when the download folder is unset', () => {
    const a = track()
    const b = track()
    expect(planAutoDedupe([group('tags', [a, b])], '', 'higher')).toEqual([])
  })
})
