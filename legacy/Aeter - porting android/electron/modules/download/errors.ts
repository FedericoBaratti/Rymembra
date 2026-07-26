// Error mapping and failure classification for downloads.
// Pure module, no electron imports (unit-tested). Returns stable error codes
// translated in the renderer (src/lib/ipcError.ts); unmapped tool output is
// wrapped in DL_YT_ERROR:<detail> so the renderer shows a translated label
// with the raw yt-dlp line as detail instead of bare English stderr.

export function friendlyYtError(stderr: string): string {
  // Corrupted yt-dlp package on Android (zipimport traceback): stable code,
  // never the raw Python traceback. The native side auto-repairs it.
  if (/bad local file header|zipimport/i.test(stderr)) return 'YTDLP_CORRUPTED'
  if (/Sign in to confirm your age|age.restricted/i.test(stderr)) return 'DL_AGE_RESTRICTED'
  if (/Video unavailable|has been removed/i.test(stderr)) return 'DL_UNAVAILABLE'
  if (/HTTP Error 429|rate.?limit|too many requests/i.test(stderr)) return 'DL_RATE_LIMITED'
  if (/HTTP Error 403|403:?\s*Forbidden/i.test(stderr)) return 'DL_FORBIDDEN'
  if (/is not a valid URL|Unsupported URL/i.test(stderr)) return 'DL_INVALID_URL'
  if (/private video/i.test(stderr)) return 'DL_PRIVATE'
  if (
    /getaddrinfo|ECONNRESET|ECONNREFUSED|ETIMEDOUT|EAI_AGAIN|network (?:error|unreachable)|unable to download webpage/i.test(
      stderr
    )
  ) {
    return 'DL_NETWORK'
  }
  const line = stderr.split('\n').find((l) => l.includes('ERROR'))
  const detail = line ? line.replace(/^ERROR:?\s*/, '').trim() : ''
  return detail ? `DL_YT_ERROR:${detail.slice(0, 200)}` : 'DL_FAILED'
}

export function friendlySpotdlError(output: string, code: number | null): string {
  if (/LookupError|No results found/i.test(output)) return 'DL_NO_RESULTS'
  if (/429|rate.?limit/i.test(output)) return 'DL_RATE_LIMITED_RETRY'
  return `DL_SPOTDL_EXIT:${code}`
}

export type DownloadFailureClass = 'rate-limited' | 'transient' | 'permanent'

const RATE_LIMITED =
  /HTTP Error 429|rate.?limit|too many requests/i
const PERMANENT =
  /age.restricted|Sign in to confirm your age|Video unavailable|has been removed|private video|is not a valid URL|Unsupported URL|LookupError|No results found|DRM|copyright/i

/**
 * Classifies raw tool output (stderr/stdout) so the queue can decide between
 * automatic retry with backoff, a long rate-limit pause, or a terminal error.
 * Unknown errors default to 'transient': YouTube keeps introducing new failure
 * strings (SABR, nsig, localized messages) that are usually recoverable, and
 * the queue's MAX_ATTEMPTS cap bounds the retries anyway. Only the known
 * PERMANENT patterns fail fast.
 */
export function classifyDownloadFailure(output: string): DownloadFailureClass {
  if (RATE_LIMITED.test(output)) return 'rate-limited'
  if (PERMANENT.test(output)) return 'permanent'
  return 'transient'
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
