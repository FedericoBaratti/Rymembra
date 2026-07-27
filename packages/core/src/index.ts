/**
 * Punto d'ingresso pubblico di @aether/core.
 *
 * Cosa c'è, e in che ordine dipende:
 *
 *   result       — l'esito come valore. Non dipende da nulla.
 *   errors       — catalogo, AppError, chiavi i18n derivate.
 *   resilience   — ritentare, arrendersi, rallentare, scadere.
 *   serialize    — la busta che attraversa l'IPC senza perdite.
 *   contract     — canali, tipi, validazione: una sola fonte.
 *   logger       — livelli, campi, stack, ring buffer, sink iniettati.
 *   supervisor   — reti di sicurezza di processo e flush di chiusura guardati.
 *
 * `AetherAPI` è DERIVATA dal contratto (`ApiFor<typeof CONTRACT>`), non scritta a
 * mano: nel legacy erano ~200 righe in `shared/types.ts:651` più un cast
 * `as unknown as` che non garantiva nulla. Il contratto dei canali reali nasce
 * nella Fase 2, insieme ai moduli di dominio che li implementano.
 *
 * Nota sui sottoinsiemi: `@aether/core/shared` è l'unico sottoinsieme che il
 * renderer può importare senza un adapter di piattaforma. Il resto di questo
 * indice è isomorfo per costruzione (nessun `window`, nessun `require` di moduli
 * Node), ma presuppone che qualcuno abbia configurato i sink del logger.
 */

export {
  ok,
  err,
  isOk,
  isErr,
  map,
  mapErr,
  andThen,
  unwrapOr,
  all,
  partition,
  type Ok,
  type Err,
  type Result
} from './result'

export {
  AppError,
  CATALOG,
  ERROR_CODES,
  i18nKeyFor,
  isAppError,
  isAppErrorPayload,
  legacyCodeToErrorCode,
  missingI18nKeys,
  requiredI18nKeys,
  type AppErrorOptions,
  type AppErrorPayload,
  type CauseInfo,
  type ErrorCode,
  type ErrorDomain,
  type ErrorMeta,
  type ErrorParams,
  type ErrorSeverity,
  type NoParams
} from './errors'

export {
  CircuitBreaker,
  MAX_ATTEMPTS,
  RATE_LIMIT_PAUSE_MS,
  RETRY_BASE_MS,
  RateLimiter,
  abortError,
  abortedIfSignalled,
  createDeadline,
  decideRetry,
  nextDelayMs,
  normalizeDownloadError,
  retryThrowing,
  runFallible,
  sleep,
  withDeadline,
  withRetry,
  type CircuitBreakerOptions,
  type CircuitState,
  type Deadline,
  type DeadlineOptions,
  type DownloadFailureClass,
  type Fallible,
  type RateLimiterOptions,
  type RetryDecision,
  type RetryOptions
} from './resilience'

export {
  NO_CAPABILITIES,
  defineCapabilities,
  type AppearanceCapabilities,
  type CapabilityOverrides,
  type Capabilities,
  type NetworkCapabilities,
  type PlaybackCapabilities,
  type SearchCapabilities,
  type SystemCapabilities
} from './capabilities'

export {
  AETHER_CHAIN,
  ANDROID_HISTORY,
  BASELINE,
  BASELINE_VERSION,
  DESKTOP_HISTORY,
  LEGACY_FINAL_VERSION,
  UNIFIED,
  createDbHandle,
  execute,
  latestVersion,
  migrate,
  planMigration,
  queryAll,
  queryOne,
  sqlError,
  validateChain,
  type Chain,
  type Db,
  type DbHandle,
  type DbStatus,
  type LegacyHistoryName,
  type MigrateDeps,
  type MigrateOutcome,
  type Migration,
  type MigrationContext,
  type MigrationFiles,
  type MigrationPlan,
  type OpenDbDeps,
  type SqlParams,
  type SqlRow,
  type SqlRunInfo,
  type SqlStatement,
  type SqlValue,
  type SqliteDriver
} from './db'

export {
  errorEnvelope,
  fromEnvelope,
  toEnvelope,
  unwrapEnvelope,
  withIpcTimeout,
  wrapHandler,
  type HandlerReturn,
  type IpcEnvelope
} from './serialize'

export {
  assertHandlersComplete,
  channel,
  channelNames,
  defineHandlers,
  type ApiFor,
  type BoundHandler,
  type ChannelDef,
  type Contract,
  type EventContract,
  type EventEmitter,
  type EventListener,
  type EventPayload,
  type HandlerContext,
  type HandlerMap,
  type InputOf,
  type OutputOf
} from './contract'

export {
  LOG_LEVELS,
  addLogSink,
  clearRecentLogs,
  configureLogger,
  createConsoleSink,
  createMemorySink,
  flushLogs,
  formatLogLine,
  logger,
  recentLogs,
  type LogLevel,
  type LogRecord,
  type LogScope,
  type LogSink,
  type LoggerConfig,
  type ScopedLogger
} from './logger'

export {
  createSupervisor,
  type FlushOutcome,
  type FlushStep,
  type Supervisor,
  type SupervisorDeps,
  type SupervisorEvent,
  type SupervisorTrigger
} from './supervisor'

/**
 * Superficie che il renderer vede come `window.aether`.
 *
 * Ancora un segnaposto: diventa `ApiFor<typeof CONTRACT>` quando i moduli di
 * dominio della Fase 2 dichiarano i canali reali. Fino a quel momento serve ai
 * test di `packages/ui`, che montano un finto `window.aether`.
 */
export type AetherAPI = Record<string, (...args: never[]) => unknown>
