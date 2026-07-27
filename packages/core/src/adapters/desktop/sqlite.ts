/**
 * Il driver SQLite del desktop: better-sqlite3 dietro l'interfaccia.
 *
 * Il file è diviso in due metà per un motivo pratico. `adaptBetterSqlite` è pura
 * adattamento e si prova con un doppio; `openBetterSqlite` è l'unico punto che
 * carica il modulo nativo. La separazione conta perché il binario nativo di
 * better-sqlite3 è compilato per l'ABI di Electron: caricarlo da Node semplice
 * fallisce. Se il caricamento stesse in cima al file, importare questo modulo
 * romperebbe ogni test che lo tocca — anche quelli che non aprono nessun database.
 *
 * Nel legacy la separazione non c'era: `db.ts` importava `better-sqlite3` alla
 * riga 1 e `electron` alla riga 2, quindi il test dello schema doveva mockare
 * Electron e non poteva provare l'apertura in nessun modo.
 */

import { AppError } from '../../errors'
import { logger } from '../../logger'
import { sqlError, type SqlParams, type SqlRow, type SqlStatement, type SqliteDriver } from '../../db'
import { foldText } from '../../shared'

const log = logger('db')

/**
 * La superficie di better-sqlite3 che usiamo davvero.
 *
 * Dichiararla invece di dipendere dai tipi del pacchetto rende visibile quanto
 * poco serve, e permette al doppio dei test di essere completo invece di
 * approssimato.
 */
export interface BetterSqliteStatement {
  all(params?: unknown): unknown[]
  get(params?: unknown): unknown
  run(params?: unknown): { changes: number; lastInsertRowid: number | bigint }
}

export interface BetterSqliteLike {
  prepare(sql: string): BetterSqliteStatement
  exec(sql: string): void
  pragma(source: string, options?: { simple?: boolean }): unknown
  transaction<T>(fn: () => T): () => T
  function(name: string, implementation: (...args: unknown[]) => unknown): void
  close(): void
}

/** better-sqlite3 vuole i parametri sparsi, o un oggetto solo per quelli con nome. */
function toArgs(params: SqlParams | undefined): unknown[] {
  if (params === undefined) return []
  return Array.isArray(params) ? [...params] : [params]
}

export function adaptBetterSqlite(database: BetterSqliteLike): SqliteDriver {
  const wrap = (statement: BetterSqliteStatement): SqlStatement => ({
    all: (params) => statement.all(...toArgs(params)) as SqlRow[],
    get: (params) => statement.get(...toArgs(params)) as SqlRow | undefined,
    run: (params) => {
      const info = statement.run(...toArgs(params))
      return {
        changes: Number(info.changes),
        // lastInsertRowid è un bigint quando supera 2^53: convertirlo a numero è
        // sicuro per gli id di questo schema, ma va fatto esplicitamente o
        // finirebbe in JSON come "12n" e il renderer non saprebbe leggerlo.
        lastInsertRowid: Number(info.lastInsertRowid)
      }
    }
  })

  return {
    exec: (sql) => database.exec(sql),
    prepare: (sql) => wrap(database.prepare(sql)),
    // È better-sqlite3 a fornire la transazione, non noi: la sua implementazione
    // gestisce anche i savepoint annidati, e riscriverla con BEGIN/COMMIT a mano
    // perderebbe quel comportamento.
    transaction: <T>(fn: () => T): T => database.transaction(fn)(),
    userVersion: () => {
      const value = database.pragma('user_version', { simple: true })
      return typeof value === 'number' ? value : 0
    },
    setUserVersion: (version) => {
      // Interpolato perché SQLite non accetta parametri nei PRAGMA, e forzato a
      // intero perché interpolare è l'unica strada.
      database.pragma(`user_version = ${Math.trunc(version)}`)
    },
    close: () => database.close()
  }
}

export interface DesktopSqliteOptions {
  /**
   * Il modulo `better-sqlite3`. Iniettato perché il caricamento del binario
   * nativo dipende dall'ABI: nel main process di Electron funziona, in un test
   * su Node semplice no.
   */
  readonly createDatabase: (path: string) => BetterSqliteLike
  /** Registra `afold()`. Vero per default: la ricerca di ripiego la richiede. */
  readonly registerFunctions?: boolean
}

/**
 * Apre e configura una connessione desktop.
 *
 * I pragma sono quelli del legacy, e vale ricordare perché: WAL permette a un
 * lettore di non bloccare uno scrittore (la scansione della libreria scrive
 * mentre la UI legge), `foreign_keys` fa rispettare i CASCADE che lo schema
 * dichiara — spento, che è il default di SQLite, le playlist manterrebbero righe
 * orfane — e `synchronous = NORMAL` è il compromesso giusto con WAL.
 */
export function openBetterSqlite(path: string, options: DesktopSqliteOptions): SqliteDriver {
  const database = options.createDatabase(path)

  database.pragma('journal_mode = WAL')
  database.pragma('foreign_keys = ON')
  database.pragma('synchronous = NORMAL')

  if (options.registerFunctions !== false) {
    registerScalarFunctions(database)
  }

  return adaptBetterSqlite(database)
}

/**
 * `afold()`: minuscole senza diacritici, in SQL.
 *
 * Serve alla ricerca di ripiego perché deve trattare accenti e maiuscole come li
 * tratta FTS5 con `remove_diacritics 2`. Sul desktop FTS5 c'è, quindi questa è la
 * strada secondaria; sul mobile è l'unica.
 */
function registerScalarFunctions(database: BetterSqliteLike): void {
  database.function('afold', (value: unknown): string =>
    value === null || value === undefined ? '' : foldText(String(value))
  )
}

/**
 * La tabella virtuale FTS5 esiste ed è interrogabile.
 *
 * Si prova con una query invece di leggere `sqlite_master`, perché una tabella
 * FTS5 può esistere nello schema e non essere caricabile se il binario di SQLite
 * è stato compilato senza il modulo — e in quel caso la ricerca deve ripiegare,
 * non fallire a ogni tasto premuto.
 */
export function detectFts5(driver: SqliteDriver): boolean {
  try {
    driver.prepare("SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH 'x' LIMIT 1").all()
    return true
  } catch (cause) {
    log.debug('FTS5 non disponibile, si userà la ricerca con afold()', {
      reason: AppError.from(cause).message
    })
    return false
  }
}

/**
 * Verifica di integrità, per decidere se un file va messo in quarantena.
 *
 * `quick_check` invece di `integrity_check`: su una libreria da 100.000 tracce la
 * verifica completa può richiedere minuti, e questa gira all'avvio.
 */
export function isHealthy(driver: SqliteDriver): boolean {
  try {
    const row = driver.prepare('PRAGMA quick_check').get()
    const first = row === undefined ? undefined : Object.values(row)[0]
    return String(first).toLowerCase() === 'ok'
  } catch (cause) {
    log.warn('verifica di integrità non riuscita', sqlError(cause))
    return false
  }
}
