/**
 * Scadenze.
 *
 * Il fallimento peggiore è il silenzio: un'operazione che non finisce mai e non
 * fallisce mai lascia la UI sugli scheletri per sempre. Nel legacy il desktop non
 * aveva alcun rilevamento di stallo — il mobile sì, con `bridgeWatchdog` — e
 * infatti se `getDb()` restava appeso non c'era nessuno a dirlo.
 *
 * `withIpcTimeout` in serialize.ts fa questo lavoro per il confine IPC. Questa è
 * la versione generica, per il codice di dominio: apertura del DB, spawn di un
 * binario, handshake LAN, scansione di una cartella di rete che non risponde.
 */

import { AppError } from '../errors'
import { err, ok, type Result } from '../result'
import { runFallible, type Fallible } from './run'

export interface DeadlineOptions {
  timeoutMs: number
  /** Cosa stava succedendo: finisce nell'errore e nei log. */
  what: string
}

/**
 * Esegue `fn` con una scadenza.
 *
 * Attenzione a cosa NON fa: non annulla il lavoro sottostante, perché in
 * generale non è annullabile (una query SQLite sincrona, una `stat` su un disco
 * di rete). Libera il chiamante e segnala il problema; se il lavoro è
 * annullabile, il modo giusto è passargli un AbortSignal.
 */
export async function withDeadline<T>(
  fn: Fallible<T>,
  options: DeadlineOptions
): Promise<Result<T, AppError>> {
  const { timeoutMs, what } = options

  let timer: ReturnType<typeof setTimeout> | undefined
  const expired = new Promise<Result<T, AppError>>((resolve) => {
    timer = setTimeout(() => {
      resolve(err(AppError.of('internal.timeout', { what, timeoutMs })))
    }, timeoutMs)
    ;(timer as { unref?: () => void }).unref?.()
  })

  try {
    return await Promise.race([runFallible(fn), expired])
  } finally {
    if (timer !== undefined) clearTimeout(timer)
  }
}

/**
 * Una scadenza condivisa da più passi. Serve alle operazioni composte — un
 * trasferimento skin è handshake, upload, commit — dove il tetto è complessivo e
 * non per passo.
 */
export interface Deadline {
  /** Millisecondi rimasti, 0 se scaduta. */
  remainingMs(): number
  expired(): boolean
  /** Err se scaduta, Ok altrimenti: da controllare fra un passo e il successivo. */
  check(what: string): Result<void, AppError>
}

export function createDeadline(
  totalMs: number,
  now: () => number = Date.now
): Deadline {
  const startedAt = now()
  const remainingMs = (): number => Math.max(0, totalMs - (now() - startedAt))
  return {
    remainingMs,
    expired: () => remainingMs() === 0,
    check: (what) =>
      remainingMs() === 0
        ? err(AppError.of('internal.timeout', { what, timeoutMs: totalMs }))
        : ok(undefined)
  }
}
