/**
 * Cosa fare quando la riproduzione fallisce.
 *
 * Nel legacy la risposta era una sola, per ogni guasto: fermarsi e scrivere
 * `loadError`. Una traccia con un byte fuori posto in mezzo a un album fermava
 * l'ascolto, e l'utente doveva capire da sé cos'era successo e cliccare la
 * successiva.
 *
 * Qui la risposta dipende dall'errore, e le possibilità sono cinque. Sono poche e
 * sono tutte necessarie:
 *
 *   retry           lo stesso, fra un momento: rete che va e viene, stallo
 *   fallbackSource  un'altra sorgente della stessa traccia
 *   skip            la traccia è rotta: si passa avanti e la si sfiducia
 *   awaitGesture    il browser aspetta un clic. Non è un guasto
 *   stop            non c'è più niente da tentare, e va detto
 *
 * La funzione è pura: decide, non agisce.
 */

import type { AppError } from '../errors'
import type { PlaybackState } from './state'

export type RecoveryAction = 'retry' | 'fallbackSource' | 'skip' | 'awaitGesture' | 'stop'

export interface RecoveryDecision {
  readonly action: RecoveryAction
  /** Attesa prima di agire. Zero quando non serve aspettare. */
  readonly delayMs: number
  /** Sorgente da usare: per `retry` è la stessa, per `fallbackSource` la seguente. */
  readonly sourceIndex: number
  /** Se la traccia va sfiduciata per il resto della sessione. */
  readonly distrust: boolean
  /**
   * Se l'utente deve vedere qualcosa.
   *
   * Falso per la maggior parte dei recuperi: un nuovo tentativo riuscito non è una
   * notizia, e riempire l'interfaccia di avvisi per cose che si sono risolte da sé
   * è il modo di far ignorare quelli che contano.
   */
  readonly notify: boolean
  /** Perché questa decisione. Per i log e per il pannello diagnostico. */
  readonly reason: string
}

/** Tentativi sulla stessa traccia prima di passare avanti. */
export const MAX_PLAYBACK_ATTEMPTS = 3
/** Attesa fra i tentativi: breve, l'utente sta aspettando di sentire musica. */
export const RETRY_BASE_MS = 400
export const RETRY_MAX_MS = 3_000

export interface RecoveryContext {
  readonly state: PlaybackState
  readonly error: AppError
  /** Quante sorgenti ha la traccia corrente (file locale, server media, stream). */
  readonly sourceCount: number
  /** Se esiste una traccia successiva da suonare. */
  readonly hasNext: boolean
}

function backoff(attempt: number): number {
  return Math.min(RETRY_BASE_MS * Math.pow(2, attempt), RETRY_MAX_MS)
}

export function decidePlaybackRecovery(ctx: RecoveryContext): RecoveryDecision {
  const { state, error, sourceCount, hasNext } = ctx
  const nextSource = state.sourceIndex + 1
  const hasFallback = nextSource < sourceCount

  // Un annullamento non è un guasto: è l'utente che ha cambiato traccia mentre la
  // precedente caricava. Nel legacy diventava «Titolo: 1» in faccia all'utente.
  if (error.code === 'internal.aborted') {
    return decision('stop', 0, state.sourceIndex, false, false, 'caricamento annullato')
  }

  // Il browser aspetta un gesto. Non si ritenta e non si salta: si aspetta il clic,
  // che è l'unica cosa che può sbloccare l'audio.
  if (error.code === 'playback.autoplayBlocked') {
    return decision(
      'awaitGesture',
      0,
      state.sourceIndex,
      false,
      true,
      'il sistema richiede un gesto dell\'utente per avviare l\'audio'
    )
  }

  // Il motore audio non c'è più: ritentare la traccia non serve a niente, il
  // problema è a monte. Va detto, e va detto una volta.
  if (error.code === 'playback.engineUnavailable') {
    return decision('stop', 0, state.sourceIndex, false, true, 'motore audio non disponibile')
  }

  // Guasti definitivi della sorgente: la stessa sorgente non guarirà. Se ce n'è
  // un'altra si prova quella — un file locale illeggibile può essere raggiungibile
  // via server media, e uno stream remoto può avere un URL alternativo.
  const permanentForSource =
    error.code === 'playback.decodeFailed' ||
    error.code === 'playback.formatUnsupported' ||
    error.code === 'playback.sourceUnavailable' ||
    error.code === 'fs.permissionDenied' ||
    error.code === 'fs.notFound'

  if (permanentForSource) {
    if (hasFallback) {
      return decision(
        'fallbackSource',
        0,
        nextSource,
        false,
        false,
        `sorgente ${state.sourceIndex} inutilizzabile, si prova la ${nextSource}`
      )
    }
    return skipOrStop(state, hasNext, 'la traccia non è riproducibile su nessuna sorgente')
  }

  // Da qui in giù l'errore è transitorio: rete, timeout, stallo, dispositivo audio
  // che è stato preso da un'altra app. Vale la pena riprovare.
  if (state.attempt < MAX_PLAYBACK_ATTEMPTS) {
    return decision(
      'retry',
      backoff(state.attempt),
      state.sourceIndex,
      false,
      false,
      `errore transitorio (${error.code}), tentativo ${state.attempt + 1} di ${MAX_PLAYBACK_ATTEMPTS}`
    )
  }

  // Tentativi esauriti. Se c'è un'altra sorgente vale ancora la pena: un file
  // locale che non si legge può essere lo stesso brano raggiungibile altrove.
  if (hasFallback) {
    return decision(
      'fallbackSource',
      0,
      nextSource,
      false,
      false,
      `tentativi esauriti sulla sorgente ${state.sourceIndex}, si prova la ${nextSource}`
    )
  }

  return skipOrStop(state, hasNext, `tentativi esauriti (${error.code})`)
}

/**
 * Saltare, ma non in cerchio.
 *
 * Senza il controllo su `hasNext`, "salta la traccia rotta" su una cartella di
 * file corrotti attraversa l'intera coda in un secondo e ricomincia. La sfiducia
 * accumulata nello stato è ciò che permette al chiamante di sapere quando la coda
 * è finita davvero.
 */
function skipOrStop(state: PlaybackState, hasNext: boolean, reason: string): RecoveryDecision {
  if (hasNext) {
    return decision('skip', 0, state.sourceIndex, true, true, reason)
  }
  return decision(
    'stop',
    0,
    state.sourceIndex,
    true,
    true,
    `${reason}, e non c'è una traccia successiva da provare`
  )
}

function decision(
  action: RecoveryAction,
  delayMs: number,
  sourceIndex: number,
  distrust: boolean,
  notify: boolean,
  reason: string
): RecoveryDecision {
  return { action, delayMs, sourceIndex, distrust, notify, reason }
}
