import { describe, it, expect } from 'vitest'
import type { PhoneTrackInfo } from '@shared/types'
import { needsEnrich, planTrack } from './plan'

// Fully-repaired reference track: mp3 at the default target, enriched, cover,
// genre and year all present → plan must be a no-op. Each test flips exactly
// the field under scrutiny.
function info(overrides: Partial<PhoneTrackInfo> = {}): PhoneTrackInfo {
  return {
    id: 1,
    trackKey: 'artist|title|album',
    title: 'Title',
    artist: 'Artist',
    album: 'Album',
    year: 2020,
    genre: 'Rock',
    durationS: 200,
    codec: 'MPEG 1 Layer 3',
    bitrate: 320_000,
    sampleRate: 44_100,
    fileSize: 8_000_000,
    mtimeMs: 1_700_000_000_000,
    ext: '.mp3',
    basename: '01 - Title.mp3',
    hasCover: true,
    enrichStatus: 'ok',
    ...overrides
  }
}

describe('needsEnrich', () => {
  it('is false for a complete track', () => {
    expect(needsEnrich(info())).toBe(false)
  })

  it.each([
    ['enrichStatus never ok', { enrichStatus: null }],
    ['enrichStatus no-match', { enrichStatus: 'no-match' }],
    ['missing cover', { hasCover: false }],
    ['missing genre', { genre: null }],
    ['missing year', { year: null }]
  ] as const)('is true with %s', (_label, patch) => {
    expect(needsEnrich(info(patch))).toBe(true)
  })
})

describe('planTrack', () => {
  it('gives ok when neither codec nor metadata need work', () => {
    const plan = planTrack(info(), 'mp3-320')
    expect(plan.badge).toBe('ok')
    expect(plan.transcode.needed).toBe(false)
    expect(plan.transcode.reason).toBe('match')
    expect(plan.enrich).toBe(false)
  })

  it('gives needs-codec for an off-target codec with complete metadata', () => {
    const plan = planTrack(info({ codec: 'Opus', ext: '.opus' }), 'mp3-320')
    expect(plan.badge).toBe('needs-codec')
    expect(plan.transcode).toEqual({ needed: true, reason: 'convert', targetExt: '.mp3' })
    expect(plan.enrich).toBe(false)
  })

  it('gives needs-enrich for an on-target codec with incomplete metadata', () => {
    const plan = planTrack(info({ hasCover: false }), 'mp3-320')
    expect(plan.badge).toBe('needs-enrich')
    expect(plan.transcode.needed).toBe(false)
    expect(plan.enrich).toBe(true)
  })

  it('gives both when codec and metadata both need work', () => {
    const plan = planTrack(info({ codec: 'Opus', ext: '.opus', enrichStatus: null }), 'mp3-320')
    expect(plan.badge).toBe('both')
    expect(plan.transcode.needed).toBe(true)
    expect(plan.enrich).toBe(true)
  })

  it('never plans a lossy→flac upconvert: badge falls back to the enrich side', () => {
    // Complete metadata + lossy source + flac target → nothing at all to do.
    const clean = planTrack(info(), 'flac')
    expect(clean.transcode).toEqual({ needed: false, reason: 'skip-upconvert', targetExt: '.flac' })
    expect(clean.badge).toBe('ok')
    // Incomplete metadata → the enrich half still applies, alone.
    const dirty = planTrack(info({ genre: null }), 'flac')
    expect(dirty.badge).toBe('needs-enrich')
  })

  it('plans a true lossless conversion to flac', () => {
    const plan = planTrack(info({ codec: 'PCM S16 LE', ext: '.wav' }), 'flac')
    expect(plan.transcode).toEqual({ needed: true, reason: 'convert', targetExt: '.flac' })
    expect(plan.badge).toBe('needs-codec')
  })

  it('falls back to the extension when the codec string is missing', () => {
    const plan = planTrack(info({ codec: null, ext: '.opus' }), 'mp3-320')
    expect(plan.transcode.needed).toBe(true)
    expect(plan.badge).toBe('needs-codec')
  })
})
