import { describe, expect, it } from 'vitest'
import { friendlyYtError, friendlySpotdlError, classifyDownloadFailure } from './errors'

// These tests regression-pin the stable error codes translated in the renderer.
describe('friendlyYtError', () => {
  it('maps age-restricted videos', () => {
    expect(friendlyYtError('ERROR: Sign in to confirm your age')).toBe('DL_AGE_RESTRICTED')
  })

  it('maps unavailable/removed videos', () => {
    expect(friendlyYtError('ERROR: Video unavailable')).toBe('DL_UNAVAILABLE')
  })

  it('maps rate limits', () => {
    expect(friendlyYtError('ERROR: HTTP Error 429: Too Many Requests')).toBe('DL_RATE_LIMITED')
  })

  it('maps invalid URLs', () => {
    expect(friendlyYtError('ERROR: "abc" is not a valid URL')).toBe('DL_INVALID_URL')
  })

  it('maps private videos', () => {
    expect(friendlyYtError('ERROR: private video')).toBe('DL_PRIVATE')
  })

  it('maps 403/Forbidden', () => {
    expect(friendlyYtError('ERROR: unable to download video data: HTTP Error 403: Forbidden')).toBe(
      'DL_FORBIDDEN'
    )
  })

  it('maps network failures', () => {
    expect(friendlyYtError('ERROR: unable to download webpage')).toBe('DL_NETWORK')
    expect(friendlyYtError('getaddrinfo ENOTFOUND youtube.com')).toBe('DL_NETWORK')
  })

  it('maps a corrupted yt-dlp package (zipimport traceback) to the stable code', () => {
    const traceback =
      'Traceback (most recent call last):\n' +
      '  File "<frozen runpy>", line 198, in _run_module_as_main\n' +
      '  File "<frozen zipimport>", line 538, in _get_data\n' +
      "zipimport.ZipImportError: bad local file header: '/data/user/0/com.aether.player/no_backup/youtubedl-android/yt-dlp/yt-dlp'"
    expect(friendlyYtError(traceback)).toBe('YTDLP_CORRUPTED')
    expect(classifyDownloadFailure(traceback)).toBe('transient')
  })

  it('wraps the raw ERROR line in a translatable param code as fallback', () => {
    expect(friendlyYtError('warning: x\nERROR: Something specific went wrong')).toBe(
      'DL_YT_ERROR:Something specific went wrong'
    )
  })

  it('falls back to a generic code', () => {
    expect(friendlyYtError('')).toBe('DL_FAILED')
  })
})

describe('friendlySpotdlError', () => {
  it('maps lookup errors', () => {
    expect(friendlySpotdlError('LookupError: No results found for song', 1)).toBe('DL_NO_RESULTS')
  })

  it('maps rate limits', () => {
    expect(friendlySpotdlError('HTTP 429 rate limit exceeded', 1)).toBe('DL_RATE_LIMITED_RETRY')
  })

  it('falls back to the exit code', () => {
    expect(friendlySpotdlError('boom', 2)).toBe('DL_SPOTDL_EXIT:2')
  })
})

describe('classifyDownloadFailure', () => {
  it('classifies 429 and rate limits', () => {
    expect(classifyDownloadFailure('ERROR: HTTP Error 429: Too Many Requests')).toBe('rate-limited')
    expect(classifyDownloadFailure('rate-limit reached')).toBe('rate-limited')
  })

  it('classifies network errors as transient', () => {
    expect(classifyDownloadFailure('ERROR: unable to download webpage')).toBe('transient')
    expect(classifyDownloadFailure('getaddrinfo ENOTFOUND youtube.com')).toBe('transient')
    expect(classifyDownloadFailure('read ECONNRESET')).toBe('transient')
    expect(classifyDownloadFailure('Connection timed out')).toBe('transient')
    expect(classifyDownloadFailure('ERROR: HTTP Error 503: Service Unavailable')).toBe('transient')
  })

  it('classifies YouTube 403/signature failures as transient (retryable)', () => {
    expect(
      classifyDownloadFailure('ERROR: unable to download video data: HTTP Error 403: Forbidden')
    ).toBe('transient')
    expect(classifyDownloadFailure('ERROR: 403: Forbidden')).toBe('transient')
    expect(classifyDownloadFailure('WARNING: [youtube] nsig extraction failed')).toBe('transient')
  })

  it('classifies content errors as permanent', () => {
    expect(classifyDownloadFailure('ERROR: Video unavailable')).toBe('permanent')
    expect(classifyDownloadFailure('ERROR: private video')).toBe('permanent')
    expect(classifyDownloadFailure('ERROR: Sign in to confirm your age')).toBe('permanent')
    expect(classifyDownloadFailure('"x" is not a valid URL')).toBe('permanent')
    expect(classifyDownloadFailure('LookupError: No results found')).toBe('permanent')
  })

  it('defaults unknown errors to transient (retry, bounded by the queue cap)', () => {
    expect(classifyDownloadFailure('some inexplicable error')).toBe('transient')
  })

  it('prefers rate-limited over other matches', () => {
    expect(classifyDownloadFailure('HTTP Error 429 while fetching video unavailable page')).toBe(
      'rate-limited'
    )
  })
})
