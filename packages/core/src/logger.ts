/**
 * Log strutturato, con livelli, e con lo stack preservato.
 *
 * Cosa c'era prima (electron/modules/logger.ts, 58 righe):
 *   - solo logWarn e logError: nessun livello configurabile, nessun debug
 *   - `formatErr` faceva `: ${err.message}` e BUTTAVA LO STACK
 *   - scope stringa libera: 19 valori diversi nati per accumulo
 *   - nessun campo strutturato: tutto concatenato in una riga di testo
 *   - il renderer non aveva alcun logger, solo console.*
 *
 * Qui il logger è isomorfo e non conosce la destinazione: i sink sono iniettati.
 * Così lo stesso codice gira nel main process, nel backend nodejs-mobile (Node
 * 12) e nel renderer, e ogni ambiente decide dove scrivere.
 *
 * Il ring buffer in memoria serve al pannello diagnostico e ai report di crash:
 * quando qualcosa va storto si vogliono le ultime righe PRIMA dell'errore, e
 * rileggere il file di log dal disco è la strada peggiore per averle.
 */

import { AppError, type AppErrorPayload } from './errors'

export const LOG_LEVELS = ['trace', 'debug', 'info', 'warn', 'error', 'fatal'] as const
export type LogLevel = (typeof LOG_LEVELS)[number]

const LEVEL_RANK: Record<LogLevel, number> = {
  trace: 10,
  debug: 20,
  info: 30,
  warn: 40,
  error: 50,
  fatal: 60
}

/**
 * Sottosistema che emette la riga. Enum e non stringa libera, così i filtri sui
 * log sono affidabili e non c'è modo di scrivere 'phonesync' dove il resto del
 * codice scrive 'phoneSync'.
 */
export type LogScope =
  // infrastruttura
  | 'boot' | 'supervisor' | 'ipc' | 'db' | 'settings' | 'secrets' | 'backup' | 'jsonFile'
  // dominio
  | 'library' | 'scan' | 'search' | 'metadata' | 'enrich' | 'cover' | 'lyrics'
  | 'playlists' | 'podcasts' | 'stats' | 'repair'
  // riproduzione
  | 'playback' | 'audio' | 'queue'
  // rete ed esterni
  | 'net' | 'download' | 'spotify' | 'musicbrainz' | 'scrobble' | 'reco' | 'sync'
  // dispositivi e skin
  | 'lan' | 'transfer' | 'phoneSync' | 'skin' | 'studio'
  // piattaforma
  | 'mobile' | 'thermal' | 'auto' | 'renderer'

export interface LogRecord {
  /** Millisecondi epoch: numero e non stringa, così ordinare e filtrare è banale. */
  ts: number
  level: LogLevel
  scope: LogScope
  message: string
  /** Campi strutturati. Nel legacy finivano concatenati nel messaggio. */
  fields?: Record<string, unknown>
  /** L'errore per intero, stack e catena delle cause compresi. */
  error?: AppErrorPayload
  /** Correla le righe alla stessa operazione, e all'errore mostrato all'utente. */
  traceId?: string
}

export interface LogSink {
  write(record: LogRecord): void
  /** Chiamata alla chiusura e dal supervisor: deve essere idempotente. */
  flush?(): void | Promise<void>
}

/** Dimensione del ring buffer: le ultime righe restano in memoria per la diagnostica. */
const RING_CAPACITY = 500

class RingBuffer {
  private readonly items: (LogRecord | undefined)[] = new Array<LogRecord | undefined>(
    RING_CAPACITY
  )
  private next = 0
  private wrapped = false

  push(record: LogRecord): void {
    this.items[this.next] = record
    this.next = (this.next + 1) % RING_CAPACITY
    if (this.next === 0) this.wrapped = true
  }

  /** Le righe dalla più vecchia alla più recente. */
  snapshot(): LogRecord[] {
    const out: LogRecord[] = []
    if (this.wrapped) {
      for (let i = this.next; i < RING_CAPACITY; i++) {
        const item = this.items[i]
        if (item !== undefined) out.push(item)
      }
    }
    for (let i = 0; i < this.next; i++) {
      const item = this.items[i]
      if (item !== undefined) out.push(item)
    }
    return out
  }

  clear(): void {
    this.items.fill(undefined)
    this.next = 0
    this.wrapped = false
  }
}

export interface LoggerConfig {
  /** Sotto questo livello le righe si scartano senza toccare i sink. */
  minLevel: LogLevel
  sinks: LogSink[]
}

const ring = new RingBuffer()

const config: LoggerConfig = {
  // Default prudente: in produzione il traffico di debug non serve. Va alzato
  // esplicitamente, da impostazioni o da variabile d'ambiente nell'adapter.
  minLevel: 'info',
  sinks: []
}

export function configureLogger(patch: Partial<LoggerConfig>): void {
  if (patch.minLevel !== undefined) config.minLevel = patch.minLevel
  if (patch.sinks !== undefined) config.sinks = patch.sinks
}

export function addLogSink(sink: LogSink): void {
  config.sinks.push(sink)
}

/** Le ultime righe, per il pannello diagnostico e per i report di crash. */
export function recentLogs(): LogRecord[] {
  return ring.snapshot()
}

export function clearRecentLogs(): void {
  ring.clear()
}

/** Svuota tutti i sink. Non lancia: usata anche durante la chiusura e i crash. */
export async function flushLogs(): Promise<void> {
  for (const sink of config.sinks) {
    try {
      await sink.flush?.()
    } catch {
      // Un sink che non riesce a svuotarsi non deve impedire agli altri di farlo,
      // né trasformare la chiusura in un crash.
    }
  }
}

function emit(record: LogRecord): void {
  // Nel ring buffer entra SEMPRE, anche sotto il livello minimo: quando qualcosa
  // va storto si vogliono i dettagli che precedono il guasto, non solo quelli
  // che erano abbastanza importanti da essere scritti su file.
  ring.push(record)

  if (LEVEL_RANK[record.level] < LEVEL_RANK[config.minLevel]) return

  for (const sink of config.sinks) {
    try {
      sink.write(record)
    } catch {
      // Un sink rotto non deve propagare: il logging non è mai la ragione per
      // cui un'operazione fallisce.
    }
  }
}

/**
 * Logger legato a uno scope. Si ottiene con `logger(scope)`.
 *
 * I metodi accettano un errore come secondo argomento — di qualunque forma:
 * viene normalizzato con `AppError.from`, quindi anche un errno di Node o una
 * stringa lanciata finiscono nei log come dato strutturato.
 */
export interface ScopedLogger {
  trace(message: string, fields?: Record<string, unknown>): void
  debug(message: string, fields?: Record<string, unknown>): void
  info(message: string, fields?: Record<string, unknown>): void
  warn(message: string, error?: unknown, fields?: Record<string, unknown>): void
  error(message: string, error?: unknown, fields?: Record<string, unknown>): void
  fatal(message: string, error?: unknown, fields?: Record<string, unknown>): void
  /** Sotto-logger con campi impliciti su ogni riga (es. un id di sessione). */
  with(fields: Record<string, unknown>): ScopedLogger
}

function build(scope: LogScope, bound: Record<string, unknown> | undefined): ScopedLogger {
  function mergeFields(
    fields: Record<string, unknown> | undefined
  ): Record<string, unknown> | undefined {
    if (bound === undefined) return fields
    return fields === undefined ? bound : { ...bound, ...fields }
  }

  function plain(level: LogLevel) {
    return (message: string, fields?: Record<string, unknown>): void => {
      const merged = mergeFields(fields)
      emit({
        ts: Date.now(),
        level,
        scope,
        message,
        ...(merged !== undefined ? { fields: merged } : {})
      })
    }
  }

  function withError(level: LogLevel) {
    return (message: string, error?: unknown, fields?: Record<string, unknown>): void => {
      const merged = mergeFields(fields)
      const normalized = error === undefined ? undefined : AppError.from(error)
      emit({
        ts: Date.now(),
        level,
        scope,
        message,
        ...(merged !== undefined ? { fields: merged } : {}),
        ...(normalized !== undefined
          ? { error: normalized.toPayload(), traceId: normalized.traceId }
          : {})
      })
    }
  }

  return {
    trace: plain('trace'),
    debug: plain('debug'),
    info: plain('info'),
    warn: withError('warn'),
    error: withError('error'),
    fatal: withError('fatal'),
    with: (fields) => build(scope, mergeFields(fields))
  }
}

/** Punto d'ingresso: `const log = logger('skin')`. */
export function logger(scope: LogScope): ScopedLogger {
  return build(scope, undefined)
}

/**
 * Sink su console, isomorfo. È il default sensato in sviluppo e nel renderer.
 *
 * Stampa il record come oggetto e non come stringa preformattata: i devtools e
 * il terminale di Node lo rendono navigabile, stack compreso — che era proprio
 * ciò che il logger legacy buttava via.
 */
export function createConsoleSink(): LogSink {
  return {
    write(record) {
      const prefix = `[${record.scope}]`
      const payload = {
        ...(record.fields !== undefined ? { fields: record.fields } : {}),
        ...(record.error !== undefined ? { error: record.error } : {})
      }
      const hasPayload = Object.keys(payload).length > 0
      const args: unknown[] = hasPayload
        ? [prefix, record.message, payload]
        : [prefix, record.message]

      switch (record.level) {
        case 'trace':
        case 'debug':
          console.debug(...args)
          break
        case 'info':
          console.info(...args)
          break
        case 'warn':
          console.warn(...args)
          break
        case 'error':
        case 'fatal':
          console.error(...args)
          break
      }
    }
  }
}

/**
 * Sink che accumula in memoria. Serve ai test e al trasporto renderer→backend
 * (il renderer raccoglie e spedisce a lotti, invece di un IPC per riga).
 */
export function createMemorySink(): LogSink & { records: LogRecord[] } {
  const records: LogRecord[] = []
  return {
    records,
    write(record) {
      records.push(record)
    }
  }
}

/** JSON che non lancia: un campo con cicli non deve far cadere il logging. */
function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value) ?? 'null'
  } catch {
    return '"[unserializable]"'
  }
}

/** Riga di testo su una sola linea, per i sink su file. */
export function formatLogLine(record: LogRecord): string {
  const parts = [
    new Date(record.ts).toISOString(),
    record.level.toUpperCase().padEnd(5),
    `[${record.scope}]`,
    record.message
  ]
  if (record.fields !== undefined) {
    for (const key of Object.keys(record.fields)) {
      // Sempre JSON, stringhe comprese: un valore con spazi o segni di uguale
      // renderebbe la riga ambigua da rileggere.
      parts.push(`${key}=${safeJson(record.fields[key])}`)
    }
  }
  if (record.traceId !== undefined) parts.push(`trace=${record.traceId}`)
  if (record.error !== undefined) {
    parts.push(`error=${record.error.code}`)
    // Lo stack va su file: è l'informazione che il logger legacy scartava e che
    // serve sempre quando si indaga a posteriori.
    if (record.error.stack !== undefined) {
      parts.push(`\n${record.error.stack}`)
    }
    for (const cause of record.error.causes ?? []) {
      parts.push(`\n  caused by ${cause.kind ?? '?'}: ${cause.message}`)
    }
  }
  return parts.join(' ')
}
