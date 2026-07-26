import { describe, it, expect } from 'vitest'
import { pickCanonical } from './albumRepair'

// pickCanonical receives the track rows of one (album, cover) group and must
// choose a single album artist to merge them under.
function row(artist: string | null, album_artist: string | null = null) {
  return { id: 0, path: '', artist, album_artist }
}

describe('albumRepair.pickCanonical', () => {
  it('picks the most frequent effective artist (feat. split → base artist)', () => {
    const group = [
      row('Michael Jackson'),
      row('Michael Jackson'),
      row('Michael Jackson, Paul McCartney'),
      row('Michael Jackson')
    ]
    expect(pickCanonical(group)).toBe('Michael Jackson')
  })

  it('breaks frequency ties by the shorter value (drops the guest credit)', () => {
    const group = [row('Daft Punk, Pharrell Williams'), row('Daft Punk')]
    expect(pickCanonical(group)).toBe('Daft Punk')
  })

  it('honours album_artist over the per-track artist when present', () => {
    const group = [
      row('Guest A', 'Various Artists'),
      row('Guest B', 'Various Artists'),
      row('Guest C', 'Various Artists')
    ]
    expect(pickCanonical(group)).toBe('Various Artists')
  })

  it('ignores empty/blank artists', () => {
    const group = [row(''), row('Radiohead'), row(null)]
    expect(pickCanonical(group)).toBe('Radiohead')
  })
})
