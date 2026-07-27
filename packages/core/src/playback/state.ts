/**
 * La macchina a stati della riproduzione.
 *
 * Nel legacy lo stato del player era implicito e distribuito in tre posti che
 * potevano contraddirsi:
 *
 *   - `usePlayerStore.isPlaying`, un booleano;
 *   - `PlayerEngine.current` / `.preloaded`, due riferimenti a oggetti Howl;
 *   - lo stato interno di Howler, non osservabile da fuori.
 *
 * Con tre fonti e nessuna sincronizzazione, "sta caricando" e "è in errore" non
 * esistevano affatto: un caricamento in corso era `isPlaying: false`, cioè
 * indistinguibile da una pausa, e un errore era `isPlaying: false` più una stringa
 * in un campo a parte. Da lì nascono i sintomi che si vedono usando l'app: il
 * pulsante play che non risponde mentre carica, la barra che resta ferma senza
 * dire perché, e la riproduzione che si arresta su una traccia rotta.
 *
 * Qui lo stato è uno, le transizioni sono dichiarate, e una transizione illegale
 * NON viene ignorata in silenzio: `reduce` la restituisce come `ignored`, con il
 * motivo. È la differenza fra un evento perso e un evento rifiutato.
 *
 * Questo file è puro: nessun Howl, nessun ExoPlayer, nessun timer. Il chiamante
 * traduce gli eventi del motore in eventi della macchina e le transizioni in
 * comandi al motore.
 */

import type { AppError } from '../errors'

export type PlaybackStatus =
  /** Niente in canna. */
  | 'idle'
  /** Sorgente in caricamento. Lo stato che nel legacy non esisteva. */
  | 'loading'
  /** Caricata e pronta, ferma. */
  | 'ready'
  | 'playing'
  | 'paused'
  /**
   * Suonava e il tempo non avanza più: rete caduta a metà di uno stream, disco
   * di rete scomparso, buffer vuoto. Diverso da `paused` (voluta) e da `error`
   * (finita). Nel legacy non c'era, quindi uno stallo era una pausa che non
   * ripartiva.
   */
  | 'stalled'
  | 'error'
  /** Un tentativo di recupero è in corso: nuovo tentativo o sorgente alternativa. */
  | 'recovering'

export interface PlaybackState {
  readonly status: PlaybackStatus
  readonly trackId: number | null
  /**
   * Quale sorgente della traccia si sta usando. Una traccia può averne più di
   * una — il file locale, l'URL del server media locale, uno stream remoto — e
   * il recupero passa alla successiva invece di arrendersi.
   */
  readonly sourceIndex: number
  readonly positionMs: number
  readonly durationMs: number
  /**
   * L'utente vuole che suoni.
   *
   * Separato dallo stato perché sono cose diverse: premere play mentre carica non
   * deve essere perso (nel legacy dipendeva dal flag `autoplay` passato a Howl al
   * momento della creazione, quindi un play arrivato 100ms dopo non aveva effetto).
   */
  readonly playIntent: boolean
  /** Tentativi già spesi su questa traccia. Azzerato quando la traccia cambia. */
  readonly attempt: number
  readonly error: AppError | null
  /** Quando è arrivato l'ultimo avanzamento, per il rilevamento di stallo. */
  readonly lastProgressAt: number | null
  /**
   * Tracce che hanno fallito in modo definitivo in questa sessione.
   *
   * Serve a evitare il ciclo: senza, "salta la traccia rotta" su una cartella di
   * file corrotti attraversa l'intera coda in un secondo e ricomincia. Vale per la
   * sessione, non è persistita: un file può tornare a posto.
   */
  readonly distrusted: readonly number[]
}

export const INITIAL_PLAYBACK_STATE: PlaybackState = {
  status: 'idle',
  trackId: null,
  sourceIndex: 0,
  positionMs: 0,
  durationMs: 0,
  playIntent: false,
  attempt: 0,
  error: null,
  lastProgressAt: null,
  distrusted: []
}

export type PlaybackEvent =
  | { readonly type: 'load'; readonly trackId: number; readonly autoplay: boolean; readonly sourceIndex?: number }
  | { readonly type: 'loaded'; readonly durationMs: number; readonly at: number }
  | { readonly type: 'play'; readonly at: number }
  | { readonly type: 'pause' }
  | { readonly type: 'progress'; readonly positionMs: number; readonly at: number }
  | { readonly type: 'seek'; readonly positionMs: number; readonly at: number }
  | { readonly type: 'ended' }
  | { readonly type: 'stalled'; readonly at: number }
  | { readonly type: 'failed'; readonly error: AppError }
  | { readonly type: 'recover'; readonly sourceIndex: number }
  | { readonly type: 'stop' }
  /** Rende definitivo il fallimento di una traccia, per questa sessione. */
  | { readonly type: 'distrust'; readonly trackId: number }
  /** L'utente riprova a mano una traccia sfiduciata: torna a fidarsi. */
  | { readonly type: 'trust'; readonly trackId: number }

export interface PlaybackTransition {
  readonly state: PlaybackState
  /**
   * Presente quando l'evento non era ammesso nello stato corrente. Il motivo è
   * per i log e per i test: un evento rifiutato in silenzio è il modo in cui una
   * macchina a stati diventa inaffidabile senza che nessuno lo noti.
   */
  readonly ignored?: { readonly event: PlaybackEvent['type']; readonly reason: string }
}

const PLAYABLE: readonly PlaybackStatus[] = ['ready', 'paused', 'stalled', 'playing']

function ignore(
  state: PlaybackState,
  event: PlaybackEvent['type'],
  reason: string
): PlaybackTransition {
  return { state, ignored: { event, reason } }
}

/** Soglia oltre la quale un avanzamento che non arriva è uno stallo. */
export const STALL_THRESHOLD_MS = 5_000

/**
 * Se dichiarare uno stallo, dato l'istante corrente.
 *
 * Sta qui e non in un timer perché la decisione è logica e va provata; il
 * chiamante fa girare l'orologio. Il desktop non aveva alcun rilevamento di
 * stallo — il backend mobile sì, con `bridgeWatchdog`, ma solo per il bridge.
 */
export function shouldDeclareStall(
  state: PlaybackState,
  now: number,
  thresholdMs: number = STALL_THRESHOLD_MS
): boolean {
  if (state.status !== 'playing') return false
  if (state.lastProgressAt === null) return false
  return now - state.lastProgressAt >= thresholdMs
}

/** Se la traccia è stata sfiduciata in questa sessione. */
export function isDistrusted(state: PlaybackState, trackId: number): boolean {
  return state.distrusted.includes(trackId)
}

export function reduce(state: PlaybackState, event: PlaybackEvent): PlaybackTransition {
  switch (event.type) {
    case 'load': {
      const sameTrack = state.trackId === event.trackId
      return {
        state: {
          ...state,
          status: 'loading',
          trackId: event.trackId,
          sourceIndex: event.sourceIndex ?? 0,
          positionMs: 0,
          durationMs: sameTrack ? state.durationMs : 0,
          playIntent: event.autoplay,
          // Il contatore dei tentativi appartiene alla traccia, non alla sessione:
          // caricarne un'altra lo azzera, ricaricare la stessa (un recupero) no.
          attempt: sameTrack ? state.attempt : 0,
          error: null,
          lastProgressAt: null
        }
      }
    }

    case 'loaded': {
      if (state.status !== 'loading' && state.status !== 'recovering') {
        return ignore(state, 'loaded', `arrivato in stato ${state.status}`)
      }
      // Qui si onora l'intenzione: un play premuto durante il caricamento non si
      // perde, come accadeva quando l'autoplay era deciso alla creazione dell'Howl.
      return {
        state: {
          ...state,
          status: state.playIntent ? 'playing' : 'ready',
          durationMs: event.durationMs,
          error: null,
          lastProgressAt: state.playIntent ? event.at : null
        }
      }
    }

    case 'play': {
      if (state.trackId === null) return ignore(state, 'play', 'nessuna traccia caricata')
      if (state.status === 'loading' || state.status === 'recovering') {
        // Non è un rifiuto: l'intenzione resta registrata e verrà onorata da
        // 'loaded'. È il caso del pulsante premuto mentre il file carica.
        return { state: { ...state, playIntent: true } }
      }
      if (!PLAYABLE.includes(state.status)) {
        return ignore(state, 'play', `non si può suonare da ${state.status}`)
      }
      return {
        state: { ...state, status: 'playing', playIntent: true, lastProgressAt: event.at }
      }
    }

    case 'pause': {
      if (state.status === 'loading' || state.status === 'recovering') {
        // Simmetrico al play: si annulla l'intenzione invece di rifiutare.
        return { state: { ...state, playIntent: false } }
      }
      if (state.status !== 'playing' && state.status !== 'stalled') {
        return ignore(state, 'pause', `non è in riproduzione (${state.status})`)
      }
      return { state: { ...state, status: 'paused', playIntent: false } }
    }

    case 'progress': {
      if (state.status !== 'playing' && state.status !== 'stalled') {
        return ignore(state, 'progress', `non è in riproduzione (${state.status})`)
      }
      // Un avanzamento durante uno stallo è la fine dello stallo: il buffer si è
      // riempito da sé. Nessun intervento, nessun messaggio all'utente.
      return {
        state: {
          ...state,
          status: 'playing',
          positionMs: event.positionMs,
          lastProgressAt: event.at,
          error: state.status === 'stalled' ? null : state.error,
          // Ripartire da sé conta come recupero riuscito: i tentativi si azzerano,
          // altrimenti tre stalli distanti un'ora esaurirebbero il budget.
          attempt: state.status === 'stalled' ? 0 : state.attempt
        }
      }
    }

    case 'seek': {
      if (state.trackId === null) return ignore(state, 'seek', 'nessuna traccia caricata')
      return { state: { ...state, positionMs: event.positionMs, lastProgressAt: event.at } }
    }

    case 'ended': {
      if (state.status !== 'playing') {
        return ignore(state, 'ended', `non è in riproduzione (${state.status})`)
      }
      // La traccia è finita bene: si torna in attesa che il chiamante carichi la
      // successiva, ma l'intenzione di suonare rimane.
      return {
        state: { ...state, status: 'idle', positionMs: 0, attempt: 0, lastProgressAt: null }
      }
    }

    case 'stalled': {
      if (state.status !== 'playing') {
        return ignore(state, 'stalled', `non è in riproduzione (${state.status})`)
      }
      return { state: { ...state, status: 'stalled' } }
    }

    case 'failed': {
      if (state.status === 'idle') return ignore(state, 'failed', 'nessuna traccia in corso')
      return {
        state: {
          ...state,
          status: 'error',
          error: event.error,
          lastProgressAt: null
        }
      }
    }

    case 'recover': {
      if (state.status !== 'error' && state.status !== 'stalled') {
        return ignore(state, 'recover', `non c'è niente da recuperare da ${state.status}`)
      }
      if (state.trackId === null) return ignore(state, 'recover', 'nessuna traccia')
      return {
        state: {
          ...state,
          status: 'recovering',
          sourceIndex: event.sourceIndex,
          attempt: state.attempt + 1,
          error: null,
          lastProgressAt: null
        }
      }
    }

    case 'stop': {
      return {
        state: {
          ...INITIAL_PLAYBACK_STATE,
          // La sfiducia sopravvive a uno stop: è una proprietà della sessione, non
          // della riproduzione in corso.
          distrusted: state.distrusted
        }
      }
    }

    case 'distrust': {
      if (state.distrusted.includes(event.trackId)) return { state }
      return { state: { ...state, distrusted: [...state.distrusted, event.trackId] } }
    }

    case 'trust': {
      if (!state.distrusted.includes(event.trackId)) return { state }
      return {
        state: {
          ...state,
          distrusted: state.distrusted.filter((id) => id !== event.trackId),
          // Ridare fiducia significa ridare tentativi: l'utente ha chiesto
          // esplicitamente di riprovare quella traccia.
          attempt: state.trackId === event.trackId ? 0 : state.attempt
        }
      }
    }
  }
}

/** Applica una sequenza di eventi. Utile ai test e al ripristino di sessione. */
export function reduceAll(
  state: PlaybackState,
  events: readonly PlaybackEvent[]
): PlaybackState {
  return events.reduce((current, event) => reduce(current, event).state, state)
}
