import { describe, it, expect } from 'vitest'
import { deriveKey, generateTotp, SECRET_CIPHERS } from './totp'

describe('spotify totp', () => {
  it('derives the HMAC key from the cipher via xor((i%33)+9) digits', () => {
    // cipher [10, 20] → [10 ^ 9, 20 ^ 10] = [3, 30] → "330" → utf8 bytes
    expect(deriveKey([10, 20]).toString('utf8')).toBe('330')
  })

  it('produces a stable 6-digit code for a fixed time window', () => {
    const code = generateTotp(SECRET_CIPHERS[0], 1_700_000_000_000)
    expect(code).toMatch(/^\d{6}$/)
    // same 30s window → identical code
    expect(generateTotp(SECRET_CIPHERS[0], 1_700_000_000_000 + 5_000)).toBe(code)
  })

  it('changes across different 30s windows', () => {
    const a = generateTotp(SECRET_CIPHERS[0], 1_700_000_000_000)
    const b = generateTotp(SECRET_CIPHERS[0], 1_700_000_000_000 + 60_000)
    // overwhelmingly likely to differ; guards against a frozen counter bug
    expect(a).not.toBe(b)
  })
})
