/**
 * Catalogo degli errori: l'unica fonte di verità.
 *
 * Nel legacy la stessa informazione viveva in tre posti che sono già divergiti:
 * i siti che producevano il codice (`download/errors.ts`, `binaries.ts`, …), le
 * tabelle `SIMPLE_CODES`/`PARAM_CODES` in `src/lib/ipcError.ts`, e le chiavi
 * `errors.*` negli `i18n/*.json`. Risultato: 8 chiavi i18n orfane sul desktop e
 * `classifyDownloadFailure` con default 'permanent' sul desktop e 'transient'
 * sul mobile.
 *
 * Qui il codice dichiara dominio, gravità, ritentabilità e chiave i18n una volta
 * sola. Le chiavi i18n si DERIVANO dal catalogo (vedi i18nKeys.ts) e un test
 * fallisce se i file di traduzione non le coprono tutte: la deriva non può più
 * accadere in silenzio.
 *
 * Aggiungere un errore = una riga in `ErrorParams` e una in `CATALOG`. Se ne
 * dimentichi una, TypeScript se ne accorge: CATALOG è un Record esaustivo su
 * ErrorCode.
 */

/** Raggruppamento per area di responsabilità: guida il routing dei log e la UI. */
export type ErrorDomain =
  | 'net'
  | 'db'
  | 'fs'
  | 'playback'
  | 'library'
  | 'metadata'
  | 'download'
  | 'skin'
  | 'transfer'
  | 'sync'
  | 'settings'
  | 'ipc'
  | 'internal'

/**
 * `warning` = l'operazione è fallita ma l'app è integra (il caso normale).
 * `error`   = una funzionalità è compromessa finché non si interviene.
 * `fatal`   = lo stato del processo non è più affidabile; tocca al supervisor.
 * `info`    = esito atteso, non un guasto (es. "nessun risultato").
 */
export type ErrorSeverity = 'info' | 'warning' | 'error' | 'fatal'

/** Comodo per i codici senza parametri, senza usare il tipo `{}`. */
export type NoParams = Record<never, never>

/**
 * Parametri per codice. Sono i dati che sopravvivono alla serializzazione e che
 * l'i18n interpola: se un messaggio all'utente dice "manca yt-dlp", il nome del
 * binario deve essere un parametro, non concatenato nel messaggio.
 */
export interface ErrorParams {
  // ── net ───────────────────────────────────────────────────────────────────
  // Generalizzano le classi di electron/modules/net/errors.ts, che nel legacy
  // erano ottime ma morivano al confine IPC e non uscivano da net/+download/.
  'net.offline': { url?: string }
  'net.timeout': { url?: string; timeoutMs?: number }
  'net.http': { status: number; url?: string; body?: string }
  'net.rateLimited': { service?: string; retryAfterMs?: number }
  'net.badSchema': { service?: string; detail?: string }
  'net.circuitOpen': { service: string; retryAfterMs?: number }

  // ── db ────────────────────────────────────────────────────────────────────
  'db.openFailed': { path?: string }
  'db.corrupt': { path?: string; quarantinedAs?: string }
  'db.migrationFailed': { from: number; to: number }
  /** DB scritto da una versione più nuova dell'app: migrare indietro non si può. */
  'db.versionAhead': { dbVersion: number; appVersion: number }
  'db.locked': NoParams
  'db.queryFailed': { detail?: string }

  // ── fs ────────────────────────────────────────────────────────────────────
  'fs.notFound': { path: string }
  'fs.permissionDenied': { path: string }
  'fs.diskFull': { path?: string }
  'fs.inUse': { path: string }
  'fs.pathInvalid': { path: string }
  'fs.readFailed': { path: string; detail?: string }
  'fs.writeFailed': { path: string; detail?: string }

  // ── playback ──────────────────────────────────────────────────────────────
  // Nel legacy TUTTO questo arrivava come un'unica stringa opaca:
  // Howler passa un codice numerico (in UI si vedeva "2") e
  // PlaybackException.errorCode di ExoPlayer veniva scartato.
  'playback.decodeFailed': { trackId?: number; format?: string }
  'playback.sourceUnavailable': { trackId?: number; path?: string }
  'playback.formatUnsupported': { format?: string }
  'playback.deviceLost': NoParams
  'playback.autoplayBlocked': NoParams
  'playback.stalled': { trackId?: number; positionMs?: number }
  'playback.engineUnavailable': NoParams

  // ── library ───────────────────────────────────────────────────────────────
  'library.scanFailed': { path?: string; detail?: string }
  'library.trackNotFound': { trackId?: number }
  'library.smartRulesInvalid': NoParams
  'library.smartFieldInvalid': { value: string }
  'library.smartOpInvalid': { value: string }

  // ── metadata ──────────────────────────────────────────────────────────────
  'metadata.tagReadFailed': { path?: string }
  'metadata.tagWriteFailed': { path?: string; detail?: string }
  'metadata.tagVerifyFailed': { fields: string }
  'metadata.enrichNoMatch': NoParams
  'metadata.enrichNeedsReview': NoParams
  'metadata.enrichFound': { what: string }
  'metadata.musicbrainzUnavailable': NoParams
  'metadata.fingerprintUnavailable': NoParams

  // ── download ──────────────────────────────────────────────────────────────
  'download.unrecognizedUrl': NoParams
  'download.invalidUrl': NoParams
  'download.ageRestricted': NoParams
  'download.private': NoParams
  'download.unavailable': NoParams
  'download.rateLimited': NoParams
  'download.rateLimitedRetry': NoParams
  'download.forbidden': NoParams
  'download.network': NoParams
  'download.failed': NoParams
  'download.noResults': NoParams
  'download.invalidFiles': NoParams
  'download.ytdlpTimeout': NoParams
  'download.ytdlpBadResponse': NoParams
  'download.ytdlpCorrupted': NoParams
  'download.ytdlpBusy': NoParams
  'download.ytError': { detail: string }
  'download.spotdlExit': { code: string }
  'download.externalSearchFailed': NoParams
  /** `dir` e `url` servono a dire all'utente dove mettere il binario e dove prenderlo. */
  'download.binaryMissing': { name: string; dir?: string; url?: string }

  // ── skin ──────────────────────────────────────────────────────────────────
  'skin.manifestInvalid': { detail?: string }
  'skin.formatUnsupported': { found: number; supported: number }
  'skin.packageCorrupt': { detail?: string }
  'skin.assetRejected': { asset: string; reason: string }
  'skin.tooLarge': { bytes: number; limitBytes: number }
  'skin.idConflict': { id: string }
  'skin.unknownEffect': { type: string }
  'skin.tokenInvalid': { token: string; value: string }
  'skin.notFound': { id: string }
  'skin.builtinReadOnly': { id: string }

  // ── transfer (accoppiamento e trasporto LAN PC↔telefono) ──────────────────
  'transfer.pairingExpired': NoParams
  'transfer.pinInvalid': { attemptsLeft?: number }
  'transfer.pairingRateLimited': { retryAfterMs?: number }
  'transfer.peerUnreachable': { host?: string }
  'transfer.peerNotPaired': NoParams
  'transfer.sessionBusy': NoParams
  'transfer.aborted': { reason?: string }
  'transfer.integrityMismatch': { expected: string; actual: string }
  'transfer.methodNotSupported': { method: string }

  // ── sync (Google Drive) ───────────────────────────────────────────────────
  'sync.authExpired': NoParams
  'sync.remoteCorrupt': NoParams
  'sync.conflict': { detail?: string }

  // ── settings / segreti ────────────────────────────────────────────────────
  'settings.corrupt': { quarantinedAs?: string }
  'settings.secretUnavailable': { key: string }
  'settings.lastfmNotConfigured': NoParams
  'settings.lastfmNoPendingToken': NoParams
  'settings.spotifyAuthFailed': { status: string }

  // ── ipc ───────────────────────────────────────────────────────────────────
  'ipc.handlerMissing': { channel: string }
  'ipc.backendUnreachable': NoParams
  'ipc.payloadInvalid': { channel: string; detail?: string }

  // ── internal ──────────────────────────────────────────────────────────────
  /**
   * Il catch-all. Esiste perché la promessa dell'engine è che QUALSIASI errore
   * abbia una rappresentazione valida: `AppError.from(x)` non fallisce mai e non
   * restituisce mai una stringa nuda. Se questo codice compare nei log, è un
   * candidato a diventare un codice proprio.
   */
  'internal.unexpected': { detail?: string }
  'internal.notImplemented': { what: string }
  'internal.invariantViolated': { what: string }
}

export type ErrorCode = keyof ErrorParams

export interface ErrorMeta {
  domain: ErrorDomain
  severity: ErrorSeverity
  /**
   * Se ritentare ha senso. Può dipendere dai parametri: un 404 non si ritenta,
   * un 503 sì. Viene valutato UNA volta alla costruzione e il booleano finisce
   * nel payload, così sopravvive alla serializzazione come dato.
   */
  retryable: boolean | ((params: Record<string, unknown>) => boolean)
  /**
   * Il codice stringa che il legacy mandava sul filo, dove esisteva. Serve a
   * leggere i dati già persistiti: le righe di download in SQLite contengono
   * questi codici, e senza mappa diventerebbero errori sconosciuti dopo la
   * migrazione. Vedi legacyCodeToErrorCode().
   */
  legacy?: string
}

/** Un 5xx o un 429 vale un altro tentativo; un 4xx "colpa nostra" no. */
function httpRetryable(params: Record<string, unknown>): boolean {
  const status = typeof params['status'] === 'number' ? params['status'] : 0
  return status === 408 || status === 429 || status >= 500
}

export const CATALOG: Record<ErrorCode, ErrorMeta> = {
  // ── net ───────────────────────────────────────────────────────────────────
  'net.offline': { domain: 'net', severity: 'warning', retryable: true },
  'net.timeout': { domain: 'net', severity: 'warning', retryable: true },
  'net.http': { domain: 'net', severity: 'warning', retryable: httpRetryable },
  'net.rateLimited': { domain: 'net', severity: 'warning', retryable: true },
  'net.badSchema': { domain: 'net', severity: 'error', retryable: false },
  'net.circuitOpen': { domain: 'net', severity: 'warning', retryable: true },

  // ── db ────────────────────────────────────────────────────────────────────
  // fatal: senza database l'app non ha una libreria. Non blocca però la
  // registrazione degli handler IPC — era il bug per cui il renderer restava
  // appeso sugli scheletri per sempre.
  'db.openFailed': { domain: 'db', severity: 'fatal', retryable: false },
  'db.corrupt': { domain: 'db', severity: 'fatal', retryable: false },
  'db.migrationFailed': { domain: 'db', severity: 'fatal', retryable: false },
  'db.versionAhead': { domain: 'db', severity: 'fatal', retryable: false },
  'db.locked': { domain: 'db', severity: 'warning', retryable: true },
  'db.queryFailed': { domain: 'db', severity: 'error', retryable: false },

  // ── fs ────────────────────────────────────────────────────────────────────
  'fs.notFound': { domain: 'fs', severity: 'warning', retryable: false },
  'fs.permissionDenied': { domain: 'fs', severity: 'error', retryable: false },
  'fs.diskFull': { domain: 'fs', severity: 'error', retryable: false },
  'fs.inUse': { domain: 'fs', severity: 'warning', retryable: true },
  'fs.pathInvalid': { domain: 'fs', severity: 'error', retryable: false },
  'fs.readFailed': { domain: 'fs', severity: 'warning', retryable: true },
  'fs.writeFailed': { domain: 'fs', severity: 'error', retryable: true },

  // ── playback ──────────────────────────────────────────────────────────────
  // Ritentare la stessa traccia con lo stesso decoder non cambia esito: la
  // macchina a stati reagisce saltando la traccia, non riprovandola.
  'playback.decodeFailed': { domain: 'playback', severity: 'warning', retryable: false },
  'playback.sourceUnavailable': { domain: 'playback', severity: 'warning', retryable: false },
  'playback.formatUnsupported': { domain: 'playback', severity: 'warning', retryable: false },
  'playback.deviceLost': { domain: 'playback', severity: 'error', retryable: true },
  'playback.autoplayBlocked': { domain: 'playback', severity: 'info', retryable: true },
  'playback.stalled': { domain: 'playback', severity: 'warning', retryable: true },
  'playback.engineUnavailable': { domain: 'playback', severity: 'fatal', retryable: false },

  // ── library ───────────────────────────────────────────────────────────────
  'library.scanFailed': { domain: 'library', severity: 'warning', retryable: true },
  'library.trackNotFound': {
    domain: 'library',
    severity: 'warning',
    retryable: false,
    legacy: 'TRACK_NOT_FOUND'
  },
  'library.smartRulesInvalid': {
    domain: 'library',
    severity: 'warning',
    retryable: false,
    legacy: 'SMART_RULES_INVALID'
  },
  'library.smartFieldInvalid': {
    domain: 'library',
    severity: 'warning',
    retryable: false,
    legacy: 'SMART_FIELD_INVALID'
  },
  'library.smartOpInvalid': {
    domain: 'library',
    severity: 'warning',
    retryable: false,
    legacy: 'SMART_OP_INVALID'
  },

  // ── metadata ──────────────────────────────────────────────────────────────
  'metadata.tagReadFailed': { domain: 'metadata', severity: 'warning', retryable: true },
  'metadata.tagWriteFailed': { domain: 'metadata', severity: 'error', retryable: true },
  'metadata.tagVerifyFailed': {
    domain: 'metadata',
    severity: 'error',
    retryable: false,
    legacy: 'TAG_VERIFY_FAILED'
  },
  'metadata.enrichNoMatch': {
    domain: 'metadata',
    severity: 'info',
    retryable: false,
    legacy: 'ENRICH_NO_MATCH'
  },
  'metadata.enrichNeedsReview': {
    domain: 'metadata',
    severity: 'info',
    retryable: false,
    legacy: 'ENRICH_NEEDS_REVIEW'
  },
  'metadata.enrichFound': {
    domain: 'metadata',
    severity: 'info',
    retryable: false,
    legacy: 'ENRICH_FOUND'
  },
  'metadata.musicbrainzUnavailable': {
    domain: 'metadata',
    severity: 'warning',
    retryable: true,
    legacy: 'ENRICH_MB_UNAVAILABLE'
  },
  'metadata.fingerprintUnavailable': {
    domain: 'metadata',
    severity: 'info',
    retryable: false
  },

  // ── download ──────────────────────────────────────────────────────────────
  // La ritentabilità qui SOSTITUISCE classifyDownloadFailure, che nel legacy
  // aveva default divergenti fra desktop ('permanent') e mobile ('transient').
  // Il default è ora esplicito per ogni codice, una volta sola.
  'download.unrecognizedUrl': {
    domain: 'download', severity: 'warning', retryable: false, legacy: 'DL_UNRECOGNIZED_URL'
  },
  'download.invalidUrl': {
    domain: 'download', severity: 'warning', retryable: false, legacy: 'DL_INVALID_URL'
  },
  'download.ageRestricted': {
    domain: 'download', severity: 'warning', retryable: false, legacy: 'DL_AGE_RESTRICTED'
  },
  'download.private': {
    domain: 'download', severity: 'warning', retryable: false, legacy: 'DL_PRIVATE'
  },
  'download.unavailable': {
    domain: 'download', severity: 'warning', retryable: false, legacy: 'DL_UNAVAILABLE'
  },
  'download.rateLimited': {
    domain: 'download', severity: 'warning', retryable: true, legacy: 'DL_RATE_LIMITED'
  },
  'download.rateLimitedRetry': {
    domain: 'download', severity: 'info', retryable: true, legacy: 'DL_RATE_LIMITED_RETRY'
  },
  'download.forbidden': {
    domain: 'download', severity: 'warning', retryable: false, legacy: 'DL_FORBIDDEN'
  },
  'download.network': {
    domain: 'download', severity: 'warning', retryable: true, legacy: 'DL_NETWORK'
  },
  'download.failed': {
    domain: 'download', severity: 'warning', retryable: true, legacy: 'DL_FAILED'
  },
  'download.noResults': {
    domain: 'download', severity: 'info', retryable: false, legacy: 'DL_NO_RESULTS'
  },
  'download.invalidFiles': {
    domain: 'download', severity: 'warning', retryable: false, legacy: 'DL_INVALID_FILES'
  },
  'download.ytdlpTimeout': {
    domain: 'download', severity: 'warning', retryable: true, legacy: 'DL_YTDLP_TIMEOUT'
  },
  'download.ytdlpBadResponse': {
    domain: 'download', severity: 'warning', retryable: true, legacy: 'DL_YTDLP_BAD_RESPONSE'
  },
  'download.ytdlpCorrupted': {
    domain: 'download', severity: 'error', retryable: false, legacy: 'YTDLP_CORRUPTED'
  },
  'download.ytdlpBusy': {
    domain: 'download', severity: 'info', retryable: true, legacy: 'YTDLP_BUSY'
  },
  'download.ytError': {
    domain: 'download', severity: 'warning', retryable: true, legacy: 'DL_YT_ERROR'
  },
  'download.spotdlExit': {
    domain: 'download', severity: 'warning', retryable: true, legacy: 'DL_SPOTDL_EXIT'
  },
  'download.externalSearchFailed': {
    domain: 'download', severity: 'warning', retryable: true, legacy: 'EXT_SEARCH_FAILED'
  },
  'download.binaryMissing': {
    domain: 'download', severity: 'error', retryable: false, legacy: 'BINARY_MISSING'
  },

  // ── skin ──────────────────────────────────────────────────────────────────
  // Nessuno di questi è ritentabile: un pacchetto non valido resta non valido.
  // Portano invece un `detail` preciso, perché chi crea una skin deve sapere
  // COSA rifiutare, non solo che è stato rifiutato.
  'skin.manifestInvalid': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.formatUnsupported': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.packageCorrupt': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.assetRejected': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.tooLarge': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.idConflict': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.unknownEffect': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.tokenInvalid': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.notFound': { domain: 'skin', severity: 'warning', retryable: false },
  'skin.builtinReadOnly': { domain: 'skin', severity: 'info', retryable: false },

  // ── transfer ──────────────────────────────────────────────────────────────
  'transfer.pairingExpired': { domain: 'transfer', severity: 'info', retryable: false },
  'transfer.pinInvalid': { domain: 'transfer', severity: 'warning', retryable: false },
  'transfer.pairingRateLimited': { domain: 'transfer', severity: 'warning', retryable: true },
  'transfer.peerUnreachable': { domain: 'transfer', severity: 'warning', retryable: true },
  'transfer.peerNotPaired': { domain: 'transfer', severity: 'warning', retryable: false },
  'transfer.sessionBusy': { domain: 'transfer', severity: 'info', retryable: true },
  'transfer.aborted': { domain: 'transfer', severity: 'warning', retryable: true },
  'transfer.integrityMismatch': { domain: 'transfer', severity: 'error', retryable: true },
  'transfer.methodNotSupported': {
    domain: 'transfer',
    severity: 'info',
    retryable: false,
    legacy: 'LAN_METHOD_NOT_SUPPORTED'
  },

  // ── sync ──────────────────────────────────────────────────────────────────
  'sync.authExpired': { domain: 'sync', severity: 'warning', retryable: false },
  'sync.remoteCorrupt': { domain: 'sync', severity: 'error', retryable: false },
  'sync.conflict': { domain: 'sync', severity: 'warning', retryable: false },

  // ── settings ──────────────────────────────────────────────────────────────
  'settings.corrupt': { domain: 'settings', severity: 'error', retryable: false },
  'settings.secretUnavailable': { domain: 'settings', severity: 'warning', retryable: false },
  'settings.lastfmNotConfigured': {
    domain: 'settings', severity: 'info', retryable: false, legacy: 'LASTFM_NOT_CONFIGURED'
  },
  'settings.lastfmNoPendingToken': {
    domain: 'settings', severity: 'info', retryable: false, legacy: 'LASTFM_NO_PENDING_TOKEN'
  },
  'settings.spotifyAuthFailed': {
    domain: 'settings', severity: 'warning', retryable: true, legacy: 'SPOTIFY_AUTH_FAILED'
  },

  // ── ipc ───────────────────────────────────────────────────────────────────
  'ipc.handlerMissing': { domain: 'ipc', severity: 'error', retryable: false },
  'ipc.backendUnreachable': {
    domain: 'ipc',
    severity: 'error',
    retryable: true,
    legacy: 'BACKEND_UNREACHABLE'
  },
  'ipc.payloadInvalid': { domain: 'ipc', severity: 'error', retryable: false },

  // ── internal ──────────────────────────────────────────────────────────────
  'internal.unexpected': { domain: 'internal', severity: 'error', retryable: false },
  'internal.notImplemented': { domain: 'internal', severity: 'error', retryable: false },
  'internal.invariantViolated': { domain: 'internal', severity: 'fatal', retryable: false }
}

/** Tutti i codici, utile per i test di esaustività e per generare l'i18n. */
export const ERROR_CODES = Object.keys(CATALOG) as ErrorCode[]

/**
 * Mappa inversa dai codici stringa del legacy. Serve a leggere i dati già
 * persistiti (le righe di download in SQLite contengono il vecchio codice):
 * senza questa, dopo la migrazione diventerebbero errori sconosciuti.
 */
const LEGACY_TO_CODE: Record<string, ErrorCode> = (() => {
  const map: Record<string, ErrorCode> = {}
  for (const code of ERROR_CODES) {
    const legacy = CATALOG[code].legacy
    if (legacy !== undefined) map[legacy] = code
  }
  return map
})()

/**
 * Traduce un codice legacy nel codice nuovo. I codici a parametro viaggiavano
 * come `PREFISSO:payload` (es. `BINARY_MISSING:yt-dlp:resources/bin`), quindi si
 * guarda anche solo la parte prima dei due punti.
 */
export function legacyCodeToErrorCode(raw: string): ErrorCode | undefined {
  const direct = LEGACY_TO_CODE[raw]
  if (direct) return direct
  const sep = raw.indexOf(':')
  if (sep > 0) return LEGACY_TO_CODE[raw.slice(0, sep)]
  return undefined
}
