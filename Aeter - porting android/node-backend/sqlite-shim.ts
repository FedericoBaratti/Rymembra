/**
 * better-sqlite3-compatible adapter backed by sql.js (SQLite compiled to WASM).
 *
 * nodejs-mobile ships Node 12.19 (V8 7.8) and can't run the native better-sqlite3
 * addon, so the node-backend Vite build aliases `better-sqlite3` → this file.
 *
 * ⚠️ sql.js is PINNED to 1.12.0 in package.json. From 1.13.0 sql.js compiles its
 * wasm with `WASM_BIGINT` (i64 passed directly across the JS↔wasm boundary), which
 * V8 7.8 rejects at call time — "wasm function signature contains illegal type" —
 * so `new SQL.Database()` throws and the whole backend fails to boot. 1.12.0 is
 * the newest release that still legalizes i64 to i32 pairs. Do not bump it without
 * a Node-12 device test. sql.js is
 * pure WASM (no per-arch native build) and fully synchronous, so it matches
 * better-sqlite3's synchronous API surface used by the app: Database with
 * prepare/exec/pragma/transaction/close/backup and Statement with get/all/run.
 *
 * The DB lives in a file; sql.js is in-memory, so we load the file on open and
 * write it back (debounced) after mutations. FTS5 is absent from sql.js, so the
 * schema skips it on Android (see electron/modules/db.ts) and search() falls
 * back to LIKE.
 *
 * ⚠️ Durability on Android: the nodejs-mobile process is killed without warning
 * when the app is backgrounded, and there is no graceful-shutdown hook. So:
 *   - writes are ATOMIC (write to `<file>.tmp`, fsync via writeFileSync, then
 *     renameSync over the live file) and keep a `<file>.bak` of the last good
 *     state. A kill mid-write can no longer truncate/corrupt `aether.db`.
 *   - the load NEVER throws: a corrupt/unreadable file is set aside as
 *     `<file>.corrupt-<ts>`, we fall back to `<file>.bak`, and as a last resort
 *     start from an empty DB (the startup library scan repopulates it). A failed
 *     boot here previously left the backend without IPC handlers registered →
 *     the renderer hung "loading forever" with an empty library/settings.
 */
import initSqlJs from 'sql.js'
import { readFileSync, writeFileSync, existsSync, renameSync, copyFileSync } from 'node:fs'
import { join } from 'node:path'
import { isBackground } from '../electron/modules/appLifecycle'

// sql.js is loosely typed here; keep it pragmatic.
/* eslint-disable @typescript-eslint/no-explicit-any */
let SQL: any = null

/** Must be awaited once (in node-backend/main.ts boot) before any getDb(). */
export async function initSqlite(): Promise<void> {
  if (SQL) return
  // __dirname is the nodejs-project dir at runtime; sql-wasm.wasm is shipped there.
  const wasmBinary = readFileSync(join(__dirname, 'sql-wasm.wasm'))
  SQL = await (initSqlJs as unknown as (cfg: { wasmBinary: Buffer }) => Promise<any>)({ wasmBinary })
}

/**
 * Open a sql.js database from optional bytes and prove it is usable. The sanity
 * query forces sql.js to actually read the header/schema page, so a corrupt
 * `bytes` buffer throws HERE (where the caller can recover) rather than later
 * mid-query. Passing `undefined` creates a fresh empty database.
 */
function openDatabase(bytes?: Buffer): any {
  const db = new SQL.Database(bytes)
  // Touch the header + sqlite_master so a malformed image surfaces immediately.
  db.run('PRAGMA user_version')
  db.exec('SELECT count(*) FROM sqlite_master')
  db.run('PRAGMA foreign_keys = ON')
  return db
}

/**
 * Atomically replace `file` with `data`: write a sibling temp file, then rename
 * over the target (rename is atomic on the same filesystem). With `withBak`,
 * keep the previous good file as `<file>.bak` first (best effort). This mirrors
 * the tmp+rename pattern in electron/modules/jsonFile.ts and makes a kill
 * mid-write unable to corrupt the live DB.
 *
 * The .bak copy is NOT taken on every flush: tmp+rename already guarantees the
 * live file can never be truncated by a kill, so re-copying tens of MB before
 * each flush only guards against post-rename storage corruption — worth doing
 * occasionally (see shouldRefreshBak), not 12×/min during a scan burst.
 */
function atomicWrite(file: string, data: Buffer, withBak = false): void {
  if (withBak && existsSync(file)) {
    try {
      copyFileSync(file, `${file}.bak`)
    } catch {
      /* best effort: a missing .bak only weakens recovery, never breaks writes */
    }
  }
  const tmp = `${file}.tmp`
  writeFileSync(tmp, data)
  renameSync(tmp, file)
}

function normalizeParams(args: unknown[]): unknown[] | Record<string, unknown> {
  // better-sqlite3 uses positional spread: stmt.run(a, b, c) or named: stmt.run({a: 1}).
  // sql.js binds an array or object. Map undefined → null (sql.js rejects undefined).
  if (args.length === 1 && typeof args[0] === 'object' && args[0] !== null && !Buffer.isBuffer(args[0]) && !(args[0] instanceof Uint8Array)) {
    const obj = args[0] as Record<string, unknown>
    const normalized: Record<string, unknown> = {}
    for (const k of Object.keys(obj)) {
      const key = k.startsWith('@') || k.startsWith(':') || k.startsWith('$') ? k : `@${k}`
      normalized[key] = obj[k] === undefined ? null : obj[k]
    }
    return normalized
  }
  return args.map((a) => (a === undefined ? null : a))
}

function convertRow(row: Record<string, unknown>): Record<string, unknown> {
  // sql.js returns Uint8Array for BLOBs; the app expects Node Buffers.
  for (const k of Object.keys(row)) {
    const v = row[k]
    if (v instanceof Uint8Array) row[k] = Buffer.from(v)
  }
  return row
}

class Statement {
  constructor(
    private readonly parent: SqliteDatabase,
    private readonly sql: string
  ) {}

  // Every call reuses one cached sql.js statement per SQL string (bind() resets
  // it first; reset() in finally releases row locks). The shared modules cache
  // their better-sqlite3 Statement objects and call them in tight loops — with
  // the old prepare()+free() per call, each of those paid a full re-parse in
  // WASM, silently defeating every statement cache in the shared code.

  get(...params: unknown[]): unknown {
    const s = this.parent.cachedStmt(this.sql)
    try {
      s.bind(normalizeParams(params))
      return s.step() ? convertRow(s.getAsObject()) : undefined
    } finally {
      s.reset()
    }
  }

  all(...params: unknown[]): unknown[] {
    const s = this.parent.cachedStmt(this.sql)
    const rows: unknown[] = []
    try {
      s.bind(normalizeParams(params))
      while (s.step()) rows.push(convertRow(s.getAsObject()))
      return rows
    } finally {
      s.reset()
    }
  }

  run(...params: unknown[]): { changes: number; lastInsertRowid: number } {
    const s = this.parent.cachedStmt(this.sql)
    try {
      s.bind(normalizeParams(params))
      while (s.step()) {
        /* run to completion */
      }
    } finally {
      s.reset()
    }
    const changes = this.parent.raw.getRowsModified()
    const res = this.parent.raw.exec('SELECT last_insert_rowid()')
    const lastInsertRowid = (res[0]?.values?.[0]?.[0] as number) ?? 0
    this.parent.markDirty()
    return { changes, lastInsertRowid }
  }
}

class SqliteDatabase {
  raw: any
  private dirty = false
  private flushTimer: ReturnType<typeof setTimeout> | null = null
  // Forces a flush even under a continuous write burst (scan/enrichment), so the
  // debounce can't be starved indefinitely.
  private maxTimer: ReturnType<typeof setTimeout> | null = null
  private txnDepth = 0
  // .bak refresh policy: once per session, then at most every 30 min, plus an
  // explicit request before risky writes (schema migrations — see db.ts).
  private lastBakAt = 0
  private bakForcedNext = false

  constructor(private readonly file: string) {
    if (!SQL) throw new Error('sqlite not initialized — await initSqlite() first')
    this.raw = this.openWithRecovery()
  }

  /**
   * Open the DB file, recovering from a corrupt/unreadable image instead of
   * throwing (which would abort boot before IPC handlers register). Order:
   *   1. the live file, 2. the `.bak`, 3. a fresh empty DB.
   * A file that fails to open is preserved as `<file>.corrupt-<ts>` (renamed,
   * never overwritten) so nothing is silently destroyed and it can be inspected.
   */
  private openWithRecovery(): any {
    const tryOpen = (path: string, label: string): any | null => {
      if (!existsSync(path)) return null
      try {
        return openDatabase(readFileSync(path))
      } catch (err) {
        console.error(`[sqlite] ${label} unreadable, setting it aside`, err)
        try {
          renameSync(path, `${path}.corrupt-${Date.now()}`)
        } catch (renameErr) {
          console.error('[sqlite] could not move aside corrupt db', renameErr)
        }
        return null
      }
    }

    // 1. live file
    const fromMain = tryOpen(this.file, 'aether.db')
    if (fromMain) return fromMain

    // 2. last-good backup
    const fromBak = tryOpen(`${this.file}.bak`, 'aether.db.bak')
    if (fromBak) {
      console.error('[sqlite] recovered database from .bak')
      return fromBak
    }

    // 3. fresh empty DB — the startup library scan repopulates the library.
    console.error('[sqlite] starting from an empty database')
    return openDatabase(undefined)
  }

  // Prepared-statement cache (see Statement above). Keyed by SQL, small LRU:
  // the app uses a finite set of statements, but a runaway dynamic query must
  // not leak WASM statement handles forever. sqlite3_prepare_v2 recompiles a
  // cached statement transparently after schema changes (migrations).
  private readonly stmtCache = new Map<string, any>()
  private static readonly STMT_CACHE_MAX = 200

  cachedStmt(sql: string): any {
    let s = this.stmtCache.get(sql)
    if (s) {
      // refresh recency (Map iterates in insertion order)
      this.stmtCache.delete(sql)
      this.stmtCache.set(sql, s)
      return s
    }
    s = this.raw.prepare(sql)
    this.stmtCache.set(sql, s)
    if (this.stmtCache.size > SqliteDatabase.STMT_CACHE_MAX) {
      const oldest = this.stmtCache.keys().next().value as string
      const evicted = this.stmtCache.get(oldest)
      this.stmtCache.delete(oldest)
      try {
        evicted.free()
      } catch {
        /* already freed by sql.js */
      }
    }
    return s
  }

  private freeStatements(): void {
    for (const s of this.stmtCache.values()) {
      try {
        s.free()
      } catch {
        /* already freed by sql.js */
      }
    }
    this.stmtCache.clear()
  }

  prepare(sql: string): Statement {
    return new Statement(this, sql)
  }

  // Custom SQL functions, kept so exportImage() can re-register them: sql.js
  // export() drops every create_function registration along with the connection.
  private readonly customFns = new Map<string, (...args: any[]) => unknown>()

  /**
   * Register a scalar SQL function. Mirrors better-sqlite3's `db.function(name, fn)`
   * by delegating to sql.js `create_function`, so backend code (e.g. the `afold()`
   * used by search()) can call one API on both engines.
   */
  function(name: string, fn: (...args: any[]) => unknown): this {
    this.raw.create_function(name, fn)
    this.customFns.set(name, fn)
    return this
  }

  exec(sql: string): this {
    this.raw.exec(sql)
    this.markDirty()
    return this
  }

  pragma(source: string, options?: { simple?: boolean }): unknown {
    if (source.includes('=')) {
      this.raw.run(`PRAGMA ${source}`)
      this.markDirty()
      return undefined
    }
    const res = this.raw.exec(`PRAGMA ${source}`)
    const value = res[0]?.values?.[0]?.[0]
    if (options?.simple) return value
    const col = res[0]?.columns?.[0] ?? 'value'
    return value === undefined ? [] : [{ [col]: value }]
  }

  transaction<T extends (...args: any[]) => any>(fn: T): T {
    // The wrapper must stay a `function` (it forwards its dynamic `this` to
    // fn.apply, matching better-sqlite3), so the db reference needs an alias.
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this
    const wrapped = function (this: unknown, ...args: unknown[]): unknown {
      // Avoid nested BEGIN (SQLite errors); inner calls just run inline.
      if (self.txnDepth > 0) return fn.apply(this, args)
      self.txnDepth++
      self.raw.run('BEGIN')
      try {
        const result = fn.apply(this, args)
        self.raw.run('COMMIT')
        // Don't force a synchronous full-DB export on every commit: that turned
        // each recordPlay/rating/enrichment write into a disk write. Just mark
        // dirty and let the coalescing flush (or the onPause flush) persist it.
        self.markDirty()
        return result
      } catch (err) {
        try {
          self.raw.run('ROLLBACK')
        } catch {
          /* ignore */
        }
        throw err
      } finally {
        self.txnDepth--
      }
    }
    return wrapped as unknown as T
  }

  /**
   * Serialize the DB to bytes. sql.js export() frees every prepared statement,
   * drops every create_function registration and closes+reopens the connection
   * (resetting per-connection PRAGMAs) — so drop the statement cache first and
   * restore the connection state after. Statements re-prepare lazily on the
   * next cachedStmt(). Skipping this poisoned the cache with freed handles and
   * every DB call after the first flush threw "Statement closed".
   */
  private exportImage(): Buffer {
    this.freeStatements()
    const bytes = Buffer.from(this.raw.export())
    this.raw.run('PRAGMA foreign_keys = ON')
    for (const [name, fn] of this.customFns) this.raw.create_function(name, fn)
    return bytes
  }

  /** better-sqlite3 db.backup(path) returns a Promise; we just dump bytes. */
  async backup(destination: string): Promise<void> {
    atomicWrite(destination, this.exportImage())
  }

  close(): void {
    this.flushNow()
    this.freeStatements()
    this.raw.close()
  }

  markDirty(): void {
    this.dirty = true
    // Coalesce bursts: a quiet ~1.5s after the last write triggers a flush, but
    // a sustained write stream (scan/enrichment) is still flushed at least every
    // ~5s via maxTimer. While the app is BACKGROUNDED (screen off, process kept
    // alive by the media foreground service) each flush is a full-DB serialize
    // + write, so relax the cadence to 30s/60s — otherwise every recordPlay at
    // a track boundary rewrites tens of MB within 5s. Durability on a background
    // kill is covered by the onPause flush (MainActivity → window.aether.flushNow
    // → flushNow()); the residual exposure is ≤60s of play-stats on a hard kill.
    const bg = isBackground()
    if (!this.flushTimer) {
      this.flushTimer = setTimeout(() => this.flushNow(), bg ? 30_000 : 1500)
    }
    if (!this.maxTimer) {
      this.maxTimer = setTimeout(() => this.flushNow(), bg ? 60_000 : 5000)
    }
  }

  /** Ask the next flush to refresh `<file>.bak` regardless of the rate limit.
   *  Feature-detected by db.ts before schema migrations (desktop better-sqlite3
   *  doesn't have it). */
  forceBakOnNextWrite(): void {
    this.bakForcedNext = true
  }

  private shouldRefreshBak(): boolean {
    if (this.bakForcedNext) return true
    if (this.lastBakAt === 0) return true // first flush of the session
    return Date.now() - this.lastBakAt >= 30 * 60_000
  }

  /** Persist pending writes now. Public so the onPause lifecycle hook can call it. */
  flushNow(): void {
    if (this.flushTimer) {
      clearTimeout(this.flushTimer)
      this.flushTimer = null
    }
    if (this.maxTimer) {
      clearTimeout(this.maxTimer)
      this.maxTimer = null
    }
    if (this.dirty) {
      const withBak = this.shouldRefreshBak()
      atomicWrite(this.file, this.exportImage(), withBak)
      if (withBak) {
        this.lastBakAt = Date.now()
        this.bakForcedNext = false
      }
      this.dirty = false
    }
  }
}

// Mirror `import Database from 'better-sqlite3'`: default export is the ctor.
export default SqliteDatabase
