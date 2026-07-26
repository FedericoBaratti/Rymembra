import { describe, expect, it } from 'vitest'
import { cleanYoutubeArtist, cleanYoutubeMetadata, cleanYoutubeTitle } from './youtubeClean'

describe('cleanYoutubeTitle', () => {
  it.each([
    ['Song (Official Video)', 'Song'],
    ['Song [Official Music Video]', 'Song'],
    ['Song (Official Audio)', 'Song'],
    ['Song (Lyric Video)', 'Song'],
    ['Song (Lyrics)', 'Song'],
    ['Song (Audio)', 'Song'],
    ['Song (Visualizer)', 'Song'],
    ['Song (Official Visualiser)', 'Song'],
    ['Song [HD]', 'Song'],
    ['Song (4K)', 'Song'],
    ['Song (Video Ufficiale)', 'Song'],
    ['Song (Testo)', 'Song'],
    ['Song (Official Music Video) [HD]', 'Song'],
    ['Song | Official Video', 'Song'],
    ['Song // Lyrics', 'Song'],
    ['Song Official Video', 'Song'],
    ['Song Video Ufficiale', 'Song']
  ])('strips noise: %s → %s', (input, expected) => {
    expect(cleanYoutubeTitle(input)).toBe(expected)
  })

  it('strips wrapping quotes', () => {
    expect(cleanYoutubeTitle('“Song Title”')).toBe('Song Title')
    expect(cleanYoutubeTitle('"Song Title" (Official Video)')).toBe('Song Title')
  })

  it('keeps live/remaster qualifiers and feat. credits', () => {
    expect(cleanYoutubeTitle('Alive (Live)')).toBe('Alive (Live)')
    expect(cleanYoutubeTitle('Song (2013 Remaster)')).toBe('Song (2013 Remaster)')
    expect(cleanYoutubeTitle('Song (feat. Someone) (Official Video)')).toBe('Song (feat. Someone)')
  })

  it('keeps meaningful parentheses', () => {
    expect(cleanYoutubeTitle('Time (Clock of the Heart)')).toBe('Time (Clock of the Heart)')
  })

  it('is idempotent', () => {
    const samples = ['Song (Official Video) [HD]', 'Song | Official Audio', '“Song”']
    for (const s of samples) {
      const once = cleanYoutubeTitle(s)
      expect(cleanYoutubeTitle(once)).toBe(once)
    }
  })

  it('falls back to the raw title when cleaning empties it', () => {
    expect(cleanYoutubeTitle('(Official Video)')).toBe('(Official Video)')
  })
})

describe('cleanYoutubeArtist', () => {
  it.each([
    ['Coldplay - Topic', 'Coldplay'],
    ['ColdplayVEVO', 'Coldplay'],
    ['Coldplay VEVO', 'Coldplay'],
    ['Coldplay Official', 'Coldplay'],
    ['Coldplay Official Channel', 'Coldplay']
  ])('strips channel suffixes: %s → %s', (input, expected) => {
    expect(cleanYoutubeArtist(input)).toBe(expected)
  })

  it('leaves normal names untouched', () => {
    expect(cleanYoutubeArtist('Daft Punk')).toBe('Daft Punk')
    expect(cleanYoutubeArtist('Topic')).toBe('Topic')
  })
})

describe('cleanYoutubeMetadata', () => {
  it('splits "Artist - Title" when the artist is a channel name', () => {
    const r = cleanYoutubeMetadata('Daft Punk - One More Time (Official Video)', 'DaftPunkVEVO')
    expect(r).toEqual({ artist: 'Daft Punk', title: 'One More Time', changed: true })
  })

  it('splits when the artist is unknown', () => {
    const r = cleanYoutubeMetadata('Daft Punk - One More Time', 'Artista sconosciuto')
    expect(r.artist).toBe('Daft Punk')
    expect(r.title).toBe('One More Time')
  })

  it('splits when the left side matches the artist tag', () => {
    const r = cleanYoutubeMetadata('Daft Punk - One More Time', 'Daft Punk')
    expect(r.artist).toBe('Daft Punk')
    expect(r.title).toBe('One More Time')
  })

  it('does not split hyphenated titles when the artist tag is trustworthy', () => {
    const r = cleanYoutubeMetadata('Sweet Home - Acoustic Session', 'Radiohead')
    expect(r.artist).toBe('Radiohead')
    expect(r.title).toBe('Sweet Home - Acoustic Session')
  })

  it('cleans the artist suffix even without a split', () => {
    const r = cleanYoutubeMetadata('One More Time', 'Daft Punk - Topic')
    expect(r).toEqual({ artist: 'Daft Punk', title: 'One More Time', changed: true })
  })

  it('reports changed=false when nothing was dirty', () => {
    const r = cleanYoutubeMetadata('One More Time', 'Daft Punk')
    expect(r.changed).toBe(false)
  })
})
