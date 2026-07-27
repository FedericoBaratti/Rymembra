/**
 * La metà pura della macchina a stati dei retry del downloader: classifica il
 * guasto e decide se e quando riprovare. Estratta dal downloader (che possiede le
 * scritture su DB e il timer) così è verificabile senza né l'uno né l'altro.
 *
 * Porta `legacy/Aeter/electron/modules/download/retryPolicy.ts` con gli stessi
 * numeri (3 tentativi, 30s/60s/120s, 5 minuti sui rate limit) e la stessa
 * politica di fondo: gli errori permanenti si fermano subito, i transitori
 * arretrano esponenzialmente, e un bug sconosciuto NON entra in un ciclo di
 * retry.
 *
 * Due cose cambiano.
 *
 * La classificazione. Nel legacy era `err instanceof DownloadError ?
 * err.failureClass : 'permanent'`, e `classifyDownloadFailure` — la funzione che
 * assegnava quella classe ai codici — aveva default DIVERGENTI fra i due alberi:
 * `'permanent'` sul desktop e `'transient'` sul mobile. Lo stesso errore veniva
 * ritentato su un dispositivo e no sull'altro. Ora la classe si legge dal
 * catalogo, che è uno.
 *
 * Ciò che si persiste. Il legacy salvava in SQLite `errorMessage`, cioè una frase
 * italiana per l'utente ("Errore di rete — nuovo tentativo automatico") oppure il
 * messaggio grezzo dell'eccezione. Un testo per l'utente in una colonna di
 * database è un vicolo cieco: non si può tradurre, non si può filtrare, non si può
 * confrontare. Qui la decisione porta un AppError: si persiste `error.code` e i
 * suoi parametri, e la frase la costruisce la UI dalla chiave i18n.
 */

import { AppError, type ErrorCode } from '../errors'

export const MAX_ATTEMPTS = 3
export const RETRY_BASE_MS = 30_000
export const RATE_LIMIT_PAUSE_MS = 300_000

export type DownloadFailureClass = 'permanent' | 'transient' | 'rate-limited'

export type RetryDecision =
  | { action: 'fail'; failureClass: DownloadFailureClass; error: AppError }
  | {
      action: 'retry'
      failureClass: DownloadFailureClass
      /** Nuovo contatore da persistere (attempts in ingresso + 1). */
      attempts: number
      delayMs: number
      error: AppError
    }

/**
 * Un pacchetto yt-dlp corrotto si presenta come un traceback di zipimport o come
 * un header zip rotto. Va riconosciuto qui perché arriva come testo dallo stderr
 * del processo figlio, non come codice: è l'unico punto in cui questo modulo
 * guarda un messaggio invece di un codice.
 */
const CORRUPTED_PACKAGE = /YTDLP_CORRUPTED|bad local file header|zipimport/i

const RATE_LIMIT_CODES: ReadonlySet<string> = new Set<ErrorCode>([
  'download.rateLimited',
  'download.rateLimitedRetry',
  'net.rateLimited'
])

function looksCorrupted(error: AppError): boolean {
  if (CORRUPTED_PACKAGE.test(error.message)) return true
  for (const cause of error.causes) {
    if (CORRUPTED_PACKAGE.test(cause.message)) return true
  }
  return false
}

function classify(error: AppError): DownloadFailureClass {
  if (RATE_LIMIT_CODES.has(error.code)) return 'rate-limited'
  // La ritentabilità arriva dal catalogo. Per un codice sconosciuto è `false`,
  // quindi 'permanent': un bug che non sappiamo nominare non deve essere
  // ritentato tre volte prima di essere visto.
  return error.retryable ? 'transient' : 'permanent'
}

/**
 * Normalizza qualunque guasto del downloader in un AppError, collassando un
 * pacchetto corrotto sul suo codice.
 *
 * Il traceback grezzo non viene mai persistito: resta nella catena delle cause
 * per i log, e sul filo va `download.ytdlpCorrupted`, che la UI sa tradurre e il
 * riparatore sa riconoscere.
 */
export function normalizeDownloadError(raw: unknown): AppError {
  const error = AppError.from(raw)
  if (error.code === 'download.ytdlpCorrupted') return error
  if (!looksCorrupted(error)) return error
  return AppError.of(
    'download.ytdlpCorrupted',
    {},
    { cause: raw, traceId: error.traceId, ...(error.context ? { context: error.context } : {}) }
  )
}

/** Decide cosa fare di un guasto, dati i tentativi già bruciati. */
export function decideRetry(raw: unknown, attempts: number): RetryDecision {
  const error = normalizeDownloadError(raw)
  const failureClass = classify(error)

  if (failureClass === 'permanent' || attempts >= MAX_ATTEMPTS) {
    return { action: 'fail', failureClass, error }
  }

  return {
    action: 'retry',
    failureClass,
    attempts: attempts + 1,
    delayMs:
      failureClass === 'rate-limited'
        ? RATE_LIMIT_PAUSE_MS
        : RETRY_BASE_MS * Math.pow(2, attempts),
    error
  }
}
