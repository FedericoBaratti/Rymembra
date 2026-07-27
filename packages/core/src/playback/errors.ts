/**
 * Errori di riproduzione: dai codici dei motori ai codici del catalogo.
 *
 * Cosa succedeva prima. Nel renderer, `player.ts:90` faceva
 * `onLoadError(track, String(err))` e lo store salvava
 * `loadError: \`${track.title}: ${message}\``. Il risultato che vedeva l'utente era
 * la riga «Nome della canzone: 2». Due, cioè `MEDIA_ERR_NETWORK`. Nessuna
 * distinzione fra rete assente, file corrotto, formato non supportato e blocco
 * dell'autoplay del browser — e quindi nessuna possibilità di reagire in modo
 * diverso ai quattro casi.
 *
 * Sul lato Android era lo stesso in peggio: `NativeAudioPlugin.onPlayerError` era
 * una riga che buttava `PlaybackException.errorCode`, l'informazione più precisa
 * disponibile in tutto il sistema.
 *
 * Qui i codici dei due motori diventano codici del catalogo, e da lì la politica
 * di recupero sa cosa fare.
 */

import { AppError, type ErrorCode } from '../errors'

/**
 * I codici di Howler sono quelli di `MediaError` dell'elemento audio HTML.
 * Sono quattro, e ognuno vuole una reazione diversa.
 */
const HOWLER_CODE_TO_ERROR: Record<number, ErrorCode> = {
  // 1 — MEDIA_ERR_ABORTED: il caricamento è stato interrotto, di norma perché
  // l'utente ha cambiato traccia. Non è un guasto e non va mostrato come tale.
  1: 'internal.aborted',
  2: 'net.offline',
  3: 'playback.decodeFailed',
  4: 'playback.formatUnsupported'
}

export interface HowlerFailure {
  /** Il valore che Howler passa a onloaderror/onplayerror. */
  readonly code: unknown
  /** Se viene da onplayerror invece che da onloaderror. */
  readonly whilePlaying?: boolean
  readonly trackId?: number
  readonly path?: string
  readonly format?: string
}

/**
 * Traduce un guasto di Howler.
 *
 * Il caso `whilePlaying` merita attenzione: `onplayerror` si attiva quasi sempre
 * perché il browser ha bloccato l'audio in attesa di un gesto dell'utente, non
 * perché la traccia sia rotta. Nel legacy finiva nello stesso messaggio di un file
 * corrotto, e l'utente vedeva un errore dove serviva solo un clic.
 */
export function fromHowlerError(failure: HowlerFailure): AppError {
  const numeric = typeof failure.code === 'number' ? failure.code : Number(failure.code)

  if (failure.whilePlaying === true && !Number.isFinite(numeric)) {
    return AppError.of('playback.autoplayBlocked')
  }

  const mapped = Number.isFinite(numeric) ? HOWLER_CODE_TO_ERROR[numeric] : undefined
  if (mapped === undefined) {
    return AppError.of(
      failure.whilePlaying === true ? 'playback.autoplayBlocked' : 'playback.decodeFailed',
      failure.whilePlaying === true ? {} : trackParams(failure),
      { context: { engine: 'howler', raw: String(failure.code) } }
    )
  }

  return buildPlaybackError(mapped, failure, 'howler', String(failure.code))
}

/**
 * `PlaybackException.errorCode` di AndroidX Media3.
 *
 * Media3 raggruppa i codici per migliaia, deliberatamente: 1xxx generici, 2xxx
 * I/O, 3xxx parsing, 4xxx decodifica, 5xxx AudioTrack, 6xxx DRM. Si mappa per
 * fascia più qualche caso specifico, così un codice nuovo introdotto da una
 * versione futura della libreria finisce comunque nella famiglia giusta invece di
 * cadere nel catch-all — che è esattamente il modo in cui `errorCode` era già
 * andato perso una volta.
 */
const EXOPLAYER_SPECIFIC: Record<number, ErrorCode> = {
  1003: 'net.timeout', // ERROR_CODE_TIMEOUT
  2002: 'net.timeout', // ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT
  2005: 'playback.sourceUnavailable', // ERROR_CODE_IO_FILE_NOT_FOUND
  2006: 'fs.permissionDenied', // ERROR_CODE_IO_NO_PERMISSION
  2007: 'playback.sourceUnavailable', // ERROR_CODE_IO_CLEARTEXT_NOT_PERMITTED
  3003: 'playback.formatUnsupported', // ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED
  3004: 'playback.formatUnsupported', // ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED
  4004: 'playback.formatUnsupported', // ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES
  4005: 'playback.formatUnsupported' // ERROR_CODE_DECODING_FORMAT_UNSUPPORTED
}

function exoplayerFamily(code: number): ErrorCode {
  if (code >= 6000) return 'playback.decodeFailed' // DRM: il contenuto non si apre
  if (code >= 5000) return 'playback.deviceLost' // AudioTrack: è il dispositivo audio
  if (code >= 4000) return 'playback.decodeFailed'
  if (code >= 3000) return 'playback.decodeFailed' // container o manifest malformato
  if (code >= 2000) return 'net.offline' // I/O: sorgente non raggiungibile
  if (code === 1001) return 'playback.engineUnavailable' // REMOTE_ERROR
  if (code === 1004) return 'internal.invariantViolated' // FAILED_RUNTIME_CHECK: è un bug
  return 'internal.unexpected'
}

export interface ExoPlayerFailure {
  readonly errorCode: number
  readonly errorCodeName?: string
  readonly message?: string
  readonly trackId?: number
  readonly path?: string
  readonly format?: string
}

export function fromExoPlayerError(failure: ExoPlayerFailure): AppError {
  const code = EXOPLAYER_SPECIFIC[failure.errorCode] ?? exoplayerFamily(failure.errorCode)
  return buildPlaybackError(
    code,
    failure,
    'exoplayer',
    failure.errorCodeName ?? String(failure.errorCode),
    failure.message
  )
}

interface TrackContext {
  readonly trackId?: number
  readonly path?: string
  readonly format?: string
}

function trackParams(source: TrackContext): { trackId?: number; path?: string; format?: string } {
  return {
    ...(source.trackId !== undefined ? { trackId: source.trackId } : {}),
    ...(source.path !== undefined ? { path: source.path } : {}),
    ...(source.format !== undefined ? { format: source.format } : {})
  }
}

/**
 * Costruisce l'errore con i parametri che il suo codice accetta.
 *
 * I codici del catalogo hanno parametri tipizzati diversi — `playback.decodeFailed`
 * prende trackId e format, `fs.permissionDenied` prende path — quindi lo switch è
 * necessario e non è una svista: passare parametri che il codice non dichiara non
 * compilerebbe, ed è la proprietà che si vuole.
 */
function buildPlaybackError(
  code: ErrorCode,
  source: TrackContext,
  engine: 'howler' | 'exoplayer',
  raw: string,
  message?: string
): AppError {
  const context = {
    engine,
    raw,
    ...(message !== undefined ? { engineMessage: message } : {}),
    ...(source.trackId !== undefined ? { trackId: source.trackId } : {})
  }
  const params = trackParams(source)

  switch (code) {
    case 'playback.decodeFailed':
      return AppError.of(
        'playback.decodeFailed',
        {
          ...(params.trackId !== undefined ? { trackId: params.trackId } : {}),
          ...(params.format !== undefined ? { format: params.format } : {})
        },
        { context }
      )
    case 'playback.sourceUnavailable':
      return AppError.of(
        'playback.sourceUnavailable',
        {
          ...(params.trackId !== undefined ? { trackId: params.trackId } : {}),
          ...(params.path !== undefined ? { path: params.path } : {})
        },
        { context }
      )
    case 'playback.formatUnsupported':
      return AppError.of(
        'playback.formatUnsupported',
        params.format !== undefined ? { format: params.format } : {},
        { context }
      )
    case 'fs.permissionDenied':
      return AppError.of('fs.permissionDenied', { path: params.path ?? '' }, { context })
    case 'net.timeout':
      return AppError.of('net.timeout', {}, { context })
    case 'net.offline':
      return AppError.of('net.offline', {}, { context })
    case 'playback.deviceLost':
      return AppError.of('playback.deviceLost', {}, { context })
    case 'playback.engineUnavailable':
      return AppError.of('playback.engineUnavailable', {}, { context })
    case 'playback.autoplayBlocked':
      return AppError.of('playback.autoplayBlocked', {}, { context })
    case 'internal.aborted':
      return AppError.of('internal.aborted', { what: 'riproduzione' }, { context })
    case 'internal.invariantViolated':
      return AppError.of(
        'internal.invariantViolated',
        { what: `il motore audio ha segnalato un controllo interno fallito (${raw})` },
        { context }
      )
    default:
      return AppError.of('internal.unexpected', { detail: `${engine}: ${raw}` }, { context })
  }
}
