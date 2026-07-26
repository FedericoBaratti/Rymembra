import { describe, it, expect } from 'vitest'
import { mergeSync } from './merge'
import type { SyncFile, SyncTrack, SyncPlaylist } from './schema'
import { trackKey, playlistKey, normalizeKey } from '@shared/trackKey'

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

function playlist(over: Partial<SyncPlaylist> = {}): SyncPlaylist {
  return {
    name: 'P',
    description: null,
    createdAt: 0,
    updatedAt: 0,
    isSmart: 0,
    rules: null,
    trackKeys: [],
    ...over
  }
}

function file(over: Partial<SyncFile> = {}): SyncFile {
  return {
    version: 1,
    generatedAt: 0,
    generatedBy: 'dev1',
    tracks: {},
    playlists: {},
    tombstones: { tracks: {}, playlists: {} },
    ...over
  }
}

describe('mergeSync — tracks', () => {
  it('takes the higher play count and lastPlayed and writes it back', () => {
    const local = file({ tracks: { k: track({ playCount: 5, lastPlayed: 100 }) } })
    const remote = file({ generatedBy: 'dev2', tracks: { k: track({ playCount: 8, lastPlayed: 50 }) } })
    const { merged, writeback, changed } = mergeSync(local, remote)
    expect(merged.tracks.k.playCount).toBe(8)
    expect(merged.tracks.k.lastPlayed).toBe(100)
    expect(writeback.tracks.k.playCount).toBe(8)
    expect(changed).toBe(true)
  })

  it('resolves a rating conflict by the newer statsUpdatedAt (remote newer)', () => {
    const local = file({ tracks: { k: track({ rating: 3, statsUpdatedAt: 100 }) } })
    const remote = file({ tracks: { k: track({ rating: 5, statsUpdatedAt: 200 }) } })
    const { merged, writeback } = mergeSync(local, remote)
    expect(merged.tracks.k.rating).toBe(5)
    expect(merged.tracks.k.statsUpdatedAt).toBe(200)
    expect(writeback.tracks.k.rating).toBe(5)
  })

  it('keeps the local rating when local stats are newer and writes nothing back', () => {
    const local = file({ tracks: { k: track({ rating: 5, statsUpdatedAt: 200 }) } })
    const remote = file({ tracks: { k: track({ rating: 3, statsUpdatedAt: 100 }) } })
    const { merged, writeback, changed } = mergeSync(local, remote)
    expect(merged.tracks.k.rating).toBe(5)
    expect(writeback.tracks).toEqual({})
    expect(changed).toBe(false)
  })

  it('resolves liked/likedAt under the same stats clock', () => {
    const local = file({ tracks: { k: track({ liked: 0, likedAt: null, statsUpdatedAt: 100 }) } })
    const remote = file({ tracks: { k: track({ liked: 1, likedAt: 500, statsUpdatedAt: 200 }) } })
    const { merged } = mergeSync(local, remote)
    expect(merged.tracks.k.liked).toBe(1)
    expect(merged.tracks.k.likedAt).toBe(500)
  })

  it('reborns a locally-present track that the remote tombstoned', () => {
    const local = file({ tracks: { k: track() } })
    const remote = file({ tombstones: { tracks: { k: 999 }, playlists: {} } })
    const { merged } = mergeSync(local, remote)
    expect(merged.tracks.k).toBeDefined()
    expect(merged.tombstones.tracks.k).toBeUndefined()
  })

  it('keeps only the tombstone when a track is dead everywhere', () => {
    const local = file({ tombstones: { tracks: { k: 300 }, playlists: {} } })
    const remote = file({ tracks: { k: track({ statsUpdatedAt: 200 }) } })
    const { merged, changed } = mergeSync(local, remote)
    expect(merged.tracks.k).toBeUndefined()
    expect(merged.tombstones.tracks.k).toBe(300)
    expect(changed).toBe(false)
  })

  it('passes a remote track through when its activity is newer than a tombstone', () => {
    const local = file({ tombstones: { tracks: { k: 100 }, playlists: {} } })
    const remote = file({ tracks: { k: track({ statsUpdatedAt: 200, playCount: 3 }) } })
    const { merged, writeback } = mergeSync(local, remote)
    expect(merged.tracks.k.playCount).toBe(3)
    expect(merged.tombstones.tracks.k).toBeUndefined()
    expect(writeback.tracks).toEqual({}) // no local row to update
  })

  it('passes a remote-only track through without writeback', () => {
    const local = file()
    const remote = file({ tracks: { k: track({ playCount: 3 }) } })
    const { merged, writeback, changed } = mergeSync(local, remote)
    expect(merged.tracks.k).toBeDefined()
    expect(writeback.tracks).toEqual({})
    expect(changed).toBe(false)
  })
})

describe('mergeSync — playlists', () => {
  it('applies the newer playlist (remote wins) and writes it back', () => {
    const local = file({ playlists: { p: playlist({ updatedAt: 100, trackKeys: ['a'] }) } })
    const remote = file({ playlists: { p: playlist({ updatedAt: 200, trackKeys: ['a', 'b'] }) } })
    const { merged, writeback } = mergeSync(local, remote)
    expect(merged.playlists.p.trackKeys).toEqual(['a', 'b'])
    expect(writeback.playlists.p).toBeDefined()
  })

  it('keeps the local playlist when it is newer', () => {
    const local = file({ playlists: { p: playlist({ updatedAt: 200 }) } })
    const remote = file({ playlists: { p: playlist({ updatedAt: 100 }) } })
    const { merged, writeback, changed } = mergeSync(local, remote)
    expect(merged.playlists.p.updatedAt).toBe(200)
    expect(writeback.playlists).toEqual({})
    expect(changed).toBe(false)
  })

  it('propagates a playlist deletion (remote tombstone newer than local edit)', () => {
    const local = file({ playlists: { p: playlist({ updatedAt: 100 }) } })
    const remote = file({ tombstones: { tracks: {}, playlists: { p: 200 } } })
    const { merged, writeback, changed } = mergeSync(local, remote)
    expect(merged.playlists.p).toBeUndefined()
    expect(merged.tombstones.playlists.p).toBe(200)
    expect(writeback.deletedPlaylists).toContain('p')
    expect(changed).toBe(true)
  })

  it('resurrects a playlist edited after a stale remote deletion', () => {
    const local = file({ playlists: { p: playlist({ updatedAt: 300 }) } })
    const remote = file({ tombstones: { tracks: {}, playlists: { p: 200 } } })
    const { merged, writeback } = mergeSync(local, remote)
    expect(merged.playlists.p).toBeDefined()
    expect(writeback.deletedPlaylists).toEqual([])
  })

  it('creates a remote-only playlist locally', () => {
    const local = file()
    const remote = file({ playlists: { p: playlist({ updatedAt: 50 }) } })
    const { merged, writeback } = mergeSync(local, remote)
    expect(merged.playlists.p).toBeDefined()
    expect(writeback.playlists.p).toBeDefined()
  })
})

describe('mergeSync — first sync (no remote)', () => {
  it('round-trips local with no writeback', () => {
    const local = file({
      tracks: { k: track({ playCount: 2 }) },
      playlists: { p: playlist() }
    })
    const { merged, writeback, changed } = mergeSync(local, null)
    expect(merged.tracks.k.playCount).toBe(2)
    expect(merged.playlists.p).toBeDefined()
    expect(changed).toBe(false)
    expect(writeback).toEqual({ tracks: {}, playlists: {}, deletedPlaylists: [] })
  })
})

describe('trackKey / playlistKey normalization', () => {
  it('folds case, diacritics and punctuation to one key', () => {
    const a = trackKey({ artist: 'Beyoncé', title: 'Déjà Vu', album: 'B’Day' })
    const b = trackKey({ artist: 'BEYONCE', title: 'Deja Vu', album: "B'Day" })
    expect(a).toBe(b)
  })

  it('builds a three-segment key without duration', () => {
    expect(trackKey({ artist: 'a', title: 'b', album: 'c' })).toBe('a|b|c')
  })

  it('treats null/undefined tags as empty segments', () => {
    expect(trackKey({ artist: null, title: 'x', album: undefined })).toBe('|x|')
  })

  it('normalizes and collapses whitespace', () => {
    expect(normalizeKey('  Hello,  World!  ')).toBe('hello world')
    expect(playlistKey('MY  Playlist')).toBe('my playlist')
  })
})
