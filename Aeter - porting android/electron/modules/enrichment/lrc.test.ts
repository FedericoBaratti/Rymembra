import { describe, expect, it } from 'vitest'
import { parseLrc } from './lrc'
import { formatLrcTime, serializeLrc } from '@shared/lrc'

describe('parseLrc', () => {
  it('parses simple timestamped lines', () => {
    const lrc = '[00:10.00]first\n[00:20.50]second\n[01:05.25]third'
    const lines = parseLrc(lrc)!
    expect(lines).toEqual([
      { time: 10, text: 'first' },
      { time: 20.5, text: 'second' },
      { time: 65.25, text: 'third' }
    ])
  })

  it('returns null for plain text (no timestamps)', () => {
    expect(parseLrc('just some\nplain lyrics\nwithout timing')).toBeNull()
  })

  it('returns null for fewer than 3 timed lines', () => {
    expect(parseLrc('[00:10.00]one\n[00:20.00]two')).toBeNull()
  })

  it('expands multiple timestamps on one line', () => {
    const lrc = '[00:10.00][00:30.00]chorus\n[00:20.00]verse\n[00:40.00]end'
    const lines = parseLrc(lrc)!
    expect(lines.map((l) => l.time)).toEqual([10, 20, 30, 40])
    expect(lines[0].text).toBe('chorus')
    expect(lines[2].text).toBe('chorus')
  })

  it('applies positive offset (lyrics appear earlier)', () => {
    const lrc = '[offset:+500]\n[00:10.00]a\n[00:20.00]b\n[00:30.00]c'
    const lines = parseLrc(lrc)!
    expect(lines.map((l) => l.time)).toEqual([9.5, 19.5, 29.5])
  })

  it('applies negative offset and clamps at zero', () => {
    const lrc = '[offset:-1000]\n[00:00.20]a\n[00:10.00]b\n[00:20.00]c'
    const lines = parseLrc(lrc)!
    expect(lines.map((l) => l.time)).toEqual([1.2, 11, 21])
  })

  it('skips metadata tags', () => {
    const lrc = '[ti:Title]\n[ar:Artist]\n[al:Album]\n[00:01.00]a\n[00:02.00]b\n[00:03.00]c'
    const lines = parseLrc(lrc)!
    expect(lines).toHaveLength(3)
    expect(lines[0].text).toBe('a')
  })

  it('sorts lines by time', () => {
    const lrc = '[00:30.00]late\n[00:10.00]early\n[00:20.00]middle'
    const lines = parseLrc(lrc)!
    expect(lines.map((l) => l.text)).toEqual(['early', 'middle', 'late'])
  })

  it('supports millisecond precision', () => {
    const lrc = '[00:10.123]a\n[00:20.456]b\n[00:30.789]c'
    const lines = parseLrc(lrc)!
    expect(lines[0].time).toBeCloseTo(10.123, 3)
  })

  it('handles CRLF line endings', () => {
    const lrc = '[00:10.00]a\r\n[00:20.00]b\r\n[00:30.00]c'
    expect(parseLrc(lrc)).toHaveLength(3)
  })
})

describe('formatLrcTime', () => {
  it('formats minutes and seconds with centisecond precision', () => {
    expect(formatLrcTime(0)).toBe('00:00.00')
    expect(formatLrcTime(75.3)).toBe('01:15.30')
    expect(formatLrcTime(605.456)).toBe('10:05.46')
  })

  it('clamps negative times to zero', () => {
    expect(formatLrcTime(-3)).toBe('00:00.00')
  })
})

describe('serializeLrc', () => {
  it('serializes lines sorted by time', () => {
    expect(
      serializeLrc([
        { time: 20, text: 'second' },
        { time: 10, text: 'first' }
      ])
    ).toBe('[00:10.00]first\n[00:20.00]second')
  })

  it('round-trips through parseLrc', () => {
    const lines = [
      { time: 9.5, text: 'one' },
      { time: 20.25, text: 'two' },
      { time: 65.75, text: 'three with: colon' }
    ]
    expect(parseLrc(serializeLrc(lines))).toEqual(lines)
  })

  it('round-trips sub-second precision to the centisecond', () => {
    const lines = [
      { time: 1.12, text: 'a' },
      { time: 2.34, text: 'b' },
      { time: 3.56, text: 'c' }
    ]
    const parsed = parseLrc(serializeLrc(lines))!
    parsed.forEach((l, i) => expect(l.time).toBeCloseTo(lines[i].time, 2))
  })
})
