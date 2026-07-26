/**
 * AppError — un solo tipo di errore, il cui contenuto è dato puro.
 *
 * Nel legacy c'erano dieci sottoclassi di Error con campi ricchi
 * (`HttpError.status`, `RateLimitError.retryAfterMs`, `DownloadError.failureClass`,
 * `CircuitOpenError.service`, …) e nessuna sopravviveva al confine IPC: sia
 * `ipc/handle.ts` sia il bridge mobile ricostruivano `new Error(messaggio)`, e
 * poi il renderer indovinava l'identità con una regex sul testo.
 *
 * La lezione è che l'errore non deve essere una GERARCHIA DI CLASSI, deve essere
 * un RECORD. Una classe sola, un payload serializzabile, e il comportamento
 * (dominio, gravità, ritentabilità, chiave i18n) letto dal catalogo. Così
 * `toPayload()`/`fromPayload()` è senza perdite e nulla va indovinato.
 *
 * AppError resta comunque una sottoclasse di Error: si può lanciare, ha uno
 * stack, e i debugger la mostrano come si aspettano.
 */

import {
  CATALOG,
  legacyCodeToErrorCode,
  type ErrorCode,
  type ErrorDomain,
  type ErrorParams,
  type ErrorSeverity
} from './catalog'

/** Un anello della catena delle cause, già appiattito e serializzabile. */
export interface CauseInfo {
  /** Il codice, se la causa era a sua volta un AppError. */
  code?: ErrorCode
  message: string
  /** `code` di Node (ENOENT, EACCES…) o `name` della classe originale. */
  kind?: string
  stack?: string
}

/** La forma che attraversa l'IPC. Solo JSON: niente classi, niente prototipi. */
export interface AppErrorPayload {
  readonly __aetherError: true
  readonly code: ErrorCode
  readonly domain: ErrorDomain
  readonly severity: ErrorSeverity
  readonly retryable: boolean
  readonly params: Record<string, unknown>
  readonly i18nKey: string
  readonly message: string
  readonly traceId: string
  readonly context?: Record<string, unknown>
  readonly causes?: readonly CauseInfo[]
  readonly stack?: string
}

export interface AppErrorOptions {
  /** L'errore sottostante, di qualunque forma: viene appiattito nella catena. */
  cause?: unknown
  /** Dati diagnostici per i log. NON vanno mostrati all'utente. */
  context?: Record<string, unknown>
  /** Per correlare più errori alla stessa operazione. Altrimenti generato. */
  traceId?: string
}

/** Profondità massima della catena: protegge da cause circolari. */
const MAX_CAUSE_DEPTH = 8

let traceCounter = 0

/**
 * Identificatore di correlazione. Non serve unicità globale né
 * imprevedibilità — serve a legare fra loro le righe di log e il messaggio
 * mostrato all'utente. `crypto.randomUUID` non esiste su Node 12 (backend
 * nodejs-mobile), quindi contatore più casuale.
 */
function newTraceId(): string {
  traceCounter = (traceCounter + 1) % 0xffff
  const rand = Math.floor(Math.random() * 0xffffff).toString(16).padStart(6, '0')
  return `${Date.now().toString(36)}-${traceCounter.toString(16)}-${rand}`
}

/** La chiave i18n si deriva dal codice: `net.http` → `errors.net.http`. */
export function i18nKeyFor(code: ErrorCode): string {
  return `errors.${code}`
}

/**
 * Errori errno di Node e di Android mappati sul dominio fs.
 *
 * Vale la pena averla: nel legacy un ENOSPC in scrittura tag diventava un
 * `logWarn` e poi una stringa generica in UI. Qui diventa `fs.diskFull`, che
 * porta con sé gravità, ritentabilità e un messaggio utile all'utente senza che
 * nessun sito di chiamata debba ricordarsene.
 */
const ERRNO_TO_CODE: Record<string, ErrorCode> = {
  ENOENT: 'fs.notFound',
  EACCES: 'fs.permissionDenied',
  EPERM: 'fs.permissionDenied',
  ENOSPC: 'fs.diskFull',
  EDQUOT: 'fs.diskFull',
  EBUSY: 'fs.inUse',
  ETXTBSY: 'fs.inUse',
  EISDIR: 'fs.pathInvalid',
  ENOTDIR: 'fs.pathInvalid',
  ENAMETOOLONG: 'fs.pathInvalid',
  EIO: 'fs.readFailed',
  // Rete a livello di socket: sono i codici che il polyfill fetch del backend
  // mobile fa emergere quando non c'è connettività.
  ENOTFOUND: 'net.offline',
  ECONNREFUSED: 'net.offline',
  ENETUNREACH: 'net.offline',
  EHOSTUNREACH: 'net.offline',
  ECONNRESET: 'net.offline',
  EPIPE: 'net.offline',
  ETIMEDOUT: 'net.timeout',
  EAI_AGAIN: 'net.offline'
}

/**
 * Se il valore è già un payload serializzato di AppError.
 * Riconoscerlo è ciò che rende il round-trip attraverso l'IPC senza perdite.
 */
export function isAppErrorPayload(value: unknown): value is AppErrorPayload {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as { __aetherError?: unknown }).__aetherError === true &&
    typeof (value as { code?: unknown }).code === 'string'
  )
}

function readErrno(value: unknown): string | undefined {
  if (typeof value !== 'object' || value === null) return undefined
  const code = (value as { code?: unknown }).code
  return typeof code === 'string' ? code : undefined
}

/** Appiattisce la catena delle cause, con tetto di profondità. */
function flattenCauses(cause: unknown, depth = 0): CauseInfo[] {
  if (cause === undefined || cause === null || depth >= MAX_CAUSE_DEPTH) return []

  if (cause instanceof AppError) {
    const head: CauseInfo = {
      code: cause.code,
      message: cause.message,
      kind: 'AppError',
      ...(cause.stack !== undefined ? { stack: cause.stack } : {})
    }
    return [head, ...cause.causes.slice(0, MAX_CAUSE_DEPTH - depth - 1)]
  }

  if (cause instanceof Error) {
    const errno = readErrno(cause)
    const head: CauseInfo = {
      message: cause.message,
      kind: errno ?? cause.name,
      ...(cause.stack !== undefined ? { stack: cause.stack } : {})
    }
    // Error.cause è ES2022: il backend mobile gira su Node 12, dove non esiste.
    // Si legge in modo difensivo perché a volte c'è (desktop) e a volte no.
    const nested = (cause as { cause?: unknown }).cause
    return [head, ...flattenCauses(nested, depth + 1)]
  }

  return [{ message: typeof cause === 'string' ? cause : safeStringify(cause) }]
}

/** Stringifica senza mai lanciare: i cicli e i BigInt non devono far cadere il log. */
function safeStringify(value: unknown): string {
  if (typeof value === 'string') return value
  if (value === undefined) return 'undefined'
  if (value === null) return 'null'
  if (typeof value === 'bigint') return `${value.toString()}n`
  if (typeof value !== 'object') return String(value)
  try {
    const seen = new WeakSet<object>()
    return JSON.stringify(value, (_k, v) => {
      if (typeof v === 'bigint') return `${v.toString()}n`
      if (typeof v === 'object' && v !== null) {
        if (seen.has(v as object)) return '[Circular]'
        seen.add(v as object)
      }
      return v
    }) ?? '[unserializable]'
  } catch {
    return '[unserializable]'
  }
}

/** Messaggio per gli sviluppatori: `[net.http] status=429 url=…`. */
function devMessage(code: ErrorCode, params: Record<string, unknown>): string {
  const parts: string[] = []
  for (const key of Object.keys(params)) {
    const value = params[key]
    if (value === undefined) continue
    const text = typeof value === 'string' ? value : safeStringify(value)
    parts.push(`${key}=${text.length > 120 ? `${text.slice(0, 117)}...` : text}`)
  }
  return parts.length > 0 ? `[${code}] ${parts.join(' ')}` : `[${code}]`
}

/**
 * `params` va tipizzato per codice, ma per i codici senza parametri obbligare a
 * passare `{}` a ogni sito di chiamata sarebbe solo rumore. La tupla
 * condizionale rende l'argomento opzionale quando il tipo dei parametri è vuoto,
 * e obbligatorio quando ci sono campi richiesti.
 */
type ParamsArg<C extends ErrorCode> = Record<never, never> extends ErrorParams[C]
  ? [params?: ErrorParams[C], options?: AppErrorOptions]
  : [params: ErrorParams[C], options?: AppErrorOptions]

export class AppError extends Error {
  readonly code: ErrorCode
  readonly domain: ErrorDomain
  readonly severity: ErrorSeverity
  readonly retryable: boolean
  readonly params: Record<string, unknown>
  readonly context: Record<string, unknown> | undefined
  readonly traceId: string
  readonly causes: readonly CauseInfo[]

  /**
   * Privato di fatto: si costruisce con `of()` (codice noto, parametri
   * tipizzati) o con `from()` (qualunque valore). Così non esiste un percorso
   * che produca un AppError senza voce di catalogo.
   */
  private constructor(
    code: ErrorCode,
    params: Record<string, unknown>,
    options: AppErrorOptions | undefined
  ) {
    const meta = CATALOG[code]
    super(devMessage(code, params))
    this.name = 'AppError'
    this.code = code
    this.domain = meta.domain
    this.severity = meta.severity
    this.retryable =
      typeof meta.retryable === 'function' ? meta.retryable(params) : meta.retryable
    this.params = params
    this.context = options?.context
    this.traceId = options?.traceId ?? newTraceId()
    this.causes = flattenCauses(options?.cause)

    // Tiene lo stack pulito: il frame di questo costruttore non interessa.
    // Presente su V8 (Node e Chromium), assente altrove: va guardato.
    const capture = (Error as unknown as {
      captureStackTrace?: (target: object, ctor?: unknown) => void
    }).captureStackTrace
    if (typeof capture === 'function') capture(this, AppError)
  }

  /**
   * Costruisce un errore da un codice del catalogo, con parametri tipizzati.
   *
   *   AppError.of('net.http', { status: 429, url })
   *   AppError.of('download.ageRestricted')
   */
  static of<C extends ErrorCode>(code: C, ...rest: ParamsArg<C>): AppError {
    const [params, options] = rest
    return new AppError(code, (params ?? {}) as Record<string, unknown>, options)
  }

  /**
   * Converte QUALSIASI valore in un AppError. Non lancia e non restituisce mai
   * una stringa nuda: è la promessa che regge tutta l'architettura degli errori.
   *
   * Nell'ordine riconosce: un AppError già pronto; un payload arrivato
   * dall'IPC; un codice stringa del legacy (i dati già in SQLite li contengono);
   * un errno di Node o Android; un Error qualsiasi; e infine qualunque altra
   * cosa (stringhe, numeri, null, oggetti, valori lanciati per sbaglio).
   */
  static from(value: unknown, options?: AppErrorOptions): AppError {
    if (value instanceof AppError) {
      return options?.context ? value.withContext(options.context) : value
    }

    if (isAppErrorPayload(value)) return AppError.fromPayload(value)

    // Codici del legacy, sia nudi sia nella forma `PREFISSO:payload`.
    if (typeof value === 'string') {
      const mapped = legacyCodeToErrorCode(value)
      if (mapped) {
        return new AppError(mapped, legacyParams(mapped, value), options)
      }
      return new AppError('internal.unexpected', { detail: value }, options)
    }

    const errno = readErrno(value)
    if (errno !== undefined) {
      const mapped = ERRNO_TO_CODE[errno]
      if (mapped) {
        const path = (value as { path?: unknown }).path
        return new AppError(
          mapped,
          typeof path === 'string' ? { path } : {},
          { ...options, cause: value }
        )
      }
    }

    if (value instanceof Error) {
      // Un messaggio che è un codice legacy capita: il vecchio confine IPC
      // rilanciava `new Error(codice)`.
      const mapped = legacyCodeToErrorCode(value.message)
      if (mapped) {
        return new AppError(mapped, legacyParams(mapped, value.message), {
          ...options,
          cause: value
        })
      }
      return new AppError(
        'internal.unexpected',
        { detail: value.message },
        { ...options, cause: value }
      )
    }

    return new AppError(
      'internal.unexpected',
      { detail: safeStringify(value) },
      options
    )
  }

  /** La chiave i18n del messaggio da mostrare all'utente. */
  get i18nKey(): string {
    return i18nKeyFor(this.code)
  }

  /** Aggiunge contesto diagnostico senza perdere identità né catena. */
  withContext(extra: Record<string, unknown>): AppError {
    const merged = new AppError(this.code, this.params, {
      context: { ...this.context, ...extra },
      traceId: this.traceId
    })
    ;(merged as { causes: readonly CauseInfo[] }).causes = this.causes
    return merged
  }

  /** Forma JSON che attraversa l'IPC senza perdite. */
  toPayload(): AppErrorPayload {
    return {
      __aetherError: true,
      code: this.code,
      domain: this.domain,
      severity: this.severity,
      retryable: this.retryable,
      params: this.params,
      i18nKey: this.i18nKey,
      message: this.message,
      traceId: this.traceId,
      ...(this.context !== undefined ? { context: this.context } : {}),
      ...(this.causes.length > 0 ? { causes: this.causes } : {}),
      ...(this.stack !== undefined ? { stack: this.stack } : {})
    }
  }

  /**
   * Ricostruisce l'errore dall'altra parte del confine.
   *
   * Un codice sconosciuto (payload da una versione più nuova dell'app, o da un
   * telefono accoppiato con build diversa) non fa fallire nulla: degrada a
   * `internal.unexpected` tenendo il codice originale nel contesto, così resta
   * leggibile nei log.
   */
  static fromPayload(payload: AppErrorPayload): AppError {
    const known = Object.prototype.hasOwnProperty.call(CATALOG, payload.code)
    const error = new AppError(
      known ? payload.code : 'internal.unexpected',
      known ? payload.params : { detail: payload.message },
      {
        traceId: payload.traceId,
        ...(payload.context !== undefined || !known
          ? { context: { ...payload.context, ...(known ? {} : { unknownCode: payload.code }) } }
          : {})
      }
    )
    const mutable = error as {
      causes: readonly CauseInfo[]
      stack?: string
      retryable: boolean
    }
    if (payload.causes) mutable.causes = payload.causes
    // Lo stack originale è quello del processo che ha generato l'errore: vale
    // più di quello ricostruito qui.
    if (payload.stack !== undefined) mutable.stack = payload.stack
    // Per i codici noti la ritentabilità si ricalcola dal catalogo (è la fonte
    // di verità); per gli sconosciuti si tiene quella dichiarata dal mittente.
    if (!known) mutable.retryable = payload.retryable
    return error
  }
}

/**
 * Ricostruisce i parametri da un codice legacy nella forma `PREFISSO:payload`.
 *
 * Il legacy codificava i parametri nella stringa: `DL_YT_ERROR:<dettaglio>`,
 * `BINARY_MISSING:<nome>:<cartella>`. Qui si spacchettano nei campi tipizzati,
 * così i dati vecchi entrano nel modello nuovo invece di restare stringhe opache.
 */
function legacyParams(code: ErrorCode, raw: string): Record<string, unknown> {
  const sep = raw.indexOf(':')
  if (sep < 0) return {}
  const payload = raw.slice(sep + 1)

  switch (code) {
    case 'download.binaryMissing': {
      // BINARY_MISSING:<nome>[:<cartella>]
      const cut = payload.indexOf(':')
      const name = cut < 0 ? payload : payload.slice(0, cut)
      const dir = cut < 0 ? '' : payload.slice(cut + 1)
      return dir ? { name, dir } : { name }
    }
    case 'download.ytError':
      return { detail: payload }
    case 'download.spotdlExit':
      return { code: payload }
    case 'settings.spotifyAuthFailed':
      return { status: payload }
    case 'metadata.tagVerifyFailed':
      return { fields: payload }
    case 'metadata.enrichFound':
      return { what: payload }
    case 'library.smartFieldInvalid':
    case 'library.smartOpInvalid':
      return { value: payload }
    default:
      return {}
  }
}

/** Type guard comodo ai confini. */
export function isAppError(value: unknown): value is AppError {
  return value instanceof AppError
}
