import { describe, it, expect } from 'vitest'
import { buildQuery, pickBestCandidate, sanitizeSegment, type Candidate } from './spotifyMatch'

const cand = (url: string, durationSec: number | null): Candidate => ({ url, durationSec, title: url })

describe('sanitizeSegment', () => {
  it('replaces filesystem-illegal characters with spaces', () => {
    expect(sanitizeSegment('AC/DC: Back?')).toBe('AC DC Back')
  })
  it('collapses whitespace and trims', () => {
    expect(sanitizeSegment('  a   b  ')).toBe('a b')
  })
  it('falls back to a placeholder for empty input', () => {
    expect(sanitizeSegment('///')).toBe('Senza titolo')
  })
  it('caps very long names', () => {
    expect(sanitizeSegment('x'.repeat(200)).length).toBe(120)
  })
})

describe('buildQuery', () => {
  it('joins artist and title', () => {
    expect(buildQuery({ title: 'Hey', artist: 'Pixies', album: null, durationMs: null })).toBe('Pixies Hey')
  })
  it('works without an artist', () => {
    expect(buildQuery({ title: 'Hey', artist: null, album: null, durationMs: null })).toBe('Hey')
  })
})

describe('pickBestCandidate', () => {
  it('returns null for no candidates', () => {
    expect(pickBestCandidate([], 200000)).toBeNull()
  })
  it('picks the closest duration within tolerance', () => {
    const list = [cand('a', 120), cand('b', 205), cand('c', 260)]
    expect(pickBestCandidate(list, 200000)?.url).toBe('b') // 205s vs target 200s
  })
  it('falls back to the first hit when nothing is within tolerance', () => {
    const list = [cand('a', 400), cand('b', 500)]
    expect(pickBestCandidate(list, 200000)?.url).toBe('a')
  })
  it('returns the first candidate when the target duration is unknown', () => {
    const list = [cand('a', 120), cand('b', 200)]
    expect(pickBestCandidate(list, null)?.url).toBe('a')
  })
  it('ignores candidates with unknown duration when scoring', () => {
    const list = [cand('a', null), cand('b', 198)]
    expect(pickBestCandidate(list, 200000)?.url).toBe('b')
  })
})
