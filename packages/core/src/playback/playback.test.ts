import { describe, expect, it } from 'vitest'
import { AppError } from '../errors'
import { fromExoPlayerError, fromHowlerError } from './errors'
import {
  MAX_PLAYBACK_ATTEMPTS,
  decidePlaybackRecovery,
  type RecoveryContext
} from './recovery'
import {
  INITIAL_PLAYBACK_STATE,
  STALL_THRESHOLD_MS,
  isDistrusted,
  reduce,
  reduceAll,
  shouldDeclareStall,
  type PlaybackEvent,
  type PlaybackState
} from './state'

/** Porta la macchina a "sta suonando la traccia 1", che è il punto di partenza reale. */
function playing(overrides: Partial<PlaybackState> = {}): PlaybackState {
  const state = reduceAll(INITIAL_PLAYBACK_STATE, [
    { type: 'load', trackId: 1, autoplay: true },
    { type: 'loaded', durationMs: 200_000, at: 1000 }
  ])
  return { ...state, ...overrides }
}

describe('macchina a stati', () => {
  it('parte da idle, senza traccia', () => {
    expect(INITIAL_PLAYBACK_STATE.status).toBe('idle')
    expect(INITIAL_PLAYBACK_STATE.trackId).toBeNull()
  })

  it('distingue il caricamento dalla pausa — lo stato che nel legacy non esisteva', () => {
    // Nel legacy entrambi erano isPlaying: false, quindi la UI non poteva mostrare
    // un caricamento in corso e il pulsante play sembrava non rispondere.
    const loading = reduce(INITIAL_PLAYBACK_STATE, {
      type: 'load',
      trackId: 7,
      autoplay: true
    }).state
    expect(loading.status).toBe('loading')
    expect(loading.playIntent).toBe(true)

    const ready = reduce(loading, { type: 'loaded', durationMs: 1000, at: 5 }).state
    expect(ready.status).toBe('playing')
  })

  it('carica senza suonare quando non è richiesto', () => {
    const state = reduceAll(INITIAL_PLAYBACK_STATE, [
      { type: 'load', trackId: 7, autoplay: false },
      { type: 'loaded', durationMs: 1000, at: 5 }
    ])
    expect(state.status).toBe('ready')
    expect(state.playIntent).toBe(false)
  })

  it('non perde un play premuto DURANTE il caricamento', () => {
    // Nel legacy l'autoplay era deciso alla creazione dell'oggetto Howl, quindi un
    // play arrivato 100ms dopo non aveva effetto e l'utente ripremeva.
    const state = reduceAll(INITIAL_PLAYBACK_STATE, [
      { type: 'load', trackId: 7, autoplay: false },
      { type: 'play', at: 10 },
      { type: 'loaded', durationMs: 1000, at: 20 }
    ])
    expect(state.status).toBe('playing')
  })

  it('un pause durante il caricamento annulla l\'intenzione', () => {
    const state = reduceAll(INITIAL_PLAYBACK_STATE, [
      { type: 'load', trackId: 7, autoplay: true },
      { type: 'pause' },
      { type: 'loaded', durationMs: 1000, at: 20 }
    ])
    expect(state.status).toBe('ready')
  })

  it('avanza la posizione mentre suona', () => {
    const state = reduce(playing(), { type: 'progress', positionMs: 4200, at: 2000 }).state
    expect(state.positionMs).toBe(4200)
    expect(state.lastProgressAt).toBe(2000)
  })

  it('mette in pausa e riprende', () => {
    const paused = reduce(playing(), { type: 'pause' }).state
    expect(paused.status).toBe('paused')
    expect(paused.playIntent).toBe(false)

    const resumed = reduce(paused, { type: 'play', at: 3000 }).state
    expect(resumed.status).toBe('playing')
  })

  it('rifiuta le transizioni illegali NOMINANDO il motivo', () => {
    // Il punto: un evento ignorato in silenzio è il modo in cui una macchina a
    // stati diventa inaffidabile senza che nessuno lo noti.
    const fromIdle = reduce(INITIAL_PLAYBACK_STATE, { type: 'play', at: 1 })
    expect(fromIdle.ignored?.event).toBe('play')
    expect(fromIdle.ignored?.reason).toContain('nessuna traccia')
    expect(fromIdle.state).toBe(INITIAL_PLAYBACK_STATE)

    const pauseWhenIdle = reduce(INITIAL_PLAYBACK_STATE, { type: 'pause' })
    expect(pauseWhenIdle.ignored?.reason).toContain('non è in riproduzione')

    const progressWhenPaused = reduce(reduce(playing(), { type: 'pause' }).state, {
      type: 'progress',
      positionMs: 10,
      at: 10
    })
    expect(progressWhenPaused.ignored?.event).toBe('progress')
  })

  it('la fine della traccia non è un errore e conserva l\'intenzione di suonare', () => {
    const ended = reduce(playing(), { type: 'ended' }).state
    expect(ended.status).toBe('idle')
    expect(ended.playIntent).toBe(true)
    expect(ended.attempt).toBe(0)
  })

  it('cambiare traccia azzera i tentativi, ricaricare la stessa no', () => {
    const failed = reduceAll(playing({ attempt: 2 }), [
      { type: 'failed', error: AppError.of('net.offline') }
    ])

    const sameTrack = reduce(failed, { type: 'load', trackId: 1, autoplay: true }).state
    expect(sameTrack.attempt).toBe(2)

    const otherTrack = reduce(failed, { type: 'load', trackId: 2, autoplay: true }).state
    expect(otherTrack.attempt).toBe(0)
  })

  it('stop azzera tutto tranne la sfiducia, che è di sessione', () => {
    const state = reduceAll(playing(), [
      { type: 'distrust', trackId: 42 },
      { type: 'stop' }
    ])
    expect(state.status).toBe('idle')
    expect(state.trackId).toBeNull()
    expect(state.distrusted).toEqual([42])
  })

  it('dare fiducia a una traccia le restituisce i tentativi', () => {
    const state = reduceAll(playing({ attempt: 3 }), [
      { type: 'distrust', trackId: 1 },
      { type: 'trust', trackId: 1 }
    ])
    expect(isDistrusted(state, 1)).toBe(false)
    expect(state.attempt).toBe(0)
  })

  it('sfiduciare due volte la stessa traccia non la duplica', () => {
    const state = reduceAll(playing(), [
      { type: 'distrust', trackId: 5 },
      { type: 'distrust', trackId: 5 }
    ])
    expect(state.distrusted).toEqual([5])
  })
})

describe('stallo', () => {
  it('lo dichiara solo dopo la soglia, e solo se suonava', () => {
    const state = reduce(playing(), { type: 'progress', positionMs: 1000, at: 10_000 }).state

    expect(shouldDeclareStall(state, 10_000 + STALL_THRESHOLD_MS - 1)).toBe(false)
    expect(shouldDeclareStall(state, 10_000 + STALL_THRESHOLD_MS)).toBe(true)

    const paused = reduce(state, { type: 'pause' }).state
    // Una pausa non è uno stallo: è la distinzione che nel legacy non esisteva.
    expect(shouldDeclareStall(paused, 10_000 + 60_000)).toBe(false)
  })

  it('uno stallo che si risolve da sé torna a suonare, azzerando i tentativi', () => {
    // Il buffer si è riempito: nessun intervento, nessun messaggio all'utente. E i
    // tentativi tornano a zero, altrimenti tre stalli distanti un'ora esaurirebbero
    // il budget della traccia.
    const stalled = reduceAll(playing({ attempt: 2 }), [{ type: 'stalled', at: 20_000 }])
    expect(stalled.status).toBe('stalled')

    const resumed = reduce(stalled, { type: 'progress', positionMs: 5000, at: 22_000 }).state
    expect(resumed.status).toBe('playing')
    expect(resumed.attempt).toBe(0)
  })

  it('da stalled si può recuperare, da playing no', () => {
    const stalled = reduce(playing(), { type: 'stalled', at: 20_000 }).state
    expect(reduce(stalled, { type: 'recover', sourceIndex: 0 }).state.status).toBe('recovering')
    expect(reduce(playing(), { type: 'recover', sourceIndex: 0 }).ignored?.event).toBe('recover')
  })

  it('un recupero conta come tentativo e ripulisce l\'errore', () => {
    const failed = reduce(playing(), {
      type: 'failed',
      error: AppError.of('net.offline')
    }).state
    const recovering = reduce(failed, { type: 'recover', sourceIndex: 1 }).state

    expect(recovering.status).toBe('recovering')
    expect(recovering.attempt).toBe(1)
    expect(recovering.sourceIndex).toBe(1)
    expect(recovering.error).toBeNull()
  })
})

describe('errori di Howler', () => {
  it.each([
    [1, 'internal.aborted'],
    [2, 'net.offline'],
    [3, 'playback.decodeFailed'],
    [4, 'playback.formatUnsupported']
  ])('il codice %i diventa %s, non la stringa "%i"', (code, expected) => {
    // Nel legacy tutti e quattro finivano in `loadError: "Titolo: <numero>"`.
    const error = fromHowlerError({ code, trackId: 9, format: 'flac' })
    expect(error.code).toBe(expected)
  })

  it('conserva il codice grezzo del motore nel contesto', () => {
    const error = fromHowlerError({ code: 3, trackId: 9, format: 'flac' })
    expect(error.context?.['engine']).toBe('howler')
    expect(error.context?.['raw']).toBe('3')
    expect(error.params['format']).toBe('flac')
  })

  it('un errore in riproduzione è quasi sempre il blocco dell\'autoplay', () => {
    // Il caso che nel legacy mostrava un errore dove serviva solo un clic.
    const error = fromHowlerError({ code: undefined, whilePlaying: true })
    expect(error.code).toBe('playback.autoplayBlocked')
    expect(error.severity).toBe('info')
  })

  it('un codice sconosciuto non fa perdere l\'informazione', () => {
    const error = fromHowlerError({ code: 99, trackId: 1 })
    expect(error.code).toBe('playback.decodeFailed')
    expect(error.context?.['raw']).toBe('99')
  })
})

describe('errori di ExoPlayer', () => {
  it.each([
    [2001, 'net.offline', 'connessione fallita'],
    [2002, 'net.timeout', 'timeout di rete'],
    [2005, 'playback.sourceUnavailable', 'file non trovato'],
    [2006, 'fs.permissionDenied', 'permesso negato'],
    [2007, 'playback.sourceUnavailable', 'cleartext non permesso'],
    [3001, 'playback.decodeFailed', 'container malformato'],
    [3003, 'playback.formatUnsupported', 'container non supportato'],
    [4003, 'playback.decodeFailed', 'decodifica fallita'],
    [4005, 'playback.formatUnsupported', 'formato non supportato'],
    [5001, 'playback.deviceLost', 'AudioTrack non inizializzato'],
    [1003, 'net.timeout', 'timeout'],
    [1001, 'playback.engineUnavailable', 'errore remoto']
  ])('%i (%s) diventa %s', (errorCode, expected) => {
    const error = fromExoPlayerError({ errorCode, trackId: 3, path: '/sdcard/x.mp3' })
    expect(error.code).toBe(expected)
  })

  it('un codice futuro finisce nella famiglia giusta, non nel catch-all', () => {
    // Media3 raggruppa per migliaia deliberatamente: un codice aggiunto da una
    // versione nuova della libreria resta interpretabile. È il modo in cui
    // `errorCode` era già andato perso una volta.
    expect(fromExoPlayerError({ errorCode: 2099 }).code).toBe('net.offline')
    expect(fromExoPlayerError({ errorCode: 4099 }).code).toBe('playback.decodeFailed')
    expect(fromExoPlayerError({ errorCode: 5099 }).code).toBe('playback.deviceLost')
  })

  it('un controllo interno fallito è un bug nostro, e lo dice', () => {
    const error = fromExoPlayerError({ errorCode: 1004 })
    expect(error.code).toBe('internal.invariantViolated')
    expect(error.severity).toBe('fatal')
  })

  it('conserva nome del codice e messaggio del motore', () => {
    const error = fromExoPlayerError({
      errorCode: 4003,
      errorCodeName: 'ERROR_CODE_DECODING_FAILED',
      message: 'Decoder init failed: OMX.google.aac.decoder',
      trackId: 3
    })
    expect(error.context?.['raw']).toBe('ERROR_CODE_DECODING_FAILED')
    expect(String(error.context?.['engineMessage'])).toContain('OMX')
    expect(error.params['trackId']).toBe(3)
  })
})

describe('politica di recupero', () => {
  function ctx(overrides: Partial<RecoveryContext> = {}): RecoveryContext {
    return {
      state: playing(),
      error: AppError.of('net.offline'),
      sourceCount: 1,
      hasNext: true,
      ...overrides
    }
  }

  it('ritenta un errore transitorio, con attesa crescente e senza avvisare', () => {
    // Un nuovo tentativo riuscito non è una notizia: riempire l'interfaccia di
    // avvisi per cose risolte da sé è il modo di far ignorare quelli che contano.
    const first = decidePlaybackRecovery(ctx({ state: playing({ attempt: 0 }) }))
    expect(first.action).toBe('retry')
    expect(first.notify).toBe(false)
    expect(first.delayMs).toBeGreaterThan(0)

    const second = decidePlaybackRecovery(ctx({ state: playing({ attempt: 1 }) }))
    expect(second.delayMs).toBeGreaterThan(first.delayMs)
  })

  it('passa alla sorgente successiva quando la prima è inutilizzabile', () => {
    // Un file locale illeggibile può essere lo stesso brano raggiungibile via
    // server media: arrendersi alla prima sorgente butta via quella possibilità.
    const decision = decidePlaybackRecovery(
      ctx({ error: AppError.of('playback.decodeFailed', { trackId: 1 }), sourceCount: 2 })
    )
    expect(decision.action).toBe('fallbackSource')
    expect(decision.sourceIndex).toBe(1)
    expect(decision.distrust).toBe(false)
  })

  it('non ritenta un formato non supportato: non guarirà', () => {
    const decision = decidePlaybackRecovery(
      ctx({ error: AppError.of('playback.formatUnsupported', { format: 'ape' }) })
    )
    expect(decision.action).toBe('skip')
    expect(decision.distrust).toBe(true)
    expect(decision.notify).toBe(true)
  })

  it('salta e sfiducia quando nessuna sorgente funziona', () => {
    const decision = decidePlaybackRecovery(
      ctx({ error: AppError.of('playback.sourceUnavailable', { trackId: 1 }) })
    )
    expect(decision.action).toBe('skip')
    expect(decision.distrust).toBe(true)
  })

  it('esaurisce i tentativi e poi passa avanti', () => {
    const decision = decidePlaybackRecovery(
      ctx({ state: playing({ attempt: MAX_PLAYBACK_ATTEMPTS }) })
    )
    expect(decision.action).toBe('skip')
    expect(String(decision.reason)).toContain('esauriti')
  })

  it('dopo i tentativi prova ancora una sorgente alternativa, se c\'è', () => {
    const decision = decidePlaybackRecovery(
      ctx({ state: playing({ attempt: MAX_PLAYBACK_ATTEMPTS, sourceIndex: 0 }), sourceCount: 2 })
    )
    expect(decision.action).toBe('fallbackSource')
    expect(decision.sourceIndex).toBe(1)
  })

  it('NON salta in cerchio: senza traccia successiva si ferma e lo dice', () => {
    // Senza questo controllo, "salta la traccia rotta" su una cartella di file
    // corrotti attraversa l'intera coda in un secondo e ricomincia.
    const decision = decidePlaybackRecovery(
      ctx({ error: AppError.of('playback.decodeFailed', { trackId: 1 }), hasNext: false })
    )
    expect(decision.action).toBe('stop')
    expect(decision.notify).toBe(true)
    expect(decision.reason).toContain('successiva')
  })

  it('il blocco dell\'autoplay aspetta un gesto, non salta la traccia', () => {
    const decision = decidePlaybackRecovery(
      ctx({ error: AppError.of('playback.autoplayBlocked') })
    )
    expect(decision.action).toBe('awaitGesture')
    expect(decision.distrust).toBe(false)
    expect(decision.notify).toBe(true)
  })

  it('un annullamento non produce né avvisi né sfiducia', () => {
    // È l'utente che ha cambiato traccia mentre la precedente caricava. Nel legacy
    // diventava «Titolo: 1» in faccia all'utente.
    const decision = decidePlaybackRecovery(
      ctx({ error: AppError.of('internal.aborted', { what: 'riproduzione' }) })
    )
    expect(decision.action).toBe('stop')
    expect(decision.notify).toBe(false)
    expect(decision.distrust).toBe(false)
  })

  it('un motore audio assente si ferma subito: il problema è a monte', () => {
    const decision = decidePlaybackRecovery(
      ctx({ error: AppError.of('playback.engineUnavailable') })
    )
    expect(decision.action).toBe('stop')
    expect(decision.distrust).toBe(false)
    expect(decision.notify).toBe(true)
  })

  it('un dispositivo audio perso si ritenta: spesso torna', () => {
    // Un\'altra app ha preso l\'uscita audio, o le cuffie si sono scollegate.
    const decision = decidePlaybackRecovery(ctx({ error: AppError.of('playback.deviceLost') }))
    expect(decision.action).toBe('retry')
  })
})

describe('percorso completo: file corrotto in mezzo a un album', () => {
  it('salta la traccia rotta, la sfiducia e continua', () => {
    // È il caso che nel legacy fermava l'ascolto con «Titolo: 3» e lasciava
    // all'utente il compito di capire cosa fare.
    let state = reduceAll(INITIAL_PLAYBACK_STATE, [
      { type: 'load', trackId: 10, autoplay: true },
      { type: 'loaded', durationMs: 200_000, at: 0 },
      { type: 'progress', positionMs: 60_000, at: 60_000 }
    ])
    expect(state.status).toBe('playing')

    // A metà traccia il decoder si arrende.
    const error = fromHowlerError({ code: 3, trackId: 10, format: 'flac' })
    state = reduce(state, { type: 'failed', error }).state
    expect(state.status).toBe('error')

    const decision = decidePlaybackRecovery({ state, error, sourceCount: 1, hasNext: true })
    expect(decision.action).toBe('skip')
    expect(decision.distrust).toBe(true)

    // Il chiamante applica la decisione: sfiducia e carica la successiva.
    const events: PlaybackEvent[] = [
      { type: 'distrust', trackId: 10 },
      { type: 'load', trackId: 11, autoplay: true },
      { type: 'loaded', durationMs: 180_000, at: 61_000 }
    ]
    state = reduceAll(state, events)

    expect(state.status).toBe('playing')
    expect(state.trackId).toBe(11)
    expect(isDistrusted(state, 10)).toBe(true)
    // I tentativi appartengono alla traccia nuova, non a quella rotta.
    expect(state.attempt).toBe(0)
  })

  it('una rete che va e viene non interrompe l\'ascolto', () => {
    let state = playing()
    const error = AppError.of('net.offline')

    for (let attempt = 0; attempt < MAX_PLAYBACK_ATTEMPTS; attempt++) {
      state = reduce(state, { type: 'failed', error }).state
      const decision = decidePlaybackRecovery({ state, error, sourceCount: 1, hasNext: true })
      expect(decision.action).toBe('retry')
      state = reduce(state, { type: 'recover', sourceIndex: decision.sourceIndex }).state
      expect(state.status).toBe('recovering')
    }

    // Al terzo tentativo la rete torna.
    state = reduce(state, { type: 'loaded', durationMs: 200_000, at: 90_000 }).state
    expect(state.status).toBe('playing')
    expect(state.error).toBeNull()
  })
})
