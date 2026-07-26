import { describe, expect, it } from 'vitest'
import { isAcceptableCoverRatio, sniffImageType, validateCoverBuffer } from './coverValidation'

/** Builds a buffer with a magic-byte header padded to `len` bytes. */
function img(header: number[], len: number): Buffer {
  const buf = Buffer.alloc(len, 0)
  for (let i = 0; i < header.length; i++) buf[i] = header[i]
  return buf
}

const JPEG = [0xff, 0xd8, 0xff, 0xe0]
const PNG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
const WEBP = [0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50]

describe('sniffImageType', () => {
  it('recognizes JPEG/PNG/WebP magic bytes', () => {
    expect(sniffImageType(img(JPEG, 64))).toBe(true)
    expect(sniffImageType(img(PNG, 64))).toBe(true)
    expect(sniffImageType(img(WEBP, 64))).toBe(true)
  })

  it('rejects non-image payloads (HTML/JSON error bodies)', () => {
    expect(sniffImageType(Buffer.from('<!DOCTYPE html><html>404</html>'))).toBe(false)
    expect(sniffImageType(Buffer.from('{"error":"not found"}'))).toBe(false)
  })

  it('rejects buffers too short to carry a header', () => {
    expect(sniffImageType(Buffer.from([0xff, 0xd8]))).toBe(false)
  })
})

describe('validateCoverBuffer', () => {
  it('accepts a real-sized image', () => {
    expect(validateCoverBuffer(img(JPEG, 4096))).toBe(true)
  })

  it('rejects tiny placeholders (1x1 / truncated)', () => {
    expect(validateCoverBuffer(img(PNG, 200))).toBe(false)
  })

  it('rejects valid-length non-images', () => {
    expect(validateCoverBuffer(Buffer.alloc(4096, 0x20))).toBe(false)
  })

  it('rejects empty/undefined buffers', () => {
    expect(validateCoverBuffer(Buffer.alloc(0))).toBe(false)
  })
})

describe('isAcceptableCoverRatio', () => {
  const cases: [number, number, boolean, string][] = [
    [500, 500, true, 'square'],
    [500, 460, true, 'slightly landscape'],
    [460, 500, true, 'slightly portrait'],
    [1450, 1000, true, 'exactly at the 1.45 bound'],
    [1000, 1450, true, 'exactly at the 1/1.45 bound'],
    [1280, 720, false, '16:9 video thumbnail'],
    [720, 1280, false, '9:16 vertical video'],
    [1500, 1000, false, 'just past the bound'],
    [1000, 500, false, '2:1 banner'],
    [0, 500, false, 'zero width'],
    [500, 0, false, 'zero height']
  ]
  for (const [w, h, ok, label] of cases) {
    it(`${ok ? 'accepts' : 'rejects'} ${w}x${h} (${label})`, () => {
      expect(isAcceptableCoverRatio(w, h)).toBe(ok)
    })
  }
})
