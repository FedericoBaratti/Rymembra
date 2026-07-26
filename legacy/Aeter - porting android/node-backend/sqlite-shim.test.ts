import { describe, it, expect, beforeAll, afterAll, vi } from 'vitest'
import { mkdtempSync, rmdirSync, readdirSync, unlinkSync, existsSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'

// initSqlite() loads the wasm from `join(__dirname, 'sql-wasm.wasm')` — the
// nodejs-project layout on device. Under vitest __dirname is the source dir,
// so redirect just that one read to the sql.js package copy.
vi.mock('node:fs', async (importOriginal) => {
  const real = await importOriginal<typeof import('node:fs')>()
  // vi.mock factories are hoisted above the top-level imports, so resolve the
  // package copy in here without touching outer bindings.
  const { createRequire } = await import('node:module')
  const wasm = createRequire(import.meta.url).resolve('sql.js/dist/sql-wasm.wasm')
  const readFileSync = ((path: unknown, ...rest: unknown[]) => {
    const p = typeof path === 'string' && path.endsWith('sql-wasm.wasm') ? wasm : path
    return (real.readFileSync as (...a: unknown[]) => unknown)(p, ...rest)
  }) as typeof real.readFileSync
  return { ...real, readFileSync }
})

import Database, { initSqlite } from './sqlite-shim'

let dir: string
let db: InstanceType<typeof Database>

beforeAll(async () => {
  await initSqlite()
  dir = mkdtempSync(join(tmpdir(), 'sqlite-shim-test-'))
  db = new Database(join(dir, 'test.db'))
  db.exec('CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT)')
})

afterAll(() => {
  try {
    db.close()
  } catch {
    /* already closed by a test */
  }
  for (const f of readdirSync(dir)) unlinkSync(join(dir, f))
  rmdirSync(dir)
})

describe('sqlite-shim statement cache', () => {
  it('round-trips rows through the better-sqlite3 API surface', () => {
    const ins = db.prepare('INSERT INTO t (v) VALUES (?)')
    const r1 = ins.run('a')
    expect(r1.changes).toBe(1)
    expect(Number(r1.lastInsertRowid)).toBeGreaterThan(0)
    ins.run('b')
    const rows = db.prepare('SELECT v FROM t ORDER BY id').all() as { v: string }[]
    expect(rows.map((r) => r.v)).toEqual(['a', 'b'])
  })

  it('prepares each distinct SQL string once, however many Statement objects use it', () => {
    const spy = vi.spyOn(db.raw as { prepare: (sql: string) => unknown }, 'prepare')
    const sql = 'SELECT v FROM t WHERE id = ?'
    db.prepare(sql).get(1)
    db.prepare(sql).get(2)
    db.prepare(sql).all(1)
    expect(spy.mock.calls.filter(([s]) => s === sql)).toHaveLength(1)
    spy.mockRestore()
  })

  it('resets the cached statement between calls (no resumed cursor, no stale binds)', () => {
    const sel = db.prepare('SELECT v FROM t ORDER BY id')
    // get() steps once and stops early — the follow-up all() on the SAME
    // underlying cached statement must restart from row 1 and see everything.
    expect((sel.get() as { v: string }).v).toBe('a')
    expect((sel.all() as { v: string }[]).map((r) => r.v)).toEqual(['a', 'b'])
    expect((sel.get() as { v: string }).v).toBe('a')
  })

  it('binds named parameters with and without the @ prefix', () => {
    const byName = db.prepare('SELECT v FROM t WHERE v = @val')
    expect((byName.get({ val: 'b' }) as { v: string }).v).toBe('b')
    expect((byName.get({ '@val': 'a' }) as { v: string }).v).toBe('a')
  })

  it('survives a schema change: cached statements recompile transparently', () => {
    const sel = db.prepare('SELECT * FROM t WHERE id = 1')
    expect(sel.get()).toMatchObject({ v: 'a' })
    db.exec('ALTER TABLE t ADD COLUMN extra TEXT')
    // Same cached handle, new column visible → sqlite3_prepare_v2 re-prepared it.
    expect(sel.get()).toMatchObject({ v: 'a', extra: null })
  })

  it('close() flushes to disk and the file reopens with the data intact', () => {
    db.close()
    const file = join(dir, 'test.db')
    expect(existsSync(file)).toBe(true)
    const reopened = new Database(file)
    try {
      const rows = reopened.prepare('SELECT v FROM t ORDER BY id').all() as { v: string }[]
      expect(rows.map((r) => r.v)).toEqual(['a', 'b'])
    } finally {
      reopened.close()
    }
  })
})

// sql.js export() — the only way to serialize the DB, called on every flush —
// frees all prepared statements, drops create_function registrations and
// close+reopens the connection (PRAGMAs reset). The shim must survive that:
// a regression here bricked every DB call after the first flush on device
// ("Statement closed" — logcat 2026-07-17).
describe('sqlite-shim flush/export', () => {
  let fdb: InstanceType<typeof Database>

  beforeAll(() => {
    fdb = new Database(join(dir, 'flush.db'))
    fdb.exec('CREATE TABLE f (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT)')
    fdb.function('tf', (v: unknown) => String(v).toUpperCase())
  })

  afterAll(() => {
    fdb.close()
  })

  it('cached statements still work after flushNow()', () => {
    const ins = fdb.prepare('INSERT INTO f (v) VALUES (?)')
    const sel = fdb.prepare('SELECT v FROM f ORDER BY id')
    ins.run('a')
    expect((sel.all() as { v: string }[]).map((r) => r.v)).toEqual(['a'])
    fdb.flushNow()
    // Pre-fix these threw "Statement closed": the cache held freed handles.
    ins.run('b')
    expect((sel.all() as { v: string }[]).map((r) => r.v)).toEqual(['a', 'b'])
    expect((sel.get() as { v: string }).v).toBe('a')
  })

  it('custom SQL functions survive flushNow()', () => {
    fdb.prepare('INSERT INTO f (v) VALUES (?)').run('c')
    fdb.flushNow()
    const row = fdb.prepare("SELECT tf(v) AS r FROM f WHERE v = 'c'").get() as { r: string }
    expect(row.r).toBe('C')
  })

  it('foreign_keys stays ON across flushNow()', () => {
    fdb.prepare('INSERT INTO f (v) VALUES (?)').run('d')
    fdb.flushNow()
    expect(fdb.pragma('foreign_keys', { simple: true })).toBe(1)
  })

  it('backup() invalidates neither cached statements nor functions', async () => {
    const sel = fdb.prepare('SELECT count(*) AS n FROM f')
    const before = (sel.get() as { n: number }).n
    await fdb.backup(join(dir, 'flush-backup.db'))
    expect((sel.get() as { n: number }).n).toBe(before)
    const row = fdb.prepare("SELECT tf('x') AS r").get() as { r: string }
    expect(row.r).toBe('X')
  })

  it('the file written by flushNow() reopens with the data intact', () => {
    const reopened = new Database(join(dir, 'flush.db'))
    try {
      const rows = reopened.prepare('SELECT v FROM f ORDER BY id').all() as { v: string }[]
      expect(rows.map((r) => r.v)).toEqual(['a', 'b', 'c', 'd'])
    } finally {
      reopened.close()
    }
  })
})
