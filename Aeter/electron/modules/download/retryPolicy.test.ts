import { describe, it, expect } from 'vitest'
import { DownloadError } from './errors'
import {
  decideRetry,
  MAX_ATTEMPTS,
  RETRY_BASE_MS,
  RATE_LIMIT_PAUSE_MS
} from './retryPolicy'

describe('decideRetry', () => {
  it('fails immediately on a permanent error, keeping the original message', () => {
    const d = decideRetry(new DownloadError('DL_UNRECOGNIZED_URL', 'permanent'), 0)
    expect(d).toEqual({
      action: 'fail',
      failureClass: 'permanent',
      errorMessage: 'DL_UNRECOGNIZED_URL'
    })
  })

  it('treats a non-DownloadError as permanent (unknown bugs must not retry-loop)', () => {
    const d = decideRetry(new Error('boom'), 0)
    expect(d.action).toBe('fail')
    expect(d.failureClass).toBe('permanent')
  })

  it('backs off exponentially on transient errors: 30s, 60s, 120s', () => {
    for (const [attempts, expected] of [
      [0, RETRY_BASE_MS],
      [1, RETRY_BASE_MS * 2],
      [2, RETRY_BASE_MS * 4]
    ] as const) {
      const d = decideRetry(new DownloadError('ETIMEDOUT', 'transient'), attempts)
      expect(d).toEqual({
        action: 'retry',
        failureClass: 'transient',
        attempts: attempts + 1,
        delayMs: expected,
        errorMessage: 'Errore di rete — nuovo tentativo automatico'
      })
    }
  })

  it('lands in error after MAX_ATTEMPTS transient failures', () => {
    const d = decideRetry(new DownloadError('ETIMEDOUT', 'transient'), MAX_ATTEMPTS)
    expect(d.action).toBe('fail')
    expect(d.failureClass).toBe('transient')
  })

  it('pauses at least 5 minutes on rate limits regardless of attempts', () => {
    const d = decideRetry(new DownloadError('HTTP 429', 'rate-limited'), 2)
    expect(d).toMatchObject({
      action: 'retry',
      failureClass: 'rate-limited',
      delayMs: RATE_LIMIT_PAUSE_MS,
      errorMessage: 'Rate limit raggiunto — riprovo tra qualche minuto'
    })
  })

  it('collapses a corrupted yt-dlp package to the YTDLP_CORRUPTED sentinel and retries it', () => {
    // Raw zipimport traceback → transient retry, traceback never persisted.
    const retry = decideRetry(new Error("zipimport.ZipImportError: bad local file header: '/data/yt-dlp'"), 0)
    expect(retry).toMatchObject({
      action: 'retry',
      failureClass: 'transient',
      errorMessage: 'YTDLP_CORRUPTED'
    })
    // Even at the attempts cap the persisted message stays the sentinel.
    const fail = decideRetry(new Error('zipimport: cannot open file'), MAX_ATTEMPTS)
    expect(fail).toEqual({
      action: 'fail',
      failureClass: 'transient',
      errorMessage: 'YTDLP_CORRUPTED'
    })
  })
})
