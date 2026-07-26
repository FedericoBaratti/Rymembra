import { describe, it, expect } from 'vitest'
import {
  candidateKey,
  buildLibraryIndex,
  blendCandidates,
  splitByLibrary,
  scoreLocalAffinity,
  rankByAffinity,
  type RawCandidate,
  type IndexableTrack
} from './engine'

const cand = (p: Partial<RawCandidate>): RawCandidate => ({
  mbid: null,
  title: null,
  artist: null,
  score: 1,
  source: 'listenbrainz',
  ...p
})

const track = (p: Partial<IndexableTrack> & { id: number }): IndexableTrack => ({
  title: 'T',
  artist: 'A',
  ...p
})

describe('candidateKey', () => {
  it('normalizes case, accents and feat. decorations', () => {
    expect(candidateKey('Beyoncé', 'Halo (feat. X)')).toBe(candidateKey('beyonce', 'Halo'))
  })
})

describe('buildLibraryIndex', () => {
  it('indexes by lowercased mbid and by normalized name', () => {
    const idx = buildLibraryIndex([
      track({ id: 1, artist: 'Daft Punk', title: 'One More Time', mb_recording_id: 'ABC-123' })
    ])
    expect(idx.byMbid.get('abc-123')).toBe(1)
    expect(idx.byName.get(candidateKey('Daft Punk', 'One More Time'))).toBe(1)
  })

  it('keeps the first id on collisions', () => {
    const idx = buildLibraryIndex([
      track({ id: 1, artist: 'A', title: 'Song' }),
      track({ id: 2, artist: 'A', title: 'Song' })
    ])
    expect(idx.byName.get(candidateKey('A', 'Song'))).toBe(1)
  })
})

describe('blendCandidates', () => {
  it('collapses the same recording from two sources and sums weighted scores', () => {
    const blended = blendCandidates([
      { weight: 1, items: [cand({ mbid: 'm1', artist: 'A', title: 'X', score: 0.5 })] },
      { weight: 0.5, items: [cand({ mbid: 'm1', artist: 'A', title: 'X', score: 1, source: 'lastfm' })] }
    ])
    expect(blended).toHaveLength(1)
    expect(blended[0].score).toBeCloseTo(1 * 0.5 + 0.5 * 1) // 1.0
    expect([...blended[0].sources].sort()).toEqual(['lastfm', 'listenbrainz'])
  })

  it('collapses by normalized name when no mbid is present', () => {
    const blended = blendCandidates([
      { weight: 1, items: [cand({ artist: 'Beyoncé', title: 'Halo (Live)', score: 1 })] },
      { weight: 1, items: [cand({ artist: 'beyonce', title: 'Halo', score: 1 })] }
    ])
    expect(blended).toHaveLength(1)
  })

  it('drops unusable candidates (no mbid and missing artist/title)', () => {
    const blended = blendCandidates([{ weight: 1, items: [cand({ artist: 'A', title: null })] }])
    expect(blended).toHaveLength(0)
  })

  it('sorts by descending score', () => {
    const blended = blendCandidates([
      {
        weight: 1,
        items: [
          cand({ mbid: 'a', artist: 'A', title: 'low', score: 0.1 }),
          cand({ mbid: 'b', artist: 'B', title: 'high', score: 0.9 })
        ]
      }
    ])
    expect(blended.map((b) => b.mbid)).toEqual(['b', 'a'])
  })
})

describe('splitByLibrary', () => {
  const index = buildLibraryIndex([
    track({ id: 7, artist: 'Owned', title: 'Mine', mb_recording_id: 'owned-mbid' }),
    track({ id: 8, artist: 'ByName', title: 'AlsoMine' })
  ])

  it('resolves owned tracks by mbid then by name; rest become external', () => {
    const blended = blendCandidates([
      {
        weight: 1,
        items: [
          cand({ mbid: 'owned-mbid', artist: 'Owned', title: 'Mine', score: 1 }),
          cand({ artist: 'ByName', title: 'AlsoMine', score: 0.9 }),
          cand({ artist: 'Stranger', title: 'NotOwned', score: 0.8 })
        ]
      }
    ])
    const res = splitByLibrary(blended, index)
    expect(res.inLibrary).toEqual([7, 8])
    expect(res.external).toHaveLength(1)
    expect(res.external[0]).toMatchObject({ artist: 'Stranger', title: 'NotOwned' })
  })

  it('dedupes repeated external candidates', () => {
    const blended = blendCandidates([
      {
        weight: 1,
        items: [
          cand({ mbid: 'x1', artist: 'New', title: 'Tune', score: 1 }),
          cand({ artist: 'New', title: 'Tune', score: 0.7 })
        ]
      }
    ])
    // both collapse to one in blend (name vs mbid differ → two entries), but
    // splitByLibrary must not emit the same external twice by name
    const res = splitByLibrary(blended, index)
    const keys = res.external.map((e) => candidateKey(e.artist, e.title))
    expect(new Set(keys).size).toBe(keys.length)
  })
})

describe('scoreLocalAffinity', () => {
  const now = 1_000_000_000_000

  it('gives a liked, recently-played, high-rated track a high score', () => {
    const hot = scoreLocalAffinity(
      track({ id: 1, play_count: 50, last_played: now, rating: 5, liked: 1 }),
      now
    )
    const cold = scoreLocalAffinity(track({ id: 2, play_count: 0, last_played: null, rating: 0 }), now)
    expect(hot).toBeGreaterThan(cold)
    expect(cold).toBe(0)
  })

  it('decays with time since last play', () => {
    const recent = scoreLocalAffinity(track({ id: 1, last_played: now }), now)
    const old = scoreLocalAffinity(track({ id: 1, last_played: now - 60 * 24 * 60 * 60 * 1000 }), now)
    expect(recent).toBeGreaterThan(old)
  })
})

describe('rankByAffinity', () => {
  it('returns the top-N tracks by affinity, descending', () => {
    const now = 2_000_000_000_000
    const ranked = rankByAffinity(
      [
        track({ id: 1, play_count: 1 }),
        track({ id: 2, play_count: 100, last_played: now, liked: 1 }),
        track({ id: 3, play_count: 5, rating: 3 })
      ],
      now,
      2
    )
    expect(ranked).toHaveLength(2)
    expect(ranked[0].id).toBe(2)
  })
})
