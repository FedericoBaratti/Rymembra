// Schema parity guard between the desktop and Android trees.
//
// The two migration HISTORIES diverged long ago (same user_version ≠ same
// schema — desktop v7 is sync, Android v7 is spotify-migration, …), but both
// chains MUST land on the same final schema or Drive sync between the two
// devices drifts silently. This test replays the local chain on an in-memory
// DB and compares the normalized result against db.schema.expected.json — a
// fixture committed IDENTICAL in both trees. A migration added to one tree
// only either fails here immediately (fixture no longer matches) or fails the
// twin tree the moment the fixture is regenerated — divergence can't ship.
//
// Regenerating the fixture (after adding MATCHING migrations to both trees):
//   DB_SCHEMA_DUMP=/tmp/schema.json npx vitest run electron/modules/db.schema.test.ts
// in each tree, diff the two dumps, then commit the agreed file to BOTH trees.
//
// Normalization notes: columns are sorted by name (the trees ALTERed them in
// different orders — a non-difference for name-based access) and the FTS5
// shadow tables are collapsed under their virtual-table definition. On the
// Android DEVICE tracks_fts is skipped at runtime (sql.js has no FTS5); that
// justified runtime divergence is outside this fixture, which captures the
// chain as replayed on the dev platform in both trees.
import { describe, it, expect, vi } from 'vitest'
import { DatabaseSync } from 'node:sqlite'
import type Database from 'better-sqlite3'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

// db.ts imports electron only for the userData path (getDb/coverPaths/logger).
// The chain runs here on an EMPTY in-memory DB, so the filesystem-touching
// steps (v9 cover extraction, warn logging) never execute and a dead path is
// enough — repo convention, see pairingStore.test.ts.
vi.mock('electron', () => ({
  app: { getPath: () => 'db-schema-test-userdata-does-not-exist' }
}))

import { MIGRATIONS } from './db'

interface NormalizedSchema {
  tables: Record<
    string,
    {
      columns: { name: string; type: string; notNull: boolean; default: string | null; pk: number }[]
      withoutRowid: boolean
    }
  >
  virtualTables: Record<string, string>
  indexes: Record<string, { table: string; unique: boolean; origin: string; columns: string[] }>
  triggers: Record<string, string>
}

interface ColumnRow {
  name: string
  type: string
  notnull: number
  dflt_value: unknown
  pk: number
}
interface IndexListRow {
  name: string
  unique: number
  origin: string
}
interface IndexInfoRow {
  seqno: number
  name: string | null
}

const flat = (sql: string): string => sql.replace(/\s+/g, ' ').trim()

function normalizeSchema(d: DatabaseSync): NormalizedSchema {
  // ORDER BY name keeps object key insertion (→ the JSON dump) deterministic.
  const master = d
    .prepare("SELECT name, type, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name")
    .all() as unknown as { name: string; type: string; sql: string | null }[]

  const virtualTables: Record<string, string> = {}
  for (const m of master) {
    if (m.type === 'table' && m.sql && /^\s*CREATE\s+VIRTUAL\s+TABLE/i.test(m.sql)) {
      virtualTables[m.name] = flat(m.sql)
    }
  }
  // FTS5 shadow tables ("tracks_fts_data", …) are an implementation detail of
  // the virtual table already captured above.
  const isShadow = (name: string): boolean =>
    Object.keys(virtualTables).some((v) => name.startsWith(`${v}_`))

  const tables: NormalizedSchema['tables'] = {}
  for (const m of master) {
    if (m.type !== 'table' || m.name in virtualTables || isShadow(m.name)) continue
    const columns = (
      d.prepare('SELECT name, type, "notnull", dflt_value, pk FROM pragma_table_info(?)').all(m.name) as unknown as ColumnRow[]
    )
      .map((c) => ({
        name: c.name,
        type: c.type.toUpperCase(),
        notNull: c.notnull !== 0,
        default: c.dflt_value == null ? null : String(c.dflt_value),
        pk: c.pk
      }))
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
    tables[m.name] = { columns, withoutRowid: /WITHOUT\s+ROWID/i.test(m.sql ?? '') }
  }

  // pragma_index_list also reports the sqlite_autoindex_* rows UNIQUE/PK
  // constraints create: keep them — they encode the constraint itself, and
  // their numbering is deterministic for identical CREATE TABLE statements.
  const collected: NormalizedSchema['indexes'] = {}
  for (const t of Object.keys(tables)) {
    const list = d
      .prepare('SELECT name, "unique", origin FROM pragma_index_list(?)')
      .all(t) as unknown as IndexListRow[]
    for (const ix of list) {
      collected[ix.name] = {
        table: t,
        unique: ix.unique !== 0,
        origin: ix.origin,
        columns: (
          d.prepare('SELECT seqno, name FROM pragma_index_info(?)').all(ix.name) as unknown as IndexInfoRow[]
        )
          .sort((a, b) => a.seqno - b.seqno)
          .map((c) => c.name ?? '<expr>')
      }
    }
  }
  const indexes: NormalizedSchema['indexes'] = {}
  for (const k of Object.keys(collected).sort()) indexes[k] = collected[k]

  const triggers: Record<string, string> = {}
  for (const m of master) {
    if (m.type === 'trigger' && m.sql) triggers[m.name] = flat(m.sql)
  }

  return { tables, virtualTables, indexes, triggers }
}

describe('db schema parity (fixture shared with the twin tree)', () => {
  it('replaying the local migration chain yields exactly the agreed schema', () => {
    const d = new DatabaseSync(':memory:')
    try {
      for (const step of MIGRATIONS) {
        // The function steps only use exec/prepare/all/run with bare named
        // parameters — the subset DatabaseSync shares with better-sqlite3
        // (bare @name binding is on by default in node:sqlite).
        if (typeof step === 'function') step(d as unknown as Database.Database)
        else d.exec(step)
      }
      const actual = normalizeSchema(d)
      if (process.env.DB_SCHEMA_DUMP) {
        writeFileSync(process.env.DB_SCHEMA_DUMP, JSON.stringify(actual, null, 2) + '\n')
      }
      const expected = JSON.parse(
        readFileSync(join(import.meta.dirname, 'db.schema.expected.json'), 'utf-8')
      ) as unknown
      expect(actual).toEqual(expected)
    } finally {
      d.close()
    }
  })
})
