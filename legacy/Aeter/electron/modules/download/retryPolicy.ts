import { DownloadError, type DownloadFailureClass } from './errors'

// Pure failure-classification half of the downloader retry state machine,
// extracted from downloader.ts (which owns the DB writes and the timer) so it
// can be unit-tested without either. Permanent errors stop immediately;
// transient network errors back off exponentially (30s/60s/120s); rate limits
// pause at least 5 minutes; after MAX_ATTEMPTS the item lands in 'error'.

export const MAX_ATTEMPTS = 3
export const RETRY_BASE_MS = 30_000
export const RATE_LIMIT_PAUSE_MS = 300_000

export type RetryDecision =
  | { action: 'fail'; failureClass: DownloadFailureClass; errorMessage: string }
  | {
      action: 'retry'
      failureClass: DownloadFailureClass
      /** New attempts counter to persist (input attempts + 1). */
      attempts: number
      delayMs: number
      errorMessage: string
    }

/**
 * Classifies a download failure given the attempts already burned.
 *
 * A corrupted yt-dlp package (partial unzip, bad zip header, Python zipimport
 * traceback) is a transient environment fault, not a bad URL: retry it, and
 * never persist the noisy traceback — collapse it to the stable
 * YTDLP_CORRUPTED sentinel the UI knows how to translate.
 */
export function decideRetry(err: unknown, attempts: number): RetryDecision {
  const message = err instanceof Error ? err.message : String(err)
  const corrupted = /YTDLP_CORRUPTED|bad local file header|zipimport/i.test(message)
  const failureClass: DownloadFailureClass = corrupted
    ? 'transient'
    : err instanceof DownloadError
      ? err.failureClass
      : 'permanent'

  if (failureClass === 'permanent' || attempts >= MAX_ATTEMPTS) {
    return {
      action: 'fail',
      failureClass,
      errorMessage: corrupted ? 'YTDLP_CORRUPTED' : message
    }
  }

  return {
    action: 'retry',
    failureClass,
    attempts: attempts + 1,
    delayMs: failureClass === 'rate-limited' ? RATE_LIMIT_PAUSE_MS : RETRY_BASE_MS * Math.pow(2, attempts),
    errorMessage: corrupted
      ? 'YTDLP_CORRUPTED'
      : failureClass === 'rate-limited'
        ? 'Rate limit raggiunto — riprovo tra qualche minuto'
        : 'Errore di rete — nuovo tentativo automatico'
  }
}
