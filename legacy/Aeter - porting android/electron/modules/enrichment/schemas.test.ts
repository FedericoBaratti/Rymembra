import { describe, expect, it } from 'vitest'
import {
  AcoustidLookupSchema,
  MbRecordingSchema,
  MbSearchSchema,
  LastfmTopTagsSchema,
  LrclibGetSchema
} from './schemas'

describe('AcoustidLookupSchema', () => {
  it('accepts a realistic payload', () => {
    const payload = {
      status: 'ok',
      results: [
        { id: 'x', score: 0.92, recordings: [{ id: 'rec-1' }, { id: 'rec-2' }] },
        { id: 'y', score: 0.41 }
      ]
    }
    const parsed = AcoustidLookupSchema.parse(payload)
    expect(parsed.results?.[0].recordings?.[0].id).toBe('rec-1')
  })

  it('accepts an empty result set', () => {
    expect(AcoustidLookupSchema.parse({}).results).toBeUndefined()
  })

  it('rejects malformed scores', () => {
    expect(() => AcoustidLookupSchema.parse({ results: [{ score: 'high' }] })).toThrow()
  })
})

describe('MbRecordingSchema', () => {
  it('accepts a realistic recording with releases and length', () => {
    const payload = {
      id: 'abc-123',
      title: 'Wish You Were Here',
      length: 334000,
      'artist-credit': [{ name: 'Pink Floyd', joinphrase: '' }],
      releases: [
        {
          id: 'rel-1',
          title: 'Wish You Were Here',
          date: '1975-09-12',
          media: [{ position: 1, 'track-offset': 0 }]
        }
      ],
      extra_field_to_strip: true
    }
    const parsed = MbRecordingSchema.parse(payload)
    expect(parsed.releases?.[0].date).toBe('1975-09-12')
    expect(parsed.length).toBe(334000)
    expect('extra_field_to_strip' in parsed).toBe(false)
  })

  it('accepts null length (MB returns null for some recordings)', () => {
    const parsed = MbRecordingSchema.parse({ id: 'x', title: 't', length: null })
    expect(parsed.length).toBeNull()
  })

  it('rejects a recording without id', () => {
    expect(() => MbRecordingSchema.parse({ title: 'no id' })).toThrow()
  })
})

describe('MbSearchSchema', () => {
  it('accepts search results with scores', () => {
    const payload = {
      created: '2026-01-01',
      count: 2,
      recordings: [
        { id: 'a', title: 'One', score: 100 },
        { id: 'b', title: 'Two', score: 87 }
      ]
    }
    expect(MbSearchSchema.parse(payload).recordings).toHaveLength(2)
  })

  it('accepts an empty body', () => {
    expect(MbSearchSchema.parse({}).recordings).toBeUndefined()
  })
})

describe('LastfmTopTagsSchema', () => {
  it('accepts a realistic toptags payload', () => {
    const payload = {
      toptags: {
        tag: [
          { name: 'progressive rock', count: 100, url: 'https://last.fm/tag/x' },
          { name: 'rock', count: 88 }
        ],
        '@attr': { artist: 'Pink Floyd', track: 'Wish You Were Here' }
      }
    }
    expect(LastfmTopTagsSchema.parse(payload).toptags?.tag?.[0].name).toBe('progressive rock')
  })

  it('accepts the error-shaped payload Last.fm returns for unknown tracks', () => {
    expect(LastfmTopTagsSchema.parse({ error: 6, message: 'Track not found' }).toptags).toBeUndefined()
  })

  it('rejects tags without a name', () => {
    expect(() => LastfmTopTagsSchema.parse({ toptags: { tag: [{ count: 3 }] } })).toThrow()
  })
})

describe('LrclibGetSchema', () => {
  it('accepts a realistic payload', () => {
    const payload = {
      id: 123,
      trackName: 'x',
      syncedLyrics: '[00:01.00]hello',
      plainLyrics: 'hello'
    }
    const parsed = LrclibGetSchema.parse(payload)
    expect(parsed.syncedLyrics).toContain('hello')
  })

  it('accepts null lyrics fields', () => {
    const parsed = LrclibGetSchema.parse({ syncedLyrics: null, plainLyrics: null })
    expect(parsed.syncedLyrics).toBeNull()
  })

  it('rejects non-string lyrics', () => {
    expect(() => LrclibGetSchema.parse({ syncedLyrics: 42 })).toThrow()
  })
})
