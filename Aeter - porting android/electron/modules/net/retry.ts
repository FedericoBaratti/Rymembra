import { isRetryableError, RateLimitError } from './errors'

export interface RetryOptions {
  /** Extra attempts after the first. Default 3. */
  retries?: number
  baseDelayMs?: number
  maxDelayMs?: number
  factor?: number
  /** Full jitter: delay * random(0.5..1). Default true. */
  jitter?: boolean
  signal?: AbortSignal
  shouldRetry?: (err: unknown, attempt: number) => boolean
  onRetry?: (err: unknown, attempt: number, delayMs: number) => void
}

function delay(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(signal.reason instanceof Error ? signal.reason : new Error('Aborted'))
      return
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    function onAbort(): void {
      clearTimeout(timer)
      reject(signal?.reason instanceof Error ? signal.reason : new Error('Aborted'))
    }
    signal?.addEventListener('abort', onAbort, { once: true })
  })
}

/**
 * Runs `fn` with exponential backoff. A RateLimitError with a Retry-After hint
 * extends the wait to at least that hint (capped at maxDelayMs * 4 — 429s
 * deserve longer waits than 5xx).
 */
export async function withRetry<T>(
  fn: (attempt: number) => Promise<T>,
  opts: RetryOptions = {}
): Promise<T> {
  const {
    retries = 3,
    baseDelayMs = 500,
    maxDelayMs = 15_000,
    factor = 2,
    jitter = true,
    signal,
    shouldRetry = isRetryableError,
    onRetry
  } = opts

  let attempt = 0
  for (;;) {
    if (signal?.aborted) {
      throw signal.reason instanceof Error ? signal.reason : new Error('Aborted')
    }
    try {
      return await fn(attempt)
    } catch (err) {
      if (attempt >= retries || !shouldRetry(err, attempt)) throw err
      let wait = Math.min(baseDelayMs * Math.pow(factor, attempt), maxDelayMs)
      if (jitter) wait = wait * (0.5 + Math.random() * 0.5)
      if (err instanceof RateLimitError && err.retryAfterMs !== null) {
        wait = Math.min(Math.max(err.retryAfterMs, wait), maxDelayMs * 4)
      }
      onRetry?.(err, attempt, wait)
      await delay(wait, signal)
      attempt++
    }
  }
}
