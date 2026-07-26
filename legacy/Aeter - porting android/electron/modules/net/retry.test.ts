import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { withRetry } from './retry'
import { HttpError, NetworkError, RateLimitError } from './errors'

describe('withRetry', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('returns immediately on success without retrying', async () => {
    const fn = vi.fn().mockResolvedValue(42)
    await expect(withRetry(fn)).resolves.toBe(42)
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('retries retryable errors up to the limit then throws', async () => {
    const fn = vi.fn().mockRejectedValue(new NetworkError('down'))
    const promise = withRetry(fn, { retries: 3, baseDelayMs: 100, jitter: false })
    const caught = promise.catch((e) => e)
    await vi.runAllTimersAsync()
    expect(await caught).toBeInstanceOf(NetworkError)
    expect(fn).toHaveBeenCalledTimes(4) // 1 + 3 retries
  })

  it('does not retry non-retryable errors', async () => {
    const fn = vi.fn().mockRejectedValue(new HttpError(404, 'http://x'))
    await expect(withRetry(fn, { retries: 3 })).rejects.toBeInstanceOf(HttpError)
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('uses exponential backoff schedule', async () => {
    const fn = vi
      .fn()
      .mockRejectedValueOnce(new NetworkError('1'))
      .mockRejectedValueOnce(new NetworkError('2'))
      .mockResolvedValue('ok')
    const delays: number[] = []
    const promise = withRetry(fn, {
      retries: 3,
      baseDelayMs: 100,
      factor: 2,
      jitter: false,
      onRetry: (_e, _a, d) => delays.push(d)
    })
    await vi.runAllTimersAsync()
    await expect(promise).resolves.toBe('ok')
    expect(delays).toEqual([100, 200])
  })

  it('caps the delay at maxDelayMs', async () => {
    const fn = vi.fn().mockRejectedValueOnce(new NetworkError('1')).mockResolvedValue('ok')
    const delays: number[] = []
    const promise = withRetry(fn, {
      retries: 1,
      baseDelayMs: 50_000,
      maxDelayMs: 1_000,
      jitter: false,
      onRetry: (_e, _a, d) => delays.push(d)
    })
    await vi.runAllTimersAsync()
    await promise
    expect(delays).toEqual([1_000])
  })

  it('honors RateLimitError retryAfterMs when longer than backoff', async () => {
    const fn = vi
      .fn()
      .mockRejectedValueOnce(new RateLimitError('http://x', 5_000))
      .mockResolvedValue('ok')
    const delays: number[] = []
    const promise = withRetry(fn, {
      retries: 1,
      baseDelayMs: 100,
      jitter: false,
      onRetry: (_e, _a, d) => delays.push(d)
    })
    await vi.runAllTimersAsync()
    await promise
    expect(delays).toEqual([5_000])
  })

  it('jitter keeps delay within [0.5x, 1x] of the computed backoff', async () => {
    const fn = vi.fn().mockRejectedValueOnce(new NetworkError('1')).mockResolvedValue('ok')
    const delays: number[] = []
    const promise = withRetry(fn, {
      retries: 1,
      baseDelayMs: 1_000,
      jitter: true,
      onRetry: (_e, _a, d) => delays.push(d)
    })
    await vi.runAllTimersAsync()
    await promise
    expect(delays[0]).toBeGreaterThanOrEqual(500)
    expect(delays[0]).toBeLessThanOrEqual(1_000)
  })

  it('respects shouldRetry veto', async () => {
    const fn = vi.fn().mockRejectedValue(new NetworkError('down'))
    await expect(
      withRetry(fn, { retries: 5, shouldRetry: () => false })
    ).rejects.toBeInstanceOf(NetworkError)
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('aborts a pending delay via AbortSignal', async () => {
    const controller = new AbortController()
    const fn = vi.fn().mockRejectedValue(new NetworkError('down'))
    const promise = withRetry(fn, {
      retries: 3,
      baseDelayMs: 10_000,
      jitter: false,
      signal: controller.signal
    })
    const caught = promise.catch((e) => e)
    await vi.advanceTimersByTimeAsync(100)
    controller.abort(new Error('user cancelled'))
    const err = await caught
    expect((err as Error).message).toBe('user cancelled')
    expect(fn).toHaveBeenCalledTimes(1)
  })
})
