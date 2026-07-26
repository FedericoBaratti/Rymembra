import { describe, expect, it } from 'vitest'
import { resolveRange, mimeForPath } from './httpRange'

describe('resolveRange', () => {
  const SIZE = 1000

  it('returns a full 200 response when no Range header is present', () => {
    const r = resolveRange(null, SIZE)
    expect(r).toEqual({ status: 200, start: 0, end: 999, headers: { 'Content-Length': '1000' } })
  })

  it('returns a full 200 response for an unparseable Range header', () => {
    const r = resolveRange('not-a-range', SIZE)
    expect(r.status).toBe(200)
  })

  it('resolves a bounded range bytes=START-END', () => {
    const r = resolveRange('bytes=100-199', SIZE)
    expect(r).toEqual({
      status: 206,
      start: 100,
      end: 199,
      headers: { 'Content-Range': 'bytes 100-199/1000', 'Content-Length': '100' }
    })
  })

  it('resolves an open-ended range bytes=START-', () => {
    const r = resolveRange('bytes=900-', SIZE)
    expect(r).toEqual({
      status: 206,
      start: 900,
      end: 999,
      headers: { 'Content-Range': 'bytes 900-999/1000', 'Content-Length': '100' }
    })
  })

  it('clamps an end beyond the file size', () => {
    const r = resolveRange('bytes=900-5000', SIZE)
    if (r.status !== 206) throw new Error('expected 206')
    expect(r.end).toBe(999)
  })

  it('resolves a suffix range bytes=-N to the last N bytes', () => {
    const r = resolveRange('bytes=-100', SIZE)
    expect(r).toEqual({
      status: 206,
      start: 900,
      end: 999,
      headers: { 'Content-Range': 'bytes 900-999/1000', 'Content-Length': '100' }
    })
  })

  it('clamps a suffix range larger than the file to the whole file', () => {
    const r = resolveRange('bytes=-5000', SIZE)
    if (r.status !== 206) throw new Error('expected 206')
    expect(r.start).toBe(0)
    expect(r.end).toBe(999)
  })

  it('returns 416 for a start beyond the file size', () => {
    const r = resolveRange('bytes=5000-', SIZE)
    expect(r).toEqual({ status: 416, headers: { 'Content-Range': 'bytes */1000' } })
  })

  it('returns 416 for an empty bytes=- range', () => {
    const r = resolveRange('bytes=-', SIZE)
    expect(r.status).toBe(416)
  })
})

describe('mimeForPath', () => {
  it('maps known audio extensions', () => {
    expect(mimeForPath('/x/song.mp3')).toBe('audio/mpeg')
    expect(mimeForPath('/x/song.flac')).toBe('audio/flac')
    expect(mimeForPath('/x/SONG.OPUS')).toBe('audio/ogg')
  })

  it('falls back to application/octet-stream for unknown extensions', () => {
    expect(mimeForPath('/x/song.xyz')).toBe('application/octet-stream')
  })
})
