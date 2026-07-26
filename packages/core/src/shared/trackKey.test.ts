import { describe, it, expect } from 'vitest'
import { trackKey, upgradeLegacyTrackKey, normalizeKey, playlistKey } from './trackKey'

describe('trackKey (v2)', () => {
  it('is artist|title|album — three segments, no duration', () => {
    expect(trackKey({ artist: 'a', title: 'b', album: 'c' })).toBe('a|b|c')
  })

  it('is insensitive to duration drift by construction', () => {
    // the exact case that used to fork keys: same song, different encodes
    const fromYoutube = trackKey({ artist: 'Artist', title: 'Song', album: 'Album' })
    const fromSpotify = trackKey({ artist: 'artist', title: 'song', album: 'album' })
    expect(fromYoutube).toBe(fromSpotify)
  })

  it('folds case, diacritics and punctuation variants', () => {
    const a = trackKey({ artist: 'Beyoncé', title: 'Déjà Vu', album: 'B’Day' })
    const b = trackKey({ artist: 'BEYONCE', title: 'Deja Vu', album: "B'Day" })
    expect(a).toBe(b)
  })

  it('treats null/undefined tags as empty segments', () => {
    expect(trackKey({ artist: null, title: 'x', album: undefined })).toBe('|x|')
  })
})

describe('upgradeLegacyTrackKey', () => {
  it('drops the numeric duration tail of a v1 key', () => {
    expect(upgradeLegacyTrackKey('a|b|c|200')).toBe('a|b|c')
    expect(upgradeLegacyTrackKey('artist|title||0')).toBe('artist|title|')
  })

  it('passes v2 keys through unchanged (idempotent)', () => {
    expect(upgradeLegacyTrackKey('a|b|c')).toBe('a|b|c')
    expect(upgradeLegacyTrackKey(upgradeLegacyTrackKey('a|b|c|200'))).toBe('a|b|c')
  })

  it('does not touch a 4-segment key whose tail is not all digits', () => {
    expect(upgradeLegacyTrackKey('a|b|c|x1')).toBe('a|b|c|x1')
  })

  it('does not touch playlist keys (single segment)', () => {
    expect(upgradeLegacyTrackKey('my playlist')).toBe('my playlist')
  })
})

describe('normalizeKey / playlistKey', () => {
  it('normalizes and collapses whitespace', () => {
    expect(normalizeKey('  Hello,  World!  ')).toBe('hello world')
    expect(playlistKey('MY  Playlist')).toBe('my playlist')
  })
})
