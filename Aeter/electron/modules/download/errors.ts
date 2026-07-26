// Error mapping and failure classification for downloads.
// Pure module, no electron imports (unit-tested). Returns stable error codes
// translated in the renderer (src/lib/ipcError.ts); unmapped tool output
// passes through verbatim and the renderer shows it as-is.

export function friendlyYtError(stderr: string): string {
  if (/Sign in to confirm your age|age.restricted/i.test(stderr)) return 'DL_AGE_RESTRICTED'
  if (/Video unavailable|has been removed/i.test(stderr)) return 'DL_UNAVAILABLE'
  if (/HTTP Error 429|rate.?limit/i.test(stderr)) return 'DL_RATE_LIMITED'
  if (/is not a valid URL|Unsupported URL/i.test(stderr)) return 'DL_INVALID_URL'
  if (/private video/i.test(stderr)) return 'DL_PRIVATE'
  const line = stderr.split('\n').find((l) => l.includes('ERROR'))
  return line?.replace(/^ERROR:?\s*/, '').trim() || 'DL_FAILED'
}

export function friendlySpotdlError(output: string, code: number | null): string {
  if (/LookupError|No results found/i.test(output)) return 'DL_NO_RESULTS'
  if (/429|rate.?limit/i.test(output)) return 'DL_RATE_LIMITED_RETRY'
  return `DL_SPOTDL_EXIT:${code}`
}

export type DownloadFailureClass = 'rate-limited' | 'transient' | 'permanent'

const RATE_LIMITED =
  /HTTP Error 429|rate.?limit|too many requests/i
const TRANSIENT =
  /unable to download webpage|temporary failure|getaddrinfo|ECONNRESET|ECONNREFUSED|ETIMEDOUT|EAI_AGAIN|timed? ?out|connection (?:error|reset|refused|aborted)|network (?:error|unreachable)|SSL|incomplete read|HTTP Error 5\d\d|service unavailable/i
const PERMANENT =
  /age.restricted|Sign in to confirm your age|Video unavailable|has been removed|private video|is not a valid URL|Unsupported URL|LookupError|No results found|DRM|copyright/i

/**
 * Classifies raw tool output (stderr/stdout) so the queue can decide between
 * automatic retry with backoff, a long rate-limit pause, or a terminal error.
 * Unknown errors default to 'permanent' (matches the previous fail-fast UX).
 */
export function classifyDownloadFailure(output: string): DownloadFailureClass {
  if (RATE_LIMITED.test(output)) return 'rate-limited'
  if (PERMANENT.test(output)) return 'permanent'
  if (TRANSIENT.test(output)) return 'transient'
  return 'permanent'
}

/** Thrown by source handlers: carries the Italian message plus the retry class. */
export class DownloadError extends Error {
  constructor(
    message: string,
    readonly failureClass: DownloadFailureClass
  ) {
    super(message)
    this.name = 'DownloadError'
  }
}
