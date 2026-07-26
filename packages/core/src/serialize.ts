/**
 * La busta che attraversa il confine fra backend e renderer.
 *
 * Il problema che risolve, in concreto. Nel legacy c'erano due trasporti e
 * entrambi collassavano l'errore in una stringa:
 *
 *   electron/ipc/handle.ts:14
 *     throw err instanceof Error ? new Error(err.message) : new Error(String(err))
 *
 *   node-backend/runtime.ts:77  (bridge nodejs-mobile)
 *     emitReply(id, false, String(body))
 *
 * In più Electron premette al messaggio un suo prefisso
 * ("Error invoking remote method 'x': Error: "), che il renderer spogliava con
 * una regex prima di cercare la stringa in una tabella di codici. Tre punti di
 * perdita in fila.
 *
 * La correzione: gli handler NON lanciano più. Risolvono sempre con una busta,
 * e l'errore viaggia come dato. Un errore lanciato è un incidente, non il
 * canale di comunicazione.
 *
 * L'API vista dal renderer resta quella di prima — `await window.aether.x()`
 * che restituisce il valore o rigetta — ma ora rigetta con un AppError vero,
 * con status, retryable, catena delle cause e traceId intatti.
 */

import { AppError, isAppErrorPayload, type AppErrorPayload } from './errors'
import { err, ok, type Result } from './result'

/** Esito di una chiamata IPC. Solo JSON: sopravvive a structured clone e a JSON.stringify. */
export type IpcEnvelope<T> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly error: AppErrorPayload }

/** Un handler può restituire un valore nudo o un Result. Entrambi vanno bene. */
export type HandlerReturn<T> = T | Result<T, AppError> | Promise<T | Result<T, AppError>>

function isResult<T>(value: unknown): value is Result<T, AppError> {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { ok?: unknown }).ok === 'boolean' &&
    ('value' in value || 'error' in value)
  )
}

/** Impacchetta un valore o un Result nella busta. */
export function toEnvelope<T>(value: T | Result<T, AppError>): IpcEnvelope<T> {
  if (isResult<T>(value)) {
    return value.ok
      ? { ok: true, value: value.value }
      : { ok: false, error: AppError.from(value.error).toPayload() }
  }
  return { ok: true, value: value as T }
}

/** Impacchetta un fallimento, qualunque forma abbia. */
export function errorEnvelope<T>(cause: unknown): IpcEnvelope<T> {
  return { ok: false, error: AppError.from(cause).toPayload() }
}

/**
 * Avvolge un handler perché non lanci MAI.
 *
 * Va bene qualunque cosa venga sollevata dentro: un AppError, un Error, un
 * errno di Node, una stringa, un null lanciato per sbaglio. `AppError.from` ha
 * sempre una risposta, quindi la busta esiste sempre e il renderer non resta
 * mai in attesa di una promise che non si risolve.
 */
export function wrapHandler<A extends readonly unknown[], T>(
  channel: string,
  handler: (...args: A) => HandlerReturn<T>
): (...args: A) => Promise<IpcEnvelope<T>> {
  return async (...args: A): Promise<IpcEnvelope<T>> => {
    try {
      const result = await handler(...args)
      return toEnvelope(result as T | Result<T, AppError>)
    } catch (cause) {
      // Il canale nel contesto: senza, un errore generico non dice da dove viene.
      return { ok: false, error: AppError.from(cause).withContext({ channel }).toPayload() }
    }
  }
}

/** Scarta la busta in un Result. Non lancia. */
export function fromEnvelope<T>(envelope: unknown): Result<T, AppError> {
  if (
    typeof envelope === 'object' &&
    envelope !== null &&
    typeof (envelope as { ok?: unknown }).ok === 'boolean'
  ) {
    const e = envelope as IpcEnvelope<T>
    if (e.ok) return ok(e.value)
    return err(
      isAppErrorPayload(e.error)
        ? AppError.fromPayload(e.error)
        : AppError.from(e.error)
    )
  }

  // Nessuna busta: il trasporto è rotto o parla un protocollo diverso.
  // Meglio dirlo con un codice preciso che indovinare.
  return err(
    AppError.of('ipc.backendUnreachable', {}, {
      context: { reason: 'risposta senza busta', received: typeof envelope }
    })
  )
}

/**
 * Scarta la busta e, se è un fallimento, rigetta con l'AppError.
 *
 * È l'adattatore che mantiene la forma `await window.aether.x()` familiare al
 * renderer, ma con un errore ricco al posto di una stringa. I siti di chiamata
 * che già fanno try/catch continuano a funzionare, e in più possono leggere
 * `AppError.from(err).retryable` o `.i18nKey`.
 */
export async function unwrapEnvelope<T>(envelope: unknown): Promise<T> {
  const result = fromEnvelope<T>(envelope)
  if (result.ok) return result.value
  throw result.error
}

/**
 * Applica un timeout a una chiamata sul trasporto.
 *
 * Serve perché il fallimento peggiore non è l'errore, è il silenzio: nel legacy
 * il desktop non aveva alcun rilevamento di stallo (il mobile sì, con
 * bridgeWatchdog.ts) e una promise mai risolta lasciava la UI appesa sugli
 * scheletri senza spiegazione.
 */
export function withIpcTimeout<T>(
  promise: Promise<T>,
  channel: string,
  timeoutMs: number
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let settled = false
    const timer = setTimeout(() => {
      if (settled) return
      settled = true
      reject(
        AppError.of('ipc.backendUnreachable', {}, {
          context: { channel, timeoutMs, reason: 'timeout' }
        })
      )
    }, timeoutMs)

    promise.then(
      (value) => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        resolve(value)
      },
      (cause) => {
        if (settled) return
        settled = true
        clearTimeout(timer)
        reject(AppError.from(cause).withContext({ channel }))
      }
    )
  })
}
