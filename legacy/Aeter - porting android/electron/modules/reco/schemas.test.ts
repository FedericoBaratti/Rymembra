import { describe, it, expect } from 'vitest'
import {
  LbSimilarSchema,
  LbRadioSchema,
  LastfmSimilarTracksSchema,
  LastfmSimilarArtistsSchema
} from './schemas'

describe('LbSimilarSchema', () => {
  it('parses labs similar-recordings rows and keeps the reference comment', () => {
    const data = LbSimilarSchema.parse([
      { recording_mbid: 'seed', comment: 'reference' },
      { recording_mbid: 'a', score: 120, recording_name: 'X', artist_credit_name: 'Y' }
    ])
    expect(data).toHaveLength(2)
    expect(data[1]).toMatchObject({ recording_mbid: 'a', score: 120 })
  })
})

describe('LbRadioSchema', () => {
  it('accepts identifier as string or string[]', () => {
    const parsed = LbRadioSchema.parse({
      payload: {
        jspf: {
          playlist: {
            track: [
              { identifier: 'https://musicbrainz.org/recording/abc', title: 'T', creator: 'A' },
              { identifier: ['https://musicbrainz.org/recording/def'], title: 'U', creator: 'B' }
            ]
          }
        }
      }
    })
    const tracks = parsed.payload?.jspf?.playlist?.track ?? []
    expect(tracks).toHaveLength(2)
  })

  it('tolerates a missing payload', () => {
    expect(() => LbRadioSchema.parse({})).not.toThrow()
  })
})

describe('LastfmSimilarTracksSchema', () => {
  it('coerces the stringified match float to a number', () => {
    const parsed = LastfmSimilarTracksSchema.parse({
      similartracks: {
        track: [{ name: 'Song', mbid: 'm', match: '0.83', artist: { name: 'Artist', mbid: 'am' } }]
      }
    })
    expect(parsed.similartracks?.track?.[0].match).toBeCloseTo(0.83)
  })
})

describe('LastfmSimilarArtistsSchema', () => {
  it('parses similar artists with numeric match coercion', () => {
    const parsed = LastfmSimilarArtistsSchema.parse({
      similarartists: { artist: [{ name: 'A', match: '0.5' }] }
    })
    expect(parsed.similarartists?.artist?.[0]).toMatchObject({ name: 'A', match: 0.5 })
  })
})
