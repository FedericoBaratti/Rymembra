import { describe, it, expect, beforeEach } from 'vitest'
import {
  pairRateLimited,
  resetPairRateLimit,
  PAIR_MAX_ATTEMPTS,
  PAIR_WINDOW_MS
} from './pairRateLimit'

const T0 = 1_000_000

beforeEach(() => {
  resetPairRateLimit()
})

describe('pairRateLimited', () => {
  it('allows PAIR_MAX_ATTEMPTS attempts, then limits the next one', () => {
    for (let i = 0; i < PAIR_MAX_ATTEMPTS; i++) {
      expect(pairRateLimited('10.0.0.1', T0 + i)).toBe(false)
    }
    expect(pairRateLimited('10.0.0.1', T0 + PAIR_MAX_ATTEMPTS)).toBe(true)
  })

  it('tracks each source IP independently', () => {
    for (let i = 0; i < PAIR_MAX_ATTEMPTS; i++) pairRateLimited('10.0.0.1', T0)
    expect(pairRateLimited('10.0.0.1', T0)).toBe(true)
    expect(pairRateLimited('10.0.0.2', T0)).toBe(false)
  })

  it('recovers as soon as the sliding window moves past the burst', () => {
    for (let i = 0; i < PAIR_MAX_ATTEMPTS; i++) pairRateLimited('10.0.0.1', T0)
    expect(pairRateLimited('10.0.0.1', T0 + 1)).toBe(true)
    // A limited call must not extend the lockout: once the original burst
    // ages out, the client is allowed again.
    expect(pairRateLimited('10.0.0.1', T0 + PAIR_WINDOW_MS)).toBe(false)
  })
})
