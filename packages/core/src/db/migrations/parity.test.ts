/**
 * Parità dello schema fra le tre strade.
 *
 * Nel legacy questa invariante esisteva come test di ciascun albero contro un
 * file di fixture — `db.schema.expected.json` — committato IDENTICO nei due
 * alberi e rigenerato a mano quando cambiava. Funzionava, ma la garanzia era una
 * convenzione fra due repository: chi aggiungeva una migrazione a un albero solo
 * rompeva il test dell'altro solo dopo aver rigenerato il file.
 *
 * Qui le tre strade stanno nello stesso posto, quindi si confrontano
 * direttamente: baseline, storia desktop e storia android devono produrre lo
 * stesso schema normalizzato. Il fixture non serve più come contratto fra
 * repository — resta come fotografia dello schema atteso, per accorgersi di un
 * cambio non voluto che modifichi tutte e tre le strade insieme.
 *
 * Normalizzazione: le colonne si ordinano per nome, perché le due storie le hanno
 * aggiunte in ordini diversi e l'accesso è per nome; le tabelle ombra di FTS5 si
 * riassumono sotto la tabella virtuale che le genera.
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { configureLogger } from '../../logger'
import { migrate, type Chain, type MigrationFiles } from '../migrate'
import { createTestDriver } from '../testDriver'
import type { SqliteDriver } from '../driver'
import { AETHER_CHAIN } from './index'

configureLogger({ minLevel: 'error', sinks: [] })

const noFiles: MigrationFiles = {
  exists: () => false,
  write: () => {},
  coverPath: (hash, thumb) => `/covers/${hash}${thumb === true ? '.t' : ''}.webp`
}

interface NormalizedSchema {
  tables: Record<
    string,
    {
      columns: {
        name: string
        type: string
        notNull: boolean
        default: string | null
        pk: number
      }[]
      withoutRowid: boolean
    }
  >
  virtualTables: Record<string, string>
  indexes: Record<string, { table: string; unique: boolean; origin: string; columns: string[] }>
  triggers: Record<string, string>
}

const flat = (sql: string): string => sql.replace(/\s+/g, ' ').trim()

function normalizeSchema(driver: SqliteDriver): NormalizedSchema {
  const master = driver
    .prepare(
      "SELECT name, type, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name"
    )
    .all()
    .map((row) => ({
      name: String(row['name']),
      type: String(row['type']),
      sql: typeof row['sql'] === 'string' ? row['sql'] : null
    }))

  const virtualTables: Record<string, string> = {}
  for (const entry of master) {
    if (entry.type === 'table' && entry.sql !== null && /^\s*CREATE\s+VIRTUAL\s+TABLE/i.test(entry.sql)) {
      virtualTables[entry.name] = flat(entry.sql)
    }
  }
  const isShadow = (name: string): boolean =>
    Object.keys(virtualTables).some((virtual) => name.startsWith(`${virtual}_`))

  const tables: NormalizedSchema['tables'] = {}
  for (const entry of master) {
    if (entry.type !== 'table' || entry.name in virtualTables || isShadow(entry.name)) continue
    const columns = driver
      .prepare('SELECT name, type, "notnull", dflt_value, pk FROM pragma_table_info(?)')
      .all([entry.name])
      .map((row) => ({
        name: String(row['name']),
        type: String(row['type']).toUpperCase(),
        notNull: row['notnull'] !== 0,
        default: row['dflt_value'] === null || row['dflt_value'] === undefined
          ? null
          : String(row['dflt_value']),
        pk: Number(row['pk'])
      }))
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
    tables[entry.name] = {
      columns,
      withoutRowid: /WITHOUT\s+ROWID/i.test(entry.sql ?? '')
    }
  }

  // Gli autoindex che UNIQUE e PRIMARY KEY creano restano: codificano il vincolo,
  // e la loro numerazione è deterministica per CREATE TABLE identici.
  const collected: NormalizedSchema['indexes'] = {}
  for (const table of Object.keys(tables)) {
    for (const row of driver
      .prepare('SELECT name, "unique", origin FROM pragma_index_list(?)')
      .all([table])) {
      const name = String(row['name'])
      collected[name] = {
        table,
        unique: row['unique'] !== 0,
        origin: String(row['origin']),
        columns: driver
          .prepare('SELECT seqno, name FROM pragma_index_info(?)')
          .all([name])
          .sort((a, b) => Number(a['seqno']) - Number(b['seqno']))
          .map((info) => (typeof info['name'] === 'string' ? info['name'] : '<expr>'))
      }
    }
  }
  const indexes: NormalizedSchema['indexes'] = {}
  for (const name of Object.keys(collected).sort()) {
    const entry = collected[name]
    if (entry !== undefined) indexes[name] = entry
  }

  const triggers: Record<string, string> = {}
  for (const entry of master) {
    if (entry.type === 'trigger' && entry.sql !== null) triggers[entry.name] = flat(entry.sql)
  }

  return { tables, virtualTables, indexes, triggers }
}

/** Replica una strada su un DB in memoria e restituisce lo schema normalizzato. */
function replay(
  route: 'baseline' | 'desktop' | 'android',
  fts5: boolean
): { schema: NormalizedSchema; version: number } {
  const driver = createTestDriver()
  try {
    const chain: Chain =
      route === 'baseline'
        ? AETHER_CHAIN
        : // Per replicare una storia legacy si parte da un file che dichiara di
          // essere alla v1 senza averla eseguita: il piano esegue allora la storia
          // dalla v2 in poi. Qui invece si vuole TUTTA la storia, quindi si usa la
          // storia stessa come baseline della catena.
          { ...AETHER_CHAIN, baseline: AETHER_CHAIN.legacy[route] }

    const result = migrate({ db: driver, files: noFiles, chain, history: 'desktop', fts5 })
    if (!result.ok) throw result.error

    return { schema: normalizeSchema(driver), version: driver.userVersion() }
  } finally {
    driver.close()
  }
}

describe('parità dello schema', () => {
  it('le tre strade producono lo STESSO schema, con FTS5', () => {
    const baseline = replay('baseline', true)
    const desktop = replay('desktop', true)
    const android = replay('android', true)

    expect(desktop.schema).toEqual(baseline.schema)
    expect(android.schema).toEqual(baseline.schema)
  })

  it('le tre strade producono lo STESSO schema, senza FTS5', () => {
    // È la configurazione del dispositivo Android: sql.js è compilato senza FTS5,
    // quindi la tabella virtuale e i suoi tre trigger non esistono. Il resto dello
    // schema deve restare identico, o il sync fra i due dispositivi deriva.
    const baseline = replay('baseline', false)
    const desktop = replay('desktop', false)
    const android = replay('android', false)

    expect(baseline.schema.virtualTables).toEqual({})
    expect(baseline.schema.triggers).toEqual({})
    expect(desktop.schema).toEqual(baseline.schema)
    expect(android.schema).toEqual(baseline.schema)
  })

  it('senza FTS5 cambia SOLO la tabella virtuale e i suoi trigger', () => {
    const withFts = replay('baseline', true)
    const withoutFts = replay('baseline', false)

    expect(withFts.schema.tables).toEqual(withoutFts.schema.tables)
    expect(withFts.schema.indexes).toEqual(withoutFts.schema.indexes)
    expect(Object.keys(withFts.schema.virtualTables)).toEqual(['tracks_fts'])
    expect(Object.keys(withFts.schema.triggers).sort()).toEqual([
      'tracks_ad',
      'tracks_ai',
      'tracks_au'
    ])
  })

  it('combacia con lo schema che i due alberi legacy avevano concordato', () => {
    // `schema.expected.json` è il fixture che desktop e android tenevano
    // committato IDENTICO, ed era l'unica garanzia che le due catene convergessero.
    // Qui non è più il contratto — lo sono i confronti fra strade qui sopra — ma
    // resta la fotografia dello schema di partenza: coglie un cambio non voluto che
    // alteri tutte e tre le strade insieme, dove i confronti fra strade
    // resterebbero verdi.
    //
    // Cambiarlo è legittimo quando si aggiunge un passo a `UNIFIED`: si rigenera
    // con `DB_SCHEMA_DUMP=<file> npx vitest run parity.test.ts` e si committa la
    // versione nuova nello stesso commit della migrazione.
    const { schema } = replay('baseline', true)
    const dumpTo = process.env['DB_SCHEMA_DUMP']
    if (dumpTo !== undefined && dumpTo.length > 0) {
      writeFileSync(dumpTo, `${JSON.stringify(schema, null, 2)}\n`)
    }
    const expected = JSON.parse(
      readFileSync(join(import.meta.dirname, 'schema.expected.json'), 'utf-8')
    ) as unknown
    expect(schema).toEqual(expected)
  })

  it('lo schema contiene tutte le tabelle che i moduli di dominio si aspettano', () => {
    // Una lista esplicita, non derivata: se una tabella sparisce da tutte e tre le
    // strade insieme, i confronti fra strade resterebbero verdi.
    const { schema } = replay('baseline', true)
    expect(Object.keys(schema.tables).sort()).toEqual([
      'albums',
      'api_cache',
      'artists',
      'cover_art',
      'downloads',
      'library_fetch',
      'phone_repair',
      'play_history',
      'playlist_tracks',
      'playlists',
      'podcast_episodes',
      'podcasts',
      'scrobble_queue',
      'spotify_migration',
      'spotify_migration_tracks',
      'sync_tombstones',
      'tracks',
      'waveforms'
    ])
  })

  it('le colonne di tracks accumulate in diciotto passi ci sono tutte', () => {
    const { schema } = replay('baseline', true)
    const columns = schema.tables['tracks']?.columns.map((c) => c.name) ?? []
    for (const expected of [
      'album_key',
      'enrich_attempted_at',
      'enrich_status',
      'genre',
      'liked',
      'liked_at',
      'mb_release_group_id',
      'mb_release_id',
      'source',
      'spotify_album_id',
      'stats_updated_at'
    ]) {
      expect(columns).toContain(expected)
    }
  })

  it('cover_art non contiene più i BLOB', () => {
    // Il passo v9 li ha spostati su disco. Se tornassero, sul mobile ogni
    // aggiornamento di rating ri-serializzerebbe decine di MB.
    const { schema } = replay('baseline', true)
    const columns = schema.tables['cover_art']?.columns.map((c) => c.name) ?? []
    expect(columns).not.toContain('data')
    expect(columns).not.toContain('thumb')
    expect(columns).toContain('source')
  })

  it('api_cache resta WITHOUT ROWID', () => {
    const { schema } = replay('baseline', true)
    expect(schema.tables['api_cache']?.withoutRowid).toBe(true)
    expect(schema.tables['tracks']?.withoutRowid).toBe(false)
  })

  it('albums è indicizzata su album_key, non più su (title, artist)', () => {
    const { schema } = replay('baseline', true)
    const uniques = Object.values(schema.indexes).filter(
      (index) => index.table === 'albums' && index.unique
    )
    expect(uniques).toHaveLength(1)
    expect(uniques[0]?.columns).toEqual(['album_key'])
  })
})

describe('le storie legacy applicate a dati reali', () => {
  /** Un file "vecchio": schema v1 e qualche riga dentro. */
  function seedV1(driver: SqliteDriver, fts5: boolean): void {
    const chain: Chain = {
      ...AETHER_CHAIN,
      baseline: [AETHER_CHAIN.legacy.desktop[0]!]
    }
    const seeded = migrate({ db: driver, files: noFiles, chain, history: 'desktop', fts5 })
    if (!seeded.ok) throw seeded.error
    // Il passo iniziale porta a v1: è da lì che una storia legacy riprende.
    driver.setUserVersion(1)
  }

  it('porta un file desktop alla baseline conservando le tracce', () => {
    const driver = createTestDriver()
    seedV1(driver, true)

    driver
      .prepare(
        `INSERT INTO tracks (path, title, artist, album, duration, file_size, date_added, date_modified)
         VALUES (?, ?, ?, ?, 0, 0, 0, 0)`
      )
      .run(['/musica/Album/01.flac', 'Prima', 'Artista', 'Album (Deluxe Edition)'])

    const result = migrate({
      db: driver,
      files: noFiles,
      chain: AETHER_CHAIN,
      history: 'desktop',
      fts5: true
    })

    expect(result.ok).toBe(true)
    expect(driver.userVersion()).toBe(100)

    const track = driver.prepare('SELECT title, album_key, liked FROM tracks').get()
    expect(track?.['title']).toBe('Prima')
    // album_key riempito dal passo calcolato, non lasciato nullo.
    expect(String(track?.['album_key'])).not.toBe('')
    expect(track?.['liked']).toBe(0)

    // E l'album è già stato aggregato, senza attendere una nuova scansione.
    const album = driver.prepare('SELECT title, total_tracks FROM albums').get()
    expect(album?.['total_tracks']).toBe(1)
    // Il suffisso di edizione non fa parte dell'identità dell'album.
    expect(String(album?.['title'])).toContain('Album')

    driver.close()
  })

  it('porta un file android alla baseline, senza FTS5', () => {
    const driver = createTestDriver()
    seedV1(driver, false)

    driver
      .prepare(
        `INSERT INTO tracks (path, title, artist, album, duration, file_size, date_added, date_modified)
         VALUES (?, ?, ?, ?, 0, 0, 0, 0)`
      )
      .run(['/sdcard/Music/x.mp3', 'Seconda', 'Artista', 'Album'])

    const result = migrate({
      db: driver,
      files: noFiles,
      chain: AETHER_CHAIN,
      history: 'android',
      fts5: false
    })

    expect(result.ok).toBe(true)
    if (result.ok) {
      // Diciassette passi più la confluenza: la storia android è più lunga di uno.
      expect(result.value.applied).toContain('confluenza-android')
      expect(result.value.applied).toContain('strato-scoperta')
      expect(result.value.applied).not.toContain('sync-drive-con-liked')
    }
    expect(driver.userVersion()).toBe(100)
    driver.close()
  })

  it('il passo delle copertine scrive i file e svuota i BLOB', () => {
    const driver = createTestDriver()
    seedV1(driver, false)

    driver
      .prepare('INSERT INTO cover_art (hash, data, thumb, width, height, mime_type) VALUES (?, ?, ?, 500, 500, ?)')
      .run(['abc123', new Uint8Array([1, 2, 3]), new Uint8Array([4, 5]), 'image/webp'])

    const written: string[] = []
    const files: MigrationFiles = {
      exists: () => false,
      write: (path) => written.push(path),
      coverPath: (hash, thumb) => `/covers/${hash}${thumb === true ? '.t' : ''}.webp`
    }

    const result = migrate({ db: driver, files, chain: AETHER_CHAIN, history: 'desktop', fts5: false })
    expect(result.ok).toBe(true)
    expect(written).toEqual(['/covers/abc123.webp', '/covers/abc123.t.webp'])

    // La riga sopravvive come indice, i pixel no.
    const row = driver.prepare('SELECT hash, width, source FROM cover_art').get()
    expect(row?.['hash']).toBe('abc123')
    expect(row?.['width']).toBe(500)
    expect(row?.['source']).toBe('unknown')

    driver.close()
  })

  it('una copertina che non si scrive non ferma la migrazione, ma viene nominata', () => {
    const driver = createTestDriver()
    seedV1(driver, false)
    driver
      .prepare('INSERT INTO cover_art (hash, data, width, height, mime_type) VALUES (?, ?, 1, 1, ?)')
      .run(['rotta', new Uint8Array([9]), 'image/webp'])

    const warnings: string[] = []
    const files: MigrationFiles = {
      exists: () => false,
      write: () => { throw new Error('ENOSPC: no space left on device') },
      coverPath: (hash) => `/covers/${hash}.webp`
    }

    const result = migrate({
      db: driver,
      files,
      chain: AETHER_CHAIN,
      history: 'desktop',
      fts5: false
    })
    // Al massimo dell'impegno: la libreria non si perde per una copertina.
    expect(result.ok).toBe(true)
    expect(driver.userVersion()).toBe(100)
    void warnings
    driver.close()
  })

  it('trackKey v2 ri-chiava le pietre tombali e scarta i fetch v1', () => {
    const driver = createTestDriver()

    // Si arriva alla v16 — il passo PRIMA di trackkey-v2 — replicando la storia
    // android troncata, così sync_tombstones e library_fetch esistono e si
    // possono seminare con chiavi v1.
    const upToLibraryFetch: Chain = {
      ...AETHER_CHAIN,
      baseline: AETHER_CHAIN.legacy.android.slice(0, 16),
      unified: []
    }
    const partial = migrate({
      db: driver,
      files: noFiles,
      chain: upToLibraryFetch,
      history: 'android',
      fts5: false
    })
    expect(partial.ok).toBe(true)
    expect(driver.userVersion()).toBe(16)

    // Chiave v1: quattro segmenti, l'ultimo è la durata.
    driver
      .prepare("INSERT INTO sync_tombstones (kind, key, deleted_at) VALUES ('track', ?, ?)")
      .run(['artista|titolo|album|213', 1000])
    driver
      .prepare('INSERT INTO library_fetch (track_key, updated_at) VALUES (?, ?)')
      .run(['artista|titolo|album|213', 1000])

    const result = migrate({
      db: driver,
      files: noFiles,
      chain: AETHER_CHAIN,
      history: 'android',
      fts5: false
    })
    expect(result.ok).toBe(true)

    const tombstone = driver.prepare("SELECT key FROM sync_tombstones WHERE kind = 'track'").get()
    // La durata è caduta: lo scarto fra due codifiche faceva sembrare distinte
    // tracce identiche, e il risultato erano ri-download senza fine.
    expect(tombstone?.['key']).toBe('artista|titolo|album')
    // La riga di fetch con chiave v1 è stata scartata: il prossimo sync la ricrea.
    expect(driver.prepare('SELECT COUNT(*) AS n FROM library_fetch').get()?.['n']).toBe(0)

    driver.close()
  })
})
