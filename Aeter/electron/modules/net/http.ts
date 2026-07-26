import { ApiSchemaError, HttpError, NetworkError, RateLimitError } from './errors'

// App version injected at build time (define in the vite configs); the guard
// keeps test runners (no define) working with a sensible fallback.
const APP_VERSION =
  typeof __APP_VERSION__ !== 'undefined' && __APP_VERSION__ ? __APP_VERSION__ : '1.0.0'
export const DEFAULT_USER_AGENT = `Aether/${APP_VERSION} (https://github.com/aether-player)`

export interface FetchJsonOptions<T> {
  init?: RequestInit
  /** Default 15s via AbortSignal.timeout. */
  timeoutMs?: number
  /** zod-compatible structural type — keeps this module zod-free. */
  schema?: { parse(v: unknown): T }
  userAgent?: string
}

export function parseRetryAfter(header: string | null): number | null {
  if (!header) return null
  const seconds = Number(header)
  if (Number.isFinite(seconds)) return Math.max(0, seconds * 1000)
  const date = Date.parse(header)
  if (!Number.isNaN(date)) return Math.max(0, date - Date.now())
  return null
}

/**
 * Combine multiple AbortSignals into one that aborts when the first does. Used
 * instead of AbortSignal.any so this stays identical to the nodejs-mobile
 * (Node 12) port, which lacks it.
 */
function anySignal(signals: AbortSignal[]): AbortSignal {
  const controller = new AbortController()
  const cleanups: Array<() => void> = []
  const cleanup = (): void => {
    for (const c of cleanups) c()
  }
  for (const s of signals) {
    if (s.aborted) {
      controller.abort(s.reason)
      cleanup()
      break
    }
    const onAbort = (): void => {
      controller.abort(s.reason)
      cleanup()
    }
    s.addEventListener('abort', onAbort)
    cleanups.push(() => s.removeEventListener('abort', onAbort))
  }
  return controller.signal
}

export interface FetchWithTimeoutOptions {
  init?: RequestInit
  /** Default 15s; raise for long uploads/downloads. */
  timeoutMs?: number
  userAgent?: string
}

/**
 * fetch + timeout for callers that need the raw Response (binary bodies,
 * token endpoints, streaming). Status handling stays with the caller; throws
 * NetworkError on fetch/timeout failure so requests can never hang forever.
 */
export async function fetchWithTimeout(
  url: string,
  opts: FetchWithTimeoutOptions = {}
): Promise<Response> {
  const { init, timeoutMs = 15_000, userAgent = DEFAULT_USER_AGENT } = opts
  try {
    return await fetch(url, {
      ...init,
      headers: { 'User-Agent': userAgent, ...(init?.headers ?? {}) },
      // Always enforce the timeout, even when the caller supplies its own
      // signal (which may never fire on its own); combine rather than drop.
      signal: init?.signal
        ? anySignal([init.signal, AbortSignal.timeout(timeoutMs)])
        : AbortSignal.timeout(timeoutMs)
    })
  } catch (err) {
    const reason = err instanceof Error ? `${err.name}: ${err.message}` : String(err)
    throw new NetworkError(`Richiesta fallita verso ${url} (${reason})`, err)
  }
}

/**
 * fetch + timeout + status classification + optional schema parse.
 * Throws RateLimitError (429), HttpError (other !ok), NetworkError
 * (fetch/timeout failure) or ApiSchemaError (payload shape mismatch).
 */
export async function fetchJson<T = unknown>(
  url: string,
  opts: FetchJsonOptions<T> = {}
): Promise<T> {
  const { init, timeoutMs = 15_000, schema, userAgent = DEFAULT_USER_AGENT } = opts

  let res: Response
  try {
    res = await fetch(url, {
      ...init,
      headers: { 'User-Agent': userAgent, ...(init?.headers ?? {}) },
      // Always enforce the timeout, even when the caller supplies its own signal
      // (which may never fire on its own); combine the two rather than dropping it.
      signal: init?.signal
        ? anySignal([init.signal, AbortSignal.timeout(timeoutMs)])
        : AbortSignal.timeout(timeoutMs)
    })
  } catch (err) {
    // Keep the underlying reason visible (DNS/timeout/cert/missing-global) so
    // logs aren't opaque; the original error is still attached as `cause`.
    const reason = err instanceof Error ? `${err.name}: ${err.message}` : String(err)
    throw new NetworkError(`Richiesta fallita verso ${url} (${reason})`, err)
  }

  if (res.status === 429) {
    throw new RateLimitError(url, parseRetryAfter(res.headers.get('retry-after')))
  }
  if (!res.ok) {
    let body: string | undefined
    try {
      body = (await res.text()).slice(0, 500)
    } catch {
      /* body unavailable */
    }
    throw new HttpError(res.status, url, body)
  }

  let data: unknown
  try {
    data = await res.json()
  } catch (err) {
    throw new ApiSchemaError(url, err)
  }
  if (!schema) return data as T
  try {
    return schema.parse(data)
  } catch (err) {
    throw new ApiSchemaError(url, err)
  }
}
