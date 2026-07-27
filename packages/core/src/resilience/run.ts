/**
 * Il pezzo comune a retry, circuit breaker e rate limiter: eseguire una funzione
 * fallibile senza che nulla possa sfuggire come eccezione.
 *
 * Nel legacy questi tre componenti stavano dentro `electron/modules/net/` e
 * parlavano solo il linguaggio delle eccezioni di quel sottoalbero
 * (`NetworkError`, `HttpError`, `RateLimitError`). Il risultato è che nessun
 * altro modulo li usava: `library`, `metadata`, `db`, `settings`, `sync`, `lan` e
 * `phoneSync` facevano log-and-continue con `logWarn` invece di ritentare, perché
 * per usare `withRetry` avrebbero dovuto adottare quelle classi.
 *
 * Qui il linguaggio comune è `Result<T, AppError>`, che tutto il core parla già.
 */

import { AppError } from '../errors'
import { err, ok, type Result } from '../result'

/** Un'operazione che può fallire, nella forma che tutto il core usa. */
export type Fallible<T> = () => Result<T, AppError> | Promise<Result<T, AppError>>

/**
 * Esegue una funzione fallibile e normalizza qualunque cosa venga sollevata.
 *
 * Serve anche come rete: una funzione che lancia invece di restituire un Err
 * (codice non ancora convertito, libreria di terze parti, un `throw` sincrono
 * prima del primo await) non manda in pezzi il ciclo di retry né lascia un
 * permesso occupato nel rate limiter.
 */
export async function runFallible<T>(fn: Fallible<T>): Promise<Result<T, AppError>> {
  try {
    return await fn()
  } catch (cause) {
    return err(AppError.from(cause))
  }
}

/**
 * Se un segnale è già stato annullato, l'errore corrispondente.
 *
 * `internal.aborted` e non un errore generico: un annullamento voluto non è un
 * guasto. Nel legacy diventava `new Error('Aborted')` e finiva nei log accanto ai
 * guasti veri, indistinguibile.
 */
export function abortedIfSignalled(
  signal: AbortSignal | undefined,
  what: string | undefined
): AppError | undefined {
  if (signal?.aborted !== true) return undefined
  return abortError(signal, what)
}

export function abortError(
  signal: AbortSignal | undefined,
  what: string | undefined
): AppError {
  // `reason` non esiste su tutti i runtime che ci interessano (il backend mobile
  // gira su Node 12): va letto in modo difensivo e non dato per scontato.
  const reason = (signal as { reason?: unknown } | undefined)?.reason
  return AppError.of(
    'internal.aborted',
    what !== undefined ? { what } : {},
    reason !== undefined ? { cause: reason } : undefined
  )
}

/** Attesa interrompibile. Restituisce un Err se il segnale scatta durante l'attesa. */
export function sleep(
  ms: number,
  signal?: AbortSignal,
  what?: string
): Promise<Result<void, AppError>> {
  return new Promise((resolve) => {
    const already = abortedIfSignalled(signal, what)
    if (already) {
      resolve(err(already))
      return
    }

    const onAbort = (): void => {
      clearTimeout(timer)
      resolve(err(abortError(signal, what)))
    }

    const timer = setTimeout(() => {
      signal?.removeEventListener('abort', onAbort)
      resolve(ok(undefined))
    }, ms)

    // Un'attesa pendente non deve tenere in vita il processo alla chiusura.
    ;(timer as { unref?: () => void }).unref?.()
    signal?.addEventListener('abort', onAbort, { once: true })
  })
}
