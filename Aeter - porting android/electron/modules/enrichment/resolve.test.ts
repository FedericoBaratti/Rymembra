import { describe, expect, it, vi, beforeEach } from 'vitest'

vi.mock('../logger', () => ({ logWarn: vi.fn(), logError: vi.fn() }))
vi.mock('./services/musicbrainz', () => ({ mbSearchRecordings: vi.fn(), mbGetRecording: vi.fn() }))
vi.mock('./services/itunes', () => ({ itunesSearch: vi.fn() }))
vi.mock('./services/deezer', () => ({ deezerSearch: vi.fn() }))

import { resolveMetadata } from './resolve'
import { scoreCandidate, type MetaCandidate } from './match'
import { mbSearchRecordings, mbGetRecording } from './services/musicbrainz'
import { itunesSearch } from './services/itunes'
import { deezerSearch } from './services/deezer'
import { NetworkError } from '../net/errors'
import type { MbRecording } from './schemas'

const mbMock = vi.mocked(mbSearchRecordings)
const mbGetMock = vi.mocked(mbGetRecording)
const itunesMock = vi.mocked(itunesSearch)
const deezerMock = vi.mocked(deezerSearch)

const track = { title: 'Wish You Were Here', artist: 'Pink Floyd', duration: 334 }

function itunesCand(over: Partial<MetaCandidate> = {}): MetaCandidate {
  return {
    source: 'itunes',
    title: 'Wish You Were Here',
    artist: 'Pink Floyd',
    durationMs: 334_000,
    coverUrl: 'https://itunes/cover.jpg',
    ...over
  }
}

beforeEach(() => {
  mbMock.mockReset()
  mbGetMock.mockReset()
  itunesMock.mockReset()
  deezerMock.mockReset()
  mbMock.mockResolvedValue([])
  mbGetMock.mockResolvedValue(null)
  itunesMock.mockResolvedValue([])
  deezerMock.mockResolvedValue([])
})

describe('resolveMetadata', () => {
  it('matches via iTunes when MusicBrainz returns nothing', async () => {
    itunesMock.mockResolvedValue([itunesCand()])
    const res = await resolveMetadata(track)
    expect(res?.best.source).toBe('itunes')
    expect(res?.best.coverUrl).toBe('https://itunes/cover.jpg')
    expect(res?.confidence).toBeGreaterThanOrEqual(0.65)
  })

  it('returns null when every candidate is below the threshold', async () => {
    itunesMock.mockResolvedValue([
      itunesCand({ title: 'Completely Different', artist: 'Other Band', durationMs: 120_000 })
    ])
    expect(await resolveMetadata(track)).toBeNull()
  })

  it('boosts confidence when two providers agree', async () => {
    // a slightly-off title keeps the base score below 0.9 so the +0.1 boost is observable
    const cand = { title: 'Wish You Were Here (Live)', artist: 'Pink Floyd', durationMs: 330_000 }
    itunesMock.mockResolvedValue([itunesCand(cand)])
    deezerMock.mockResolvedValue([{ ...itunesCand(cand), source: 'deezer' }])
    const base = scoreCandidate(track, cand)
    const res = await resolveMetadata(track)
    expect(res?.confidence).toBeCloseTo(Math.min(1, base + 0.1), 5)
  })

  it('borrows a cover URL from an agreeing provider when the winner has none', async () => {
    const rec: MbRecording = {
      id: 'mb1',
      title: 'Wish You Were Here',
      length: 334_000,
      'artist-credit': [{ name: 'Pink Floyd' }],
      releases: [
        {
          id: 'rel1',
          title: 'Wish You Were Here',
          status: 'Official',
          'release-group': { id: 'rg1', 'primary-type': 'Album' }
        }
      ]
    }
    mbMock.mockResolvedValue([rec])
    deezerMock.mockResolvedValue([{ ...itunesCand(), source: 'deezer', coverUrl: 'https://deezer/xl.jpg' }])
    const res = await resolveMetadata(track)
    // MB and Deezer tie on score; whichever wins must end up with a cover URL.
    expect(res?.best.coverUrl).toBeTruthy()
    expect(res?.best.mbReleaseGroupId === 'rg1' || res?.best.source === 'deezer').toBe(true)
  })

  it('returns needs-review for a title-only match (unknown artist, no duration)', async () => {
    const anonymous = { title: 'Wish You Were Here', artist: 'Artista sconosciuto', duration: 0 }
    itunesMock.mockResolvedValue([itunesCand()])
    const res = await resolveMetadata(anonymous)
    expect(res?.verdict).toBe('needs-review')
    expect(res?.evidence.artistSim).toBeNull()
    expect(res?.evidence.durationDeltaSec).toBeNull()
  })

  it('does not borrow a cover from a provider that only agrees on title+artist', async () => {
    // Same song name but a duration 30s apart and no album on either side:
    // that cover likely belongs to a different release.
    itunesMock.mockResolvedValue([itunesCand({ coverUrl: null })])
    deezerMock.mockResolvedValue([
      { ...itunesCand(), source: 'deezer', durationMs: 364_000, coverUrl: 'https://deezer/xl.jpg' }
    ])
    const res = await resolveMetadata(track)
    expect(res?.best.source).toBe('itunes')
    expect(res?.best.coverUrl).toBeNull()
    expect(res?.evidence.consensus).toBe(false)
  })

  it('throws NetworkError when all providers are unreachable', async () => {
    mbMock.mockRejectedValue(new NetworkError('mb down'))
    itunesMock.mockRejectedValue(new NetworkError('itunes down'))
    deezerMock.mockRejectedValue(new NetworkError('deezer down'))
    await expect(resolveMetadata(track)).rejects.toBeInstanceOf(NetworkError)
  })
})

describe('resolveMetadata — AcoustID corroboration', () => {
  const acoustidRec: MbRecording = {
    id: 'rec-abc',
    title: 'Wish You Were Here',
    length: 334_000,
    'artist-credit': [{ name: 'Pink Floyd' }],
    releases: [
      {
        id: 'rel-ac',
        title: 'Wish You Were Here',
        status: 'Official',
        date: '1975-09-12',
        'release-group': { id: 'rg-ac', 'primary-type': 'Album' }
      }
    ]
  } as MbRecording

  it('trusts an AcoustID hit over garbage local tags and applies with high confidence', async () => {
    mbGetMock.mockResolvedValue(acoustidRec)
    // Messy tags no textual search would match; the audio fingerprint identifies it.
    const messy = { title: 'pinkfloyd_wywh_320', artist: 'Artista sconosciuto', duration: 334 }
    const res = await resolveMetadata(messy, null, 'rec-abc')

    expect(res).not.toBeNull()
    expect(res!.verdict).toBe('apply')
    expect(res!.confidence).toBeGreaterThanOrEqual(0.95)
    expect(res!.best.mbRecordingId).toBe('rec-abc')
    expect(res!.best.title).toBe('Wish You Were Here')
    // AcoustID flows through the fingerprint channel of the decision.
    expect(res!.evidence.fingerprint).toBe('corroborated')
    expect(mbGetMock).toHaveBeenCalledWith('rec-abc')
  })

  it('degrades gracefully when the AcoustID recording cannot be fetched (Android: no fpcalc)', async () => {
    // id given but MB lookup misses and there are no textual candidates → null,
    // exactly the desktop-no-key / Android (acoustidRecordingId undefined) behaviour.
    mbGetMock.mockResolvedValue(null)
    const res = await resolveMetadata(
      { title: 'pinkfloyd_wywh_320', artist: 'Artista sconosciuto', duration: 334 },
      null,
      'rec-missing'
    )
    expect(res).toBeNull()
  })
})
