/**
 * Accesso al database.
 *
 * Tre pezzi, con tre responsabilità separate — nel legacy erano un unico file di
 * 591 righe che importava `electron` e `better-sqlite3` e non era provabile senza
 * entrambi:
 *
 *   driver.ts   la superficie SQLite come interfaccia, uguale per i tre driver
 *   migrate.ts  la catena, il piano e le guardie. Nessun I/O oltre al DB
 *   open.ts     l'orchestrazione, e la promessa di non bloccare mai l'avvio
 */

export {
  execute,
  queryAll,
  queryOne,
  sqlError,
  type Db,
  type SqlParams,
  type SqlRow,
  type SqlRunInfo,
  type SqlStatement,
  type SqlValue,
  type SqliteDriver
} from './driver'

export {
  BASELINE_VERSION,
  LEGACY_FINAL_VERSION,
  latestVersion,
  migrate,
  planMigration,
  validateChain,
  type Chain,
  type LegacyHistoryName,
  type Migration,
  type MigrateDeps,
  type MigrateOutcome,
  type MigrationContext,
  type MigrationFiles,
  type MigrationPlan
} from './migrate'

export { createDbHandle, type DbHandle, type DbStatus, type OpenDbDeps } from './open'

export {
  AETHER_CHAIN,
  ANDROID_HISTORY,
  BASELINE,
  DESKTOP_HISTORY,
  UNIFIED
} from './migrations'
