/**
 * Riproduzione: stato esplicito, errori tipizzati, recupero.
 *
 * I tre file rispondono a tre domande diverse, e nel legacy nessuna aveva una
 * risposta in un posto solo:
 *
 *   state.ts     dove siamo (era diviso fra isPlaying, PlayerEngine e Howler)
 *   errors.ts    cosa è andato storto (era `String(err)`, cioè «2»)
 *   recovery.ts  cosa fare adesso (era: fermarsi)
 *
 * Tutti e tre sono puri: nessun Howl, nessun ExoPlayer, nessun timer. Il motore
 * concreto sta negli adapter, e questi tre file lo governano.
 */

export {
  INITIAL_PLAYBACK_STATE,
  STALL_THRESHOLD_MS,
  isDistrusted,
  reduce,
  reduceAll,
  shouldDeclareStall,
  type PlaybackEvent,
  type PlaybackState,
  type PlaybackStatus,
  type PlaybackTransition
} from './state'

export {
  fromExoPlayerError,
  fromHowlerError,
  type ExoPlayerFailure,
  type HowlerFailure
} from './errors'

export {
  MAX_PLAYBACK_ATTEMPTS,
  RETRY_BASE_MS as PLAYBACK_RETRY_BASE_MS,
  RETRY_MAX_MS as PLAYBACK_RETRY_MAX_MS,
  decidePlaybackRecovery,
  type RecoveryAction,
  type RecoveryContext,
  type RecoveryDecision
} from './recovery'
