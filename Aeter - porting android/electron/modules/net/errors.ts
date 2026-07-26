// Error taxonomy for outbound HTTP/network work.
// This module must stay free of electron/db imports (unit-tested in plain node).

export class NetworkError extends Error {
  readonly code = 'ENETWORK'
  constructor(
    message: string,
    readonly cause?: unknown
  ) {
    super(message)
    this.name = 'NetworkError'
  }
}

export class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly url: string,
    readonly body?: string
  ) {
    super(`HTTP ${status} for ${url}`)
    this.name = 'HttpError'
  }
}

export class RateLimitError extends HttpError {
  constructor(
    url: string,
    readonly retryAfterMs: number | null
  ) {
    super(429, url)
    this.name = 'RateLimitError'
  }
}

/** Schema/shape mismatch on an external API payload. Never retryable. */
export class ApiSchemaError extends Error {
  constructor(
    readonly url: string,
    readonly cause?: unknown
  ) {
    super(`Risposta API non valida da ${url}`)
    this.name = 'ApiSchemaError'
  }
}

export class CircuitOpenError extends Error {
  constructor(readonly service: string) {
    super(`Servizio ${service} momentaneamente non disponibile (circuito aperto)`)
    this.name = 'CircuitOpenError'
  }
}

/** Default classifier: network failures, HTTP 5xx and 429 are retryable. */
export function isRetryableError(err: unknown): boolean {
  if (err instanceof NetworkError) return true
  if (err instanceof RateLimitError) return true
  if (err instanceof HttpError) return err.status >= 500
  return false
}
