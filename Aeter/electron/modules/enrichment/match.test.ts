import { describe, expect, it } from 'vitest'
import {
  cleanQueryText,
  normalizeForMatch,
  scoreCandidate,
  similarity,
  pickBestRecording
} from './match'
import type { MbRecording } from './schemas'

describe('normalizeForMatch', () => {
  it('lowercases and strips diacritics', () => {
    expect(normalizeForMatch('Beyoncé')).toBe('beyonce')
    expect(normalizeForMatch('Café Tacvba')).toBe('cafe tacvba')
  })

  it('removes feat. clauses in parentheses and trailing form', () => {
    expect(normalizeForMatch('Song Title (feat. Someone)')).toBe('song title')
    expect(normalizeForMatch('Song Title ft. Someone')).toBe('song title')
    expect(normalizeForMatch('Song Title [featuring A & B]')).toBe('song title')
  })

  it('removes remaster/live qualifiers', () => {
    expect(normalizeForMatch('Hotel California (2013 Remaster)')).toBe('hotel california')
    expect(normalizeForMatch('Money - 2011 Remastered')).toBe('money')
    expect(normalizeForMatch('Alive (Live)')).toBe('alive')
  })

  it('strips punctuation and collapses whitespace', () => {
    expect(normalizeForMatch("Don't  Stop -- Me, Now!")).toBe('don t stop me now')
  })

  it('keeps digits and non-Latin letters (no ICU \\p{} escapes)', () => {
    // Regression: \p{L}\p{N} threw on nodejs-mobile (small-ICU) and killed
    // auto-enrichment. The codepoint denylist must keep letters/digits intact
    // while still stripping ASCII/Latin-1/general punctuation.
    expect(normalizeForMatch('Blink 182')).toBe('blink 182')
    expect(normalizeForMatch('Кино')).toBe('кино') // Cyrillic letters kept
    expect(normalizeForMatch('A — B')).toBe('a b') // em dash (U+2014) stripped
    expect(normalizeForMatch('¡Hola! «Mundo»')).toBe('hola mundo')
  })
})

describe('similarity', () => {
  it('returns 1 for equal strings after normalization', () => {
    expect(similarity('Hotel California (2013 Remaster)', 'hotel california')).toBe(1)
  })

  it('is symmetric', () => {
    const a = 'Bohemian Rhapsody'
    const b = 'Bohemian Rapsody'
    expect(similarity(a, b)).toBeCloseTo(similarity(b, a), 10)
  })

  it('scores unrelated strings low', () => {
    expect(similarity('Bohemian Rhapsody', 'Smoke on the Water')).toBeLessThan(0.3)
  })

  it('scores near-identical strings high', () => {
    expect(similarity('Comfortably Numb', 'Comfortably Numb.')).toBeGreaterThan(0.9)
  })

  it('handles empty strings', () => {
    expect(similarity('', '')).toBe(0)
    expect(similarity('abc', '')).toBe(0)
  })
})

describe('cleanQueryText', () => {
  it('strips YouTube-style noise', () => {
    expect(cleanQueryText('Numb (Official Video)')).toBe('Numb')
    expect(cleanQueryText('Bohemian Rhapsody [HD]')).toBe('Bohemian Rhapsody')
    expect(cleanQueryText('Smells Like Teen Spirit (Official Music Video)')).toBe(
      'Smells Like Teen Spirit'
    )
    expect(cleanQueryText('Song Title - Lyric Video')).toBe('Song Title -')
  })

  it('drops trailing feat. and " - Topic"', () => {
    expect(cleanQueryText('Otherside feat. Someone')).toBe('Otherside')
    expect(cleanQueryText('Daft Punk - Topic')).toBe('Daft Punk')
  })

  it('keeps a clean title untouched and collapses whitespace', () => {
    expect(cleanQueryText('  Clean   Title  ')).toBe('Clean Title')
  })

  it('keeps non-Latin scripts and digits (ICU-free)', () => {
    expect(cleanQueryText('Кино 1987')).toBe('Кино 1987')
  })
})

describe('scoreCandidate', () => {
  const track = { title: 'Wish You Were Here', artist: 'Pink Floyd', duration: 334 }

  it('scores an exact match near 1', () => {
    const s = scoreCandidate(track, {
      title: 'Wish You Were Here',
      artist: 'Pink Floyd',
      durationMs: 334_000
    })
    expect(s).toBeGreaterThan(0.95)
  })

  it('scores an unrelated candidate below the threshold', () => {
    const s = scoreCandidate(track, {
      title: 'Smoke on the Water',
      artist: 'Deep Purple',
      durationMs: 200_000
    })
    expect(s).toBeLessThan(0.65)
  })

  it('treats missing duration as neutral', () => {
    const s = scoreCandidate(track, {
      title: 'Wish You Were Here',
      artist: 'Pink Floyd',
      durationMs: null
    })
    expect(s).toBeGreaterThan(0.8)
  })
})

function rec(partial: Partial<MbRecording> & { id: string; title: string }): MbRecording {
  return { ...partial }
}

describe('pickBestRecording', () => {
  const track = { title: 'Wish You Were Here', artist: 'Pink Floyd', duration: 334 }

  it('picks the candidate with matching duration over a mismatched one', () => {
    const good = rec({
      id: 'good',
      title: 'Wish You Were Here',
      length: 335_000,
      'artist-credit': [{ name: 'Pink Floyd' }]
    })
    const bad = rec({
      id: 'bad',
      title: 'Wish You Were Here',
      length: 180_000,
      'artist-credit': [{ name: 'Pink Floyd' }]
    })
    expect(pickBestRecording(track, [bad, good])?.id).toBe('good')
  })

  it('rejects matches below the 0.65 composite threshold', () => {
    const wrong = rec({
      id: 'wrong',
      title: 'Completely Different Song',
      length: 100_000,
      'artist-credit': [{ name: 'Another Band' }]
    })
    expect(pickBestRecording(track, [wrong])).toBeNull()
  })

  it('treats unknown artist as neutral instead of penalizing', () => {
    const unknownTrack = { title: 'Wish You Were Here', artist: 'Artista sconosciuto', duration: 334 }
    const candidate = rec({
      id: 'c',
      title: 'Wish You Were Here',
      length: 334_000,
      'artist-credit': [{ name: 'Pink Floyd' }]
    })
    expect(pickBestRecording(unknownTrack, [candidate])?.id).toBe('c')
  })

  it('treats missing recording length as neutral', () => {
    const candidate = rec({
      id: 'c',
      title: 'Wish You Were Here',
      'artist-credit': [{ name: 'Pink Floyd' }]
    })
    expect(pickBestRecording(track, [candidate])?.id).toBe('c')
  })

  it('skips candidates the MB search itself scored below 50', () => {
    const lowScore = rec({
      id: 'low',
      title: 'Wish You Were Here',
      score: 30,
      length: 334_000,
      'artist-credit': [{ name: 'Pink Floyd' }]
    })
    expect(pickBestRecording(track, [lowScore])).toBeNull()
  })

  it('returns null for an empty candidate list', () => {
    expect(pickBestRecording(track, [])).toBeNull()
  })

  it('matches titles that differ only by remaster qualifiers', () => {
    const candidate = rec({
      id: 'c',
      title: 'Wish You Were Here (2011 Remaster)',
      length: 334_000,
      'artist-credit': [{ name: 'Pink Floyd' }]
    })
    expect(pickBestRecording(track, [candidate])?.id).toBe('c')
  })
})
