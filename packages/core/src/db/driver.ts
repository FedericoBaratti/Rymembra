/**
 * Il driver SQLite come interfaccia, non come dipendenza.
 *
 * Nel legacy `db.ts` importava `better-sqlite3` in cima e ogni modulo di dominio
 * chiamava `getDb()` ottenendo un `Database.Database`. Sul mobile la stessa cosa
 * era ottenuta con `sqlite-shim.ts` (416 righe, sql.js su WASM) più un alias di
 * bundling che sostituiva il modulo — cioè la compatibilità era garantita dal
 * build, non dai tipi. Bastava usare un metodo che lo shim non implementava per
 * scoprirlo sul dispositivo.
 *
 * Qui la superficie è dichiarata: è l'intersezione di ciò che sanno fare
 * better-sqlite3 (desktop), sql.js (mobile) e `node:sqlite` (test). Un adapter che
 * non riesce a implementarla non compila, e nessun modulo di dominio può usare
 * di più.
 *
 * Vincolo di forma: i parametri sono un array o un oggetto, mai varargs. I tre
 * driver li accettano tutti in questa forma, mentre i varargs di better-sqlite3
 * non esistono altrove.
 */

import { AppError } from '../errors'
import { err, ok, type Result } from '../result'

export type SqlValue = string | number | bigint | boolean | null | Uint8Array
export type SqlParams = readonly SqlValue[] | Readonly<Record<string, SqlValue>>
export type SqlRow = Record<string, SqlValue | undefined>

export interface SqlRunInfo {
  readonly changes: number
  readonly lastInsertRowid: number
}

export interface SqlStatement {
  all(params?: SqlParams): SqlRow[]
  get(params?: SqlParams): SqlRow | undefined
  run(params?: SqlParams): SqlRunInfo
}

export interface SqliteDriver {
  /** Più istruzioni separate da `;`, senza parametri. */
  exec(sql: string): void
  prepare(sql: string): SqlStatement
  /**
   * Esegue `fn` in una transazione, con rollback se solleva.
   *
   * È il driver a fornirla e non questo modulo perché better-sqlite3 la
   * implementa con `d.transaction()`, che gestisce anche i savepoint annidati —
   * riscriverla a mano con BEGIN/COMMIT perderebbe quel comportamento.
   */
  transaction<T>(fn: () => T): T
  userVersion(): number
  setUserVersion(version: number): void
  close(): void
}

/**
 * Un'istanza aperta, con le sue capacità.
 *
 * `fts5` sta qui e non nelle capacità globali perché è una proprietà del DB
 * aperto: lo stesso codice, sullo stesso dispositivo, può trovarsi con o senza la
 * tabella virtuale a seconda di come il file è stato creato.
 */
export interface Db {
  readonly driver: SqliteDriver
  readonly path: string
  readonly version: number
  readonly fts5: boolean
}

/** Esegue una query racchiudendo il fallimento in un Result. */
export function queryAll(
  driver: SqliteDriver,
  sql: string,
  params?: SqlParams
): Result<SqlRow[], AppError> {
  try {
    return ok(driver.prepare(sql).all(params))
  } catch (cause) {
    return err(sqlError(cause, sql))
  }
}

export function queryOne(
  driver: SqliteDriver,
  sql: string,
  params?: SqlParams
): Result<SqlRow | undefined, AppError> {
  try {
    return ok(driver.prepare(sql).get(params))
  } catch (cause) {
    return err(sqlError(cause, sql))
  }
}

export function execute(
  driver: SqliteDriver,
  sql: string,
  params?: SqlParams
): Result<SqlRunInfo, AppError> {
  try {
    return ok(driver.prepare(sql).run(params))
  } catch (cause) {
    return err(sqlError(cause, sql))
  }
}

/**
 * Classifica un guasto SQLite.
 *
 * Le tre risposte sono diverse e nel legacy erano la stessa: `logWarn` più una
 * stringa. Un DB bloccato è transitorio e va ritentato, un DB corrotto richiede
 * la quarantena del file, una query sbagliata è un bug nostro.
 */
export function sqlError(cause: unknown, sql?: string): AppError {
  const message = cause instanceof Error ? cause.message : String(cause)

  if (/SQLITE_BUSY|database is locked/i.test(message)) {
    return AppError.of('db.locked', {}, { cause, ...(sql ? { context: { sql: brief(sql) } } : {}) })
  }
  if (/SQLITE_CORRUPT|malformed|not a database|file is encrypted/i.test(message)) {
    return AppError.of('db.corrupt', {}, { cause })
  }
  return AppError.of(
    'db.queryFailed',
    { detail: message },
    sql !== undefined ? { cause, context: { sql: brief(sql) } } : { cause }
  )
}

/** L'SQL nei log serve a riconoscere la query, non a rileggerla per intero. */
function brief(sql: string): string {
  const flat = sql.replace(/\s+/g, ' ').trim()
  return flat.length > 200 ? `${flat.slice(0, 197)}...` : flat
}
