import { CircuitOpenError, HttpError, NetworkError, RateLimitError } from './errors'

export type CircuitState = 'closed' | 'open' | 'half-open'

/** Failures that indicate the *service* is unhealthy (not the request). */
function countsAsFailure(err: unknown): boolean {
  if (err instanceof NetworkError) return true
  if (err instanceof RateLimitError) return true
  if (err instanceof HttpError) return err.status >= 500
  return false
}

export class CircuitBreaker {
  readonly name: string
  private readonly failureThreshold: number
  private readonly cooldownMs: number
  private failures = 0
  private openedAt = 0
  private halfOpenProbe = false

  constructor(opts: { name: string; failureThreshold?: number; cooldownMs?: number }) {
    this.name = opts.name
    this.failureThreshold = opts.failureThreshold ?? 5
    this.cooldownMs = opts.cooldownMs ?? 60_000
  }

  get state(): CircuitState {
    if (this.failures < this.failureThreshold) return 'closed'
    if (Date.now() - this.openedAt >= this.cooldownMs) return 'half-open'
    return 'open'
  }

  /**
   * Throws CircuitOpenError fast when open. Only network errors, 5xx and 429
   * count as failures; other 4xx and schema errors pass through untouched.
   */
  async exec<T>(fn: () => Promise<T>): Promise<T> {
    const state = this.state
    if (state === 'open') throw new CircuitOpenError(this.name)
    const wasHalfOpen = state === 'half-open'
    if (wasHalfOpen) {
      // Allow a single probe; everyone else fails fast until it resolves.
      if (this.halfOpenProbe) throw new CircuitOpenError(this.name)
      this.halfOpenProbe = true
    }
    try {
      const result = await fn()
      this.failures = 0
      return result
    } catch (err) {
      if (countsAsFailure(err)) {
        this.failures++
        if (this.failures >= this.failureThreshold) this.openedAt = Date.now()
      } else if (wasHalfOpen) {
        // The probe failed for a non-counting reason (4xx/schema). Don't leave
        // the breaker half-open — that would let every later call through as a
        // fresh probe. Re-arm the cooldown so we wait before probing again.
        this.openedAt = Date.now()
      }
      throw err
    } finally {
      this.halfOpenProbe = false
    }
  }
}
