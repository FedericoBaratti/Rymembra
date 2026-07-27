/**
 * Driver di prova su `node:sqlite`.
 *
 * Non è un mock: è SQLite vero, la stessa libreria che sta sotto better-sqlite3.
 * Il legacy usava già `node:sqlite` per il test di parità dello schema, con la
 * motivazione giusta — una migrazione va provata su un motore che rifiuta l'SQL
 * sbagliato, non su un finto che accetta tutto.
 *
 * Vive nel codice di produzione e non in un file `.test.ts` perché serve a più
 * suite (db, library, playlist) e perché è anche la prova che l'interfaccia
 * `SqliteDriver` è implementabile da un terzo driver: se domani non lo fosse più,
 * qui si rompe la compilazione.
 *
 * Non entra nel bundle: nessun modulo di dominio lo importa, e `node:sqlite` non
 * esiste su nodejs-mobile.
 */

// Nessun modulo di dominio importa questo file, quindi non entra mai nel bundle
// del dispositivo — ed è la guardia check:node12 a tenere quella promessa per
// tutti gli altri file.
import { DatabaseSync } from 'node:sqlite' // node12-ok: solo test, mai sul dispositivo
import type { SqlParams, SqlRow, SqlRunInfo, SqlStatement, SqliteDriver } from './driver'

interface StatementLike {
  all(...params: unknown[]): unknown[]
  get(...params: unknown[]): unknown
  run(...params: unknown[]): { changes: number | bigint; lastInsertRowid: number | bigint }
}

/** I parametri passano come array sparso o come singolo oggetto di named params. */
function spread(params: SqlParams | undefined): unknown[] {
  if (params === undefined) return []
  return Array.isArray(params) ? [...params] : [params]
}

export function createTestDriver(path = ':memory:'): SqliteDriver {
  const database = new DatabaseSync(path)
  let depth = 0

  const wrap = (statement: StatementLike): SqlStatement => ({
    all: (params) => statement.all(...spread(params)) as SqlRow[],
    get: (params) => statement.get(...spread(params)) as SqlRow | undefined,
    run: (params) => {
      const info = statement.run(...spread(params))
      return {
        changes: Number(info.changes),
        lastInsertRowid: Number(info.lastInsertRowid)
      } satisfies SqlRunInfo
    }
  })

  return {
    exec: (sql) => database.exec(sql),
    prepare: (sql) => wrap(database.prepare(sql) as unknown as StatementLike),
    transaction: <T>(fn: () => T): T => {
      // Annidata: SAVEPOINT, come fa better-sqlite3. Senza, una transazione
      // dentro un'altra fallirebbe con "cannot start a transaction within a
      // transaction" — e i passi di migrazione che chiamano helper transazionali
      // sono esattamente quel caso.
      const nested = depth > 0
      const name = `sp_${depth}`
      database.exec(nested ? `SAVEPOINT ${name}` : 'BEGIN')
      depth++
      try {
        const value = fn()
        database.exec(nested ? `RELEASE ${name}` : 'COMMIT')
        return value
      } catch (cause) {
        database.exec(nested ? `ROLLBACK TO ${name}` : 'ROLLBACK')
        throw cause
      } finally {
        depth--
      }
    },
    userVersion: () => {
      const row = database.prepare('PRAGMA user_version').get() as
        | { user_version?: number }
        | undefined
      return row?.user_version ?? 0
    },
    setUserVersion: (version) => {
      // Interpolato e non legato: SQLite non accetta parametri nei PRAGMA. Il
      // valore è un intero prodotto dalla catena, non un input esterno, ma si
      // forza comunque a intero — un'iniezione da qui sarebbe imperdonabile.
      database.exec(`PRAGMA user_version = ${Math.trunc(version)}`)
    },
    close: () => database.close()
  }
}
