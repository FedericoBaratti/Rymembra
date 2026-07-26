// SQLite-backed cache for external API responses (JSON payloads only —
// cover images go straight to cover_art, which dedups by SHA1).
// Negative entries (payload NULL) remember misses across sessions.

/**
 * Structural subset of better-sqlite3's Database, also satisfied by
 * node:sqlite DatabaseSync — lets unit tests avoid native bindings
 * compiled for Electron's ABI.
 */
export interface SqliteLike {
  prepare(sql: string): {
    get(...params: unknown[]): unknown
    run(...params: unknown[]): { changes: number | bigint }
  }
}

/** Mirrors the api_cache DDL of migration v3 — used by in-memory test DBs. */
export const API_CACHE_DDL = `
  CREATE TABLE IF NOT EXISTS api_cache (
    service TEXT NOT NULL,
    key TEXT NOT NULL,
    payload TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY (service, key)
  ) WITHOUT ROWID;
  CREATE INDEX IF NOT EXISTS idx_api_cache_expiry ON api_cache(expires_at);
`

export const TTL = {
  DAY: 24 * 60 * 60 * 1000,
  MB_RECORDING: 30 * 24 * 60 * 60 * 1000,
  MB_SEARCH: 7 * 24 * 60 * 60 * 1000,
  ACOUSTID: 30 * 24 * 60 * 60 * 1000,
  LASTFM: 30 * 24 * 60 * 60 * 1000,
  LRCLIB: 30 * 24 * 60 * 60 * 1000,
  NEGATIVE: 7 * 24 * 60 * 60 * 1000
} as const

export interface CachedJsonOptions<T> {
  service: string
  key: string
  ttlMs: number
  /** TTL for negative (null) results. Default ttlMs / 4. */
  missTtlMs?: number
  fetcher: () => Promise<T | null>
}

export interface ApiCache {
  /** Returns the cached value when fresh; caches null results as negative entries. */
  cachedJson<T>(opts: CachedJsonOptions<T>): Promise<T | null>
  /** True when a fresh entry (positive or negative) exists. Never hits the network. */
  has(service: string, key: string): boolean
  /** Writes an entry directly (null = negative entry). */
  set(service: string, key: string, value: unknown, ttlMs: number): void
  invalidate(service: string, key: string): void
  pruneExpired(): number
}

export function createApiCache(db: SqliteLike): ApiCache {
  const getStmt = db.prepare(
    'SELECT payload, expires_at FROM api_cache WHERE service = ? AND key = ?'
  )
  const putStmt = db.prepare(
    `INSERT INTO api_cache (service, key, payload, created_at, expires_at)
     VALUES (@service, @key, @payload, @now, @expiresAt)
     ON CONFLICT(service, key) DO UPDATE SET
       payload = excluded.payload, created_at = excluded.created_at, expires_at = excluded.expires_at`
  )
  const delStmt = db.prepare('DELETE FROM api_cache WHERE service = ? AND key = ?')
  const pruneStmt = db.prepare('DELETE FROM api_cache WHERE expires_at <= ?')

  return {
    async cachedJson<T>(opts: CachedJsonOptions<T>): Promise<T | null> {
      const now = Date.now()
      const row = getStmt.get(opts.service, opts.key) as
        | { payload: string | null; expires_at: number }
        | undefined
      if (row && row.expires_at > now) {
        if (row.payload === null) return null
        try {
          return JSON.parse(row.payload) as T
        } catch {
          delStmt.run(opts.service, opts.key)
        }
      }
      const value = await opts.fetcher()
      const ttl = value === null ? (opts.missTtlMs ?? opts.ttlMs / 4) : opts.ttlMs
      putStmt.run({
        service: opts.service,
        key: opts.key,
        payload: value === null ? null : JSON.stringify(value),
        now,
        expiresAt: now + ttl
      })
      return value
    },

    has(service: string, key: string): boolean {
      const row = getStmt.get(service, key) as { expires_at: number } | undefined
      return !!row && row.expires_at > Date.now()
    },

    set(service: string, key: string, value: unknown, ttlMs: number): void {
      const now = Date.now()
      putStmt.run({
        service,
        key,
        payload: value === null ? null : JSON.stringify(value),
        now,
        expiresAt: now + ttlMs
      })
    },

    invalidate(service: string, key: string): void {
      delStmt.run(service, key)
    },

    pruneExpired(): number {
      return Number(pruneStmt.run(Date.now()).changes)
    }
  }
}
