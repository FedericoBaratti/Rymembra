/**
 * Backoff esponenziale con jitter, su Result.
 *
 * Porta `legacy/Aeter/electron/modules/net/retry.ts` senza cambiarne la
 * matematica — era già corretta: crescita geometrica, tetto massimo, full jitter
 * sulla metà bassa, e il suggerimento `Retry-After` che allunga l'attesa. Due
 * cose cambiano.
 *
 * La prima: `shouldRetry` non è più `isRetryableError`, che ispezionava le classi
 * di errore di quel sottoalbero (`err instanceof NetworkError`, `err.status >=
 * 500`) e quindi funzionava solo per il codice che le usava. Ora la
 * ritentabilità è un DATO sull'errore, calcolato una volta dal catalogo alla
 * costruzione: `error.retryable`. Un ENOSPC in scrittura tag, un 503 da
 * MusicBrainz e un DB bloccato rispondono alla stessa domanda senza che questo
 * file conosca nessuno di loro.
 *
 * La seconda: non si lancia. Il ciclo restituisce l'ultimo errore con il numero
 * di tentativi bruciati nel contesto, così i log dicono "caduto dopo 4 tentativi"
 * invece di ripetere quattro volte la stessa riga senza dire che era un retry.
 */

import { AppError } from '../errors'
import { err, ok, type Result } from '../result'
import { abortedIfSignalled, runFallible, sleep as realSleep } from './run'

export interface RetryOptions {
  /** Tentativi OLTRE il primo. Default 3, cioè 4 chiamate in tutto. */
  retries?: number
  baseDelayMs?: number
  maxDelayMs?: number
  factor?: number
  /** Full jitter: l'attesa diventa `delay * random(0.5..1)`. Default acceso. */
  jitter?: boolean
  signal?: AbortSignal
  /** Default: `error.retryable`, il dato che arriva dal catalogo. */
  shouldRetry?: (error: AppError, attempt: number) => boolean
  onRetry?: (error: AppError, attempt: number, delayMs: number) => void
  /** Nome dell'operazione: finisce nel contesto dell'errore e nei log. */
  what?: string
  /** Iniettabili per i test, così non servono timer finti né sorte. */
  sleep?: (ms: number, signal?: AbortSignal) => Promise<Result<void, AppError>>
  random?: () => number
}

/** Il tetto per l'attesa suggerita da un `Retry-After`, in multipli di maxDelayMs. */
const RETRY_AFTER_CAP_FACTOR = 4

/**
 * Quanto aspettare secondo l'errore stesso.
 *
 * Nel legacy era `err instanceof RateLimitError && err.retryAfterMs !== null`, e
 * valeva per un solo tipo di errore. Ora il suggerimento è un parametro
 * dell'errore, quindi lo portano `net.rateLimited`, `net.circuitOpen` e chiunque
 * altro sappia dire quando riprovare — senza toccare questo file.
 */
function retryAfterHint(error: AppError): number | undefined {
  const value = error.params['retryAfterMs']
  return typeof value === 'number' && value > 0 ? value : undefined
}

export function nextDelayMs(
  attempt: number,
  error: AppError,
  opts: RetryOptions = {}
): number {
  const {
    baseDelayMs = 500,
    maxDelayMs = 15_000,
    factor = 2,
    jitter = true,
    random = Math.random
  } = opts

  let wait = Math.min(baseDelayMs * Math.pow(factor, attempt), maxDelayMs)
  if (jitter) wait = wait * (0.5 + random() * 0.5)

  const hint = retryAfterHint(error)
  if (hint !== undefined) {
    // Un 429 merita un'attesa più lunga di un 5xx: il tetto qui è quattro volte
    // quello normale, perché ignorare un Retry-After è il modo migliore per
    // restare bloccati fuori.
    wait = Math.min(Math.max(hint, wait), maxDelayMs * RETRY_AFTER_CAP_FACTOR)
  }
  return wait
}

/**
 * Esegue `fn` ritentando secondo `opts`. Non lancia mai.
 *
 * `fn` riceve il numero di tentativo (0 il primo), utile per variare la
 * strategia — nel legacy serviva a cambiare client HTTP fra i tentativi.
 */
export async function withRetry<T>(
  fn: (attempt: number) => Result<T, AppError> | Promise<Result<T, AppError>>,
  opts: RetryOptions = {}
): Promise<Result<T, AppError>> {
  const {
    retries = 3,
    signal,
    what,
    shouldRetry = (error) => error.retryable,
    onRetry,
    sleep = realSleep
  } = opts

  let attempt = 0
  for (;;) {
    const aborted = abortedIfSignalled(signal, what)
    if (aborted) return err(aborted)

    const result = await runFallible(() => fn(attempt))
    if (result.ok) return result

    const error = result.error
    const exhausted = attempt >= retries
    if (exhausted || !shouldRetry(error, attempt)) {
      return err(
        error.withContext({
          attempts: attempt + 1,
          ...(what !== undefined ? { operation: what } : {}),
          ...(exhausted && attempt > 0 ? { retriesExhausted: true } : {})
        })
      )
    }

    const wait = nextDelayMs(attempt, error, opts)
    onRetry?.(error, attempt, wait)

    const waited = await sleep(wait, signal)
    // Annullato durante l'attesa: si restituisce l'annullamento, non l'errore
    // che stavamo per ritentare. Chi ha annullato non vuole una diagnosi.
    if (!waited.ok) return err(waited.error)

    attempt++
  }
}

/**
 * Variante per il codice che ancora lancia: avvolge una promise qualunque.
 * Utile ai bordi (fetch, driver SQLite, plugin nativi) dove non c'è ancora un
 * Result da restituire.
 */
export async function retryThrowing<T>(
  fn: (attempt: number) => Promise<T>,
  opts: RetryOptions = {}
): Promise<Result<T, AppError>> {
  return withRetry(async (attempt) => {
    const value = await fn(attempt)
    return ok(value)
  }, opts)
}
