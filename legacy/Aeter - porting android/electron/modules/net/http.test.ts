import { afterEach, describe, expect, it, vi } from 'vitest'
import { parseRetryAfter, fetchWithTimeout } from './http'
import { NetworkError } from './errors'

describe('parseRetryAfter', () => {
  it('returns null for a missing header (not 0)', () => {
    // Regression: Number(null) === 0 used to fake a 0ms hint on a 429 with no
    // Retry-After header (see spotify.ts). It must be null instead.
    expect(parseRetryAfter(null)).toBeNull()
  })

  it('returns null for a non-numeric, non-date header', () => {
    expect(parseRetryAfter('soon')).toBeNull()
  })

  it('parses delta-seconds to milliseconds', () => {
    expect(parseRetryAfter('5')).toBe(5_000)
    expect(parseRetryAfter('0')).toBe(0)
  })

  it('parses an HTTP-date to a non-negative delay', () => {
    const future = new Date(Date.now() + 30_000).toUTCString()
    const ms = parseRetryAfter(future)
    expect(ms).not.toBeNull()
    expect(ms!).toBeGreaterThan(0)
    // A past date clamps to 0, never negative.
    expect(parseRetryAfter(new Date(Date.now() - 30_000).toUTCString())).toBe(0)
  })
})

// A fetch stub that never settles on its own and only rejects when its signal
// aborts — the "server accepts and goes silent" case the timeout must cover.
function hangingFetch(): typeof fetch {
  return (_url, init) =>
    new Promise((_resolve, reject) => {
      const signal = (init as RequestInit | undefined)?.signal
      signal?.addEventListener('abort', () => reject(signal.reason))
    })
}

describe('fetchWithTimeout', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('returns the raw Response on success and stamps the app User-Agent', async () => {
    const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) => new Response('ok'))
    vi.stubGlobal('fetch', fetchMock)
    const res = await fetchWithTimeout('https://example.test/x')
    expect(res.ok).toBe(true)
    const init = fetchMock.mock.calls[0][1]
    expect((init?.headers as Record<string, string>)['User-Agent']).toMatch(/^Aether\//)
  })

  it('aborts a hung request after timeoutMs with NetworkError instead of hanging forever', async () => {
    vi.stubGlobal('fetch', hangingFetch())
    await expect(
      fetchWithTimeout('https://example.test/slow', { timeoutMs: 25 })
    ).rejects.toBeInstanceOf(NetworkError)
  })

  it('combines (not replaces) a caller-supplied abort signal with the timeout', async () => {
    vi.stubGlobal('fetch', hangingFetch())
    const ctl = new AbortController()
    const pending = expect(
      fetchWithTimeout('https://example.test/slow', {
        timeoutMs: 60_000,
        init: { signal: ctl.signal }
      })
    ).rejects.toBeInstanceOf(NetworkError)
    ctl.abort()
    await pending
  })

  it('wraps a synchronous fetch failure in NetworkError with the url in the message', async () => {
    vi.stubGlobal('fetch', async () => {
      throw new TypeError('getaddrinfo ENOTFOUND example.test')
    })
    await expect(fetchWithTimeout('https://example.test/down')).rejects.toThrow(
      /example\.test\/down/
    )
  })
})
