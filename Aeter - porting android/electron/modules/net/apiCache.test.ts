import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
// node:sqlite instead of better-sqlite3: the native module is compiled for
// Electron's ABI and cannot load under the system Node that runs vitest.
import { DatabaseSync } from 'node:sqlite'
import { API_CACHE_DDL, createApiCache, type ApiCache } from './apiCache'

describe('apiCache', () => {
  let db: DatabaseSync
  let cache: ApiCache

  beforeEach(() => {
    db = new DatabaseSync(':memory:')
    db.exec(API_CACHE_DDL)
    cache = createApiCache(db)
  })
  afterEach(() => {
    db.close()
    vi.useRealTimers()
  })

  it('calls the fetcher on a cold key and caches the value', async () => {
    const fetcher = vi.fn().mockResolvedValue({ a: 1 })
    const v1 = await cache.cachedJson({ service: 'mb', key: 'k', ttlMs: 1000, fetcher })
    const v2 = await cache.cachedJson({ service: 'mb', key: 'k', ttlMs: 1000, fetcher })
    expect(v1).toEqual({ a: 1 })
    expect(v2).toEqual({ a: 1 })
    expect(fetcher).toHaveBeenCalledTimes(1)
  })

  it('expires entries after the TTL', async () => {
    vi.useFakeTimers()
    const fetcher = vi.fn().mockResolvedValue('x')
    await cache.cachedJson({ service: 's', key: 'k', ttlMs: 1000, fetcher })
    vi.advanceTimersByTime(1001)
    await cache.cachedJson({ service: 's', key: 'k', ttlMs: 1000, fetcher })
    expect(fetcher).toHaveBeenCalledTimes(2)
  })

  it('caches null as a negative entry with missTtlMs', async () => {
    vi.useFakeTimers()
    const fetcher = vi.fn().mockResolvedValue(null)
    const v1 = await cache.cachedJson({
      service: 's',
      key: 'k',
      ttlMs: 10_000,
      missTtlMs: 500,
      fetcher
    })
    expect(v1).toBeNull()
    // within miss TTL: no refetch
    const v2 = await cache.cachedJson({ service: 's', key: 'k', ttlMs: 10_000, missTtlMs: 500, fetcher })
    expect(v2).toBeNull()
    expect(fetcher).toHaveBeenCalledTimes(1)
    // after miss TTL: refetch
    vi.advanceTimersByTime(501)
    await cache.cachedJson({ service: 's', key: 'k', ttlMs: 10_000, missTtlMs: 500, fetcher })
    expect(fetcher).toHaveBeenCalledTimes(2)
  })

  it('has() reports fresh entries without fetching', async () => {
    expect(cache.has('s', 'k')).toBe(false)
    await cache.cachedJson({ service: 's', key: 'k', ttlMs: 1000, fetcher: async () => 1 })
    expect(cache.has('s', 'k')).toBe(true)
  })

  it('invalidate() removes an entry', async () => {
    const fetcher = vi.fn().mockResolvedValue(1)
    await cache.cachedJson({ service: 's', key: 'k', ttlMs: 1000, fetcher })
    cache.invalidate('s', 'k')
    await cache.cachedJson({ service: 's', key: 'k', ttlMs: 1000, fetcher })
    expect(fetcher).toHaveBeenCalledTimes(2)
  })

  it('pruneExpired() removes only expired rows', async () => {
    vi.useFakeTimers()
    await cache.cachedJson({ service: 's', key: 'old', ttlMs: 100, fetcher: async () => 1 })
    await cache.cachedJson({ service: 's', key: 'fresh', ttlMs: 60_000, fetcher: async () => 2 })
    vi.advanceTimersByTime(200)
    expect(cache.pruneExpired()).toBe(1)
    expect(cache.has('s', 'fresh')).toBe(true)
    expect(cache.has('s', 'old')).toBe(false)
  })

  it('keys are namespaced by service', async () => {
    const f1 = vi.fn().mockResolvedValue('a')
    const f2 = vi.fn().mockResolvedValue('b')
    const v1 = await cache.cachedJson({ service: 's1', key: 'k', ttlMs: 1000, fetcher: f1 })
    const v2 = await cache.cachedJson({ service: 's2', key: 'k', ttlMs: 1000, fetcher: f2 })
    expect(v1).toBe('a')
    expect(v2).toBe('b')
  })
})
