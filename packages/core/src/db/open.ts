/**
 * Apertura del DB.
 *
 * Questo file esiste per un bug preciso del legacy, e la sua forma è la
 * correzione.
 *
 * `electron/ipc/library.ipc.ts:65` chiamava `getDb()` come prima istruzione di
 * `registerLibraryIpc()`, cioè PRIMA di registrare qualunque handler. E `getDb()`
 * lanciava. Quindi un DB corrotto, un disco pieno o un file bloccato da un altro
 * processo producevano questo: nessun handler registrato, il renderer che chiama
 * `library:getTracks` e resta in attesa per sempre, la UI ferma sugli scheletri.
 * Nessun messaggio, nessun errore, nessun modo di capirlo dall'interno dell'app.
 *
 * L'albero mobile l'aveva già risolto — `node-backend/main.ts:233-252`, col
 * commento "registering IPC anyway" — e si adotta quel comportamento:
 *
 *   1. `open()` non lancia: restituisce un Result.
 *   2. Il guasto si RICORDA. `get()` lo restituisce a ogni chiamata successiva,
 *      quindi ogni handler che serve il DB risponde subito con un errore
 *      classificato invece di non rispondere.
 *   3. La registrazione degli handler non dipende dall'apertura. Un DB guasto
 *      rende inutilizzabile la libreria, non l'applicazione: le impostazioni,
 *      l'aspetto, la diagnostica e il pannello degli errori continuano a
 *      funzionare — ed è da lì che l'utente scopre cosa è andato storto.
 *   4. `reopen()` esiste perché alcuni guasti sono transitori (SQLITE_BUSY, un
 *      volume di rete che torna) e chiedere all'utente di riavviare l'app per un
 *      lock è una risposta povera.
 */

import { AppError } from '../errors'
import { logger } from '../logger'
import { err, ok, type Result } from '../result'
import type { Db, SqliteDriver } from './driver'
import { sqlError } from './driver'
import {
  latestVersion,
  migrate,
  validateChain,
  type Chain,
  type LegacyHistoryName,
  type MigrationFiles
} from './migrate'

const log = logger('db')

export type DbStatus = 'closed' | 'open' | 'failed'

export interface OpenDbDeps {
  readonly path: string
  /**
   * Crea il driver. Iniettata: è l'unico punto in cui entra better-sqlite3 o
   * sql.js, e il test la sostituisce con `node:sqlite`.
   */
  readonly openDriver: (path: string) => SqliteDriver
  readonly chain: Chain
  /** Quale storia legacy ha scritto questo file. Lo sa l'adapter, non il core. */
  readonly history: LegacyHistoryName
  readonly files: MigrationFiles
  /**
   * Prepara la connessione: pragma (WAL, foreign_keys, synchronous), funzioni
   * scalari come `afold()`. Sta nell'adapter perché i pragma disponibili
   * dipendono dal driver — sql.js non ha WAL.
   */
  readonly configure?: (driver: SqliteDriver) => void
  /**
   * Se il driver SUPPORTA FTS5. Va alle migrazioni, che creano la tabella
   * virtuale solo quando è vero: su sql.js crearla farebbe fallire ogni insert
   * su `tracks` a causa dei trigger.
   */
  readonly supportsFts5?: boolean
  /**
   * Se la tabella virtuale c'è DAVVERO in questo file. Domanda diversa dalla
   * precedente: un file creato da una build senza FTS5 non ce l'ha, anche se il
   * driver che lo apre ora la supporterebbe. È questa che decide come si cerca.
   */
  readonly detectFts5?: (driver: SqliteDriver) => boolean
  readonly backup?: (version: number) => void
  /**
   * Sposta da parte un file corrotto, restituendo dove è finito. Senza questo un
   * DB corrotto è un vicolo cieco: ogni avvio ritenta e fallisce allo stesso modo.
   */
  readonly quarantine?: (path: string) => string
}

export interface DbHandle {
  readonly status: DbStatus
  /** Il DB aperto, o il guasto ricordato. Non lancia, non blocca. */
  get(): Result<Db, AppError>
  /** Apre se serve. Idempotente. */
  open(): Result<Db, AppError>
  /** Dimentica il guasto e riprova. Per i lock e i volumi che tornano. */
  reopen(): Result<Db, AppError>
  close(): void
}

export function createDbHandle(deps: OpenDbDeps): DbHandle {
  let db: Db | null = null
  let failure: AppError | null = null

  function attempt(): Result<Db, AppError> {
    const invalidChain = validateChain(deps.chain)
    if (!invalidChain.ok) return invalidChain

    let driver: SqliteDriver
    try {
      driver = deps.openDriver(deps.path)
    } catch (cause) {
      const error = classifyOpenFailure(cause, deps.path)
      return err(maybeQuarantine(error, deps))
    }

    try {
      deps.configure?.(driver)
    } catch (cause) {
      driver.close()
      return err(AppError.of('db.openFailed', { path: deps.path }, { cause }))
    }

    const migrated = migrate({
      db: driver,
      files: deps.files,
      chain: deps.chain,
      history: deps.history,
      fts5: deps.supportsFts5 ?? false,
      ...(deps.backup !== undefined ? { backup: deps.backup } : {})
    })

    if (!migrated.ok) {
      // La connessione si chiude, ma il file NON si tocca: una migrazione fallita
      // lascia il file alla versione precedente, che è ancora leggibile dalla
      // build che l'ha scritto. Metterlo in quarantena qui sarebbe distruggere
      // dati recuperabili.
      driver.close()
      return err(migrated.error)
    }

    let fts5 = false
    try {
      fts5 = deps.detectFts5?.(driver) ?? false
    } catch (cause) {
      // Non sapere se c'è FTS5 non è un motivo per non aprire: si ripiega sulla
      // ricerca con afold(), che è la strada che il mobile usa sempre.
      log.warn('rilevamento FTS5 non riuscito, si userà la ricerca di ripiego', cause)
    }

    const version = driver.userVersion()
    log.info('database aperto', {
      version,
      latest: latestVersion(deps.chain),
      fts5,
      migrations: migrated.value.applied.length
    })

    return ok({ driver, path: deps.path, version, fts5 })
  }

  function open(): Result<Db, AppError> {
    if (db !== null) return ok(db)
    if (failure !== null) return err(failure)

    const result = attempt()
    if (result.ok) {
      db = result.value
      failure = null
    } else {
      // Ricordare il guasto è il punto: da qui in poi ogni handler che serve il
      // DB risponde con QUESTO errore, invece di riprovare ad aprire a ogni
      // chiamata e far aspettare il renderer ogni volta.
      failure = result.error
      log.error('apertura del database non riuscita', result.error, { path: deps.path })
    }
    return result
  }

  return {
    get status(): DbStatus {
      if (db !== null) return 'open'
      return failure !== null ? 'failed' : 'closed'
    },
    get: () => (db !== null ? ok(db) : failure !== null ? err(failure) : open()),
    open,
    reopen: () => {
      if (db !== null) {
        try {
          db.driver.close()
        } catch (cause) {
          log.warn('chiusura prima della riapertura non riuscita', cause)
        }
      }
      db = null
      failure = null
      return open()
    },
    close: () => {
      if (db === null) return
      try {
        db.driver.close()
      } catch (cause) {
        log.warn('chiusura del database non riuscita', cause)
      }
      db = null
    }
  }
}

function classifyOpenFailure(cause: unknown, path: string): AppError {
  const classified = sqlError(cause)

  // Corrotto e bloccato sono le due risposte che cambiano cosa fa l'app: il
  // primo va messo da parte, il secondo si ritenta. Nel legacy erano entrambi
  // un throw generico da getDb().
  if (classified.code === 'db.corrupt') return AppError.of('db.corrupt', { path }, { cause })
  if (classified.code === 'db.locked') return classified.withContext({ dbPath: path })

  // Un errno di filesystem (ENOENT sulla cartella, EACCES, ENOSPC) resta un
  // errore di filesystem: dice all'utente cosa fare, dove 'db.openFailed'
  // direbbe solo che non si è aperto.
  const errno = AppError.from(cause)
  if (errno.domain === 'fs') return errno.withContext({ dbPath: path })
  return AppError.of('db.openFailed', { path }, { cause })
}

function maybeQuarantine(error: AppError, deps: OpenDbDeps): AppError {
  if (error.code !== 'db.corrupt' || deps.quarantine === undefined) return error
  try {
    const quarantinedAs = deps.quarantine(deps.path)
    log.warn('database corrotto messo da parte', error, { quarantinedAs })
    return AppError.of('db.corrupt', { path: deps.path, quarantinedAs }, { cause: error })
  } catch (cause) {
    log.error('quarantena del database corrotto non riuscita', cause, { path: deps.path })
    return error
  }
}
