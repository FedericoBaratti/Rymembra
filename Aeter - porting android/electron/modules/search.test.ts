import { describe, it, expect } from 'vitest'
import { foldText } from '@shared/text'

/**
 * Validates the JS-side fold+filter strategy that search() uses as its
 * cross-engine fallback (the only search path on Android, where FTS5 is
 * unavailable). This deliberately does NOT use a custom SQL function (afold):
 * on nodejs-mobile the sql.js (V8 7.8) create_function callback dispatch is
 * unreliable and threw at call time, making search() reject and return nothing.
 * Folding+matching in plain JS is engine-independent, so the test exercises the
 * exact predicate the handler runs (electron/ipc/library.ipc.ts).
 */
interface Row {
  title: string
  artist: string
  album: string
}

const TRACKS: Row[] = [
  { title: 'Perché no', artist: 'Caparezza', album: 'Il Sogno Eretico' },
  { title: 'Another Brick in the Wall', artist: 'Pink Floyd', album: 'The Wall' },
  { title: 'Björk Song', artist: 'Björk', album: 'Debut' },
  // 141 Subsonica tracks, mirroring the user's library that returned nothing.
  ...Array.from({ length: 141 }, (_, i) => ({
    title: `Brano ${i + 1}`,
    artist: 'Subsonica',
    album: 'Microchip Emozionale'
  }))
]

function search(term: string): Row[] {
  const tokens = term.trim().split(/\s+/).map(foldText).filter(Boolean)
  if (tokens.length === 0) return []
  return TRACKS.filter((t) => {
    const hay = foldText(`${t.title} ${t.artist} ${t.album}`)
    return tokens.every((tk) => hay.includes(tk))
  })
}

describe('JS fold+filter search (Android fallback)', () => {
  it('finds the Subsonica tracks (the original bug report)', () => {
    const r = search('Subsonica')
    expect(r).toHaveLength(141)
    expect(r.every((t) => t.artist === 'Subsonica')).toBe(true)
  })

  it('matches accents insensitively', () => {
    expect(search('perche').map((r) => r.artist)).toContain('Caparezza')
    expect(search('bjork').map((r) => r.artist)).toContain('Björk')
  })

  it('requires every token (AND) across the concatenated fields', () => {
    expect(search('the wall').map((r) => r.title)).toContain('Another Brick in the Wall')
    expect(search('pink another')).toHaveLength(1)
    expect(search('the missing')).toHaveLength(0)
  })

  it('is case insensitive', () => {
    expect(search('CAPAREZZA')).toHaveLength(1)
  })
})
