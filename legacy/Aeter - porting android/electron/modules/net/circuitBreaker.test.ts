import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CircuitBreaker } from './circuitBreaker'
import { CircuitOpenError, HttpError, NetworkError } from './errors'

const fail = (): Promise<never> => Promise.reject(new NetworkError('down'))
const ok = (): Promise<string> => Promise.resolve('ok')

describe('CircuitBreaker', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('stays closed under the failure threshold', async () => {
    const cb = new CircuitBreaker({ name: 'svc', failureThreshold: 3 })
    await expect(cb.exec(fail)).rejects.toBeInstanceOf(NetworkError)
    await expect(cb.exec(fail)).rejects.toBeInstanceOf(NetworkError)
    expect(cb.state).toBe('closed')
  })

  it('opens after N consecutive failures and fails fast', async () => {
    const cb = new CircuitBreaker({ name: 'svc', failureThreshold: 2, cooldownMs: 60_000 })
    await expect(cb.exec(fail)).rejects.toBeInstanceOf(NetworkError)
    await expect(cb.exec(fail)).rejects.toBeInstanceOf(NetworkError)
    expect(cb.state).toBe('open')
    const spy = vi.fn(ok)
    await expect(cb.exec(spy)).rejects.toBeInstanceOf(CircuitOpenError)
    expect(spy).not.toHaveBeenCalled()
  })

  it('success resets the consecutive-failure count', async () => {
    const cb = new CircuitBreaker({ name: 'svc', failureThreshold: 2 })
    await expect(cb.exec(fail)).rejects.toThrow()
    await expect(cb.exec(ok)).resolves.toBe('ok')
    await expect(cb.exec(fail)).rejects.toThrow()
    expect(cb.state).toBe('closed')
  })

  it('half-open probe after cooldown: success closes the circuit', async () => {
    const cb = new CircuitBreaker({ name: 'svc', failureThreshold: 1, cooldownMs: 1_000 })
    await expect(cb.exec(fail)).rejects.toThrow()
    expect(cb.state).toBe('open')
    vi.advanceTimersByTime(1_001)
    expect(cb.state).toBe('half-open')
    await expect(cb.exec(ok)).resolves.toBe('ok')
    expect(cb.state).toBe('closed')
  })

  it('half-open probe failure re-opens for another cooldown', async () => {
    const cb = new CircuitBreaker({ name: 'svc', failureThreshold: 1, cooldownMs: 1_000 })
    await expect(cb.exec(fail)).rejects.toThrow()
    vi.advanceTimersByTime(1_001)
    await expect(cb.exec(fail)).rejects.toBeInstanceOf(NetworkError)
    expect(cb.state).toBe('open')
  })

  it('half-open probe failing with a non-counting error re-arms the cooldown (no flood)', async () => {
    const cb = new CircuitBreaker({ name: 'svc', failureThreshold: 1, cooldownMs: 1_000 })
    await expect(cb.exec(fail)).rejects.toThrow()
    expect(cb.state).toBe('open')
    vi.advanceTimersByTime(1_001)
    expect(cb.state).toBe('half-open')
    // The single probe fails with a 4xx (does not count as a service failure).
    const notFound = (): Promise<never> => Promise.reject(new HttpError(404, 'http://x'))
    await expect(cb.exec(notFound)).rejects.toBeInstanceOf(HttpError)
    // Must go back to 'open' for another cooldown, not stay half-open letting
    // every subsequent call through as a fresh probe.
    expect(cb.state).toBe('open')
    const spy = vi.fn(ok)
    await expect(cb.exec(spy)).rejects.toBeInstanceOf(CircuitOpenError)
    expect(spy).not.toHaveBeenCalled()
  })

  it('4xx (except 429) does not trip the breaker', async () => {
    const cb = new CircuitBreaker({ name: 'svc', failureThreshold: 1 })
    const notFound = (): Promise<never> => Promise.reject(new HttpError(404, 'http://x'))
    await expect(cb.exec(notFound)).rejects.toBeInstanceOf(HttpError)
    await expect(cb.exec(notFound)).rejects.toBeInstanceOf(HttpError)
    expect(cb.state).toBe('closed')
  })

  it('5xx trips the breaker', async () => {
    const cb = new CircuitBreaker({ name: 'svc', failureThreshold: 1 })
    const boom = (): Promise<never> => Promise.reject(new HttpError(503, 'http://x'))
    await expect(cb.exec(boom)).rejects.toBeInstanceOf(HttpError)
    expect(cb.state).toBe('open')
  })
})
