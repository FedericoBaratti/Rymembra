import { describe, expect, it } from 'vitest'
import { decide, type Evidence } from './decision'

function ev(over: Partial<Evidence> = {}): Evidence {
  return {
    titleSim: 1,
    artistSim: 1,
    durationDeltaSec: 0,
    albumSim: null,
    consensus: false,
    fingerprint: 'none',
    ...over
  }
}

describe('decide', () => {
  it('abstains on a title-only match (unknown artist + unknown duration)', () => {
    // V2: neutral 0.5 axes used to push title-only matches past the 0.65
    // threshold (0.4·1 + 0.3·0.5 + 0.3·0.5 = 0.70). Unknown must not count.
    const d = decide(ev({ artistSim: null, durationDeltaSec: null }), 0.7)
    expect(d.verdict).toBe('needs-review')
  })

  it('applies with two strong axes (title + duration, artist unknown)', () => {
    const d = decide(ev({ artistSim: null, durationDeltaSec: 2 }), 0.85)
    expect(d.verdict).toBe('apply')
  })

  it('applies with strong title + strong artist when duration is unknown', () => {
    const d = decide(ev({ artistSim: 0.95, durationDeltaSec: null }), 0.85)
    expect(d.verdict).toBe('apply')
  })

  it('near-miss axes never count as strong', () => {
    const d = decide(
      ev({ titleSim: 0.89, artistSim: 0.84, durationDeltaSec: 5 }),
      0.8
    )
    expect(d.verdict).toBe('needs-review')
  })

  it('demotes to needs-review when the known album contradicts (karaoke/remix)', () => {
    // V4: same title+duration but the candidate comes from a karaoke album.
    const d = decide(ev({ albumSim: 0.1 }), 0.9)
    expect(d.verdict).toBe('needs-review')
  })

  it('demotes to needs-review when the known artist contradicts (tribute band)', () => {
    const d = decide(ev({ artistSim: 0.2, durationDeltaSec: 1 }), 0.75)
    expect(d.verdict).toBe('needs-review')
  })

  it('an unknown album is not a contradiction', () => {
    const d = decide(ev({ albumSim: null }), 0.9)
    expect(d.verdict).toBe('apply')
  })

  it('applies on strong title + strict consensus when other axes are unknown', () => {
    const d = decide(
      ev({ artistSim: null, durationDeltaSec: null, consensus: true }),
      0.7
    )
    expect(d.verdict).toBe('apply')
  })

  it('consensus alone does not rescue a weak title', () => {
    const d = decide(
      ev({ titleSim: 0.8, artistSim: null, durationDeltaSec: null, consensus: true }),
      0.66
    )
    expect(d.verdict).toBe('needs-review')
  })

  it('corroborated fingerprint with textual agreement overrides a contradiction', () => {
    const d = decide(
      ev({ albumSim: 0.1, fingerprint: 'corroborated', fingerprintAgrees: true }),
      0.9
    )
    expect(d.verdict).toBe('apply')
    expect(d.confidence).toBe(1)
  })

  it('a corroborated fingerprint that disagrees with the candidate does not apply it', () => {
    const d = decide(
      ev({ titleSim: 0.7, artistSim: null, durationDeltaSec: null, fingerprint: 'corroborated', fingerprintAgrees: false }),
      0.66
    )
    expect(d.verdict).toBe('needs-review')
  })

  it('returns no-match below the ranking threshold', () => {
    const d = decide(
      ev({ titleSim: 0.5, artistSim: null, durationDeltaSec: null }),
      0.5
    )
    expect(d.verdict).toBe('no-match')
  })

  it('adds the consensus bonus to confidence and clamps at 1', () => {
    const low = decide(ev({ titleSim: 0.92, durationDeltaSec: 2, consensus: true }), 0.8)
    expect(low.confidence).toBeCloseTo(0.9, 5)
    const clamped = decide(ev({ consensus: true }), 0.95)
    expect(clamped.confidence).toBe(1)
  })
})
