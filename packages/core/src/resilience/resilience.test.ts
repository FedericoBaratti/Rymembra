import { describe, expect, it, vi } from 'vitest'
import { AppError } from '../errors'
import { err, ok, type Result } from '../result'
import { CircuitBreaker } from './circuitBreaker'
import {
  decideRetry,
  normalizeDownloadError,
  MAX_ATTEMPTS,
  RATE_LIMIT_PAUSE_MS,
  RETRY_BASE_MS
} from './downloadPolicy'
import { RateLimiter } from './rateLimiter'
import { nextDelayMs, retryThrowing, withRetry, type RetryOptions } from './retry'
import { runFallible, sleep } from './run'
import { createDeadline, withDeadline } from './timeout'

/** Attese finte: i test misurano la strategia, non aspettano davvero. */
function fakeSleep(): {
  sleep: RetryOptions['sleep']
  waits: number[]
} {
  const waits: number[] = []
  return {
    waits,
    sleep: (ms) => {
      waits.push(ms)
      return Promise.resolve(ok(undefined))
    }
  }
}

/** Jitter disattivato: senza, i tempi attesi non sarebbero verificabili. */
const deterministic = { jitter: false } as const

describe('runFallible', () => {
  it('lascia passare un Result', async () => {
    expect(await runFallible(() => ok(3))).toEqual({ ok: true, value: 3 })
  })

  it.each([
    ['un Error', () => { throw new Error('boom') }, 'internal.unexpected'],
    ['una stringa legacy', () => { throw 'DL_NETWORK' }, 'download.network'],
    ['un errno', () => { throw Object.assign(new Error('x'), { code: 'ENOSPC' }) }, 'fs.diskFull'],
    ['un null', () => { throw null }, 'internal.unexpected']
  ])('cattura %s e lo classifica', async (_label, thrower, code) => {
    const result = await runFallible(thrower as () => Result<never, AppError>)
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe(code)
  })
})

describe('withRetry', () => {
  it('non ritenta ciò che il catalogo dice non ritentabile', async () => {
    // Nel legacy la risposta veniva da isRetryableError, che sapeva ispezionare
    // solo le classi di errore di net/: un ENOSPC o un 404 dal downloader non
    // avevano nessuno a rispondere per loro.
    const fn = vi.fn(() => err(AppError.of('net.http', { status: 404, url: 'x' })))
    const result = await withRetry(fn, { retries: 3, ...fakeSleep() })

    expect(fn).toHaveBeenCalledTimes(1)
    expect(result.ok).toBe(false)
  })

  it.each([
    ['un 503', AppError.of('net.http', { status: 503, url: 'x' }), true],
    ['un 429', AppError.of('net.http', { status: 429, url: 'x' }), true],
    ['un 408', AppError.of('net.http', { status: 408, url: 'x' }), true],
    ['un 404', AppError.of('net.http', { status: 404, url: 'x' }), false],
    ['un 401', AppError.of('net.http', { status: 401, url: 'x' }), false],
    ['offline', AppError.of('net.offline'), true],
    ['uno schema invalido', AppError.of('net.badSchema', {}), false],
    ['un disco pieno', AppError.of('fs.diskFull', {}), false]
  ])('%s: ritenta = %s', async (_label, error, shouldRetry) => {
    const fn = vi.fn(() => err(error))
    await withRetry(fn, { retries: 2, ...fakeSleep() })
    expect(fn).toHaveBeenCalledTimes(shouldRetry ? 3 : 1)
  })

  it('cresce in modo geometrico fino al tetto', async () => {
    const timing = fakeSleep()
    await withRetry(() => err(AppError.of('net.offline')), {
      retries: 6,
      baseDelayMs: 500,
      maxDelayMs: 4000,
      ...deterministic,
      sleep: timing.sleep
    })
    expect(timing.waits).toEqual([500, 1000, 2000, 4000, 4000, 4000])
  })

  it('il jitter resta nella metà bassa dell\'intervallo', () => {
    const error = AppError.of('net.offline')
    const opts = { baseDelayMs: 1000, maxDelayMs: 60_000 }
    expect(nextDelayMs(0, error, { ...opts, random: () => 0 })).toBe(500)
    expect(nextDelayMs(0, error, { ...opts, random: () => 1 })).toBe(1000)
  })

  it('rispetta il Retry-After dell\'errore, non il proprio calendario', async () => {
    const timing = fakeSleep()
    // La chiave del cambio: nel legacy il suggerimento era leggibile solo da un
    // `err instanceof RateLimitError`. Ora è un parametro dell'errore, quindi
    // qualunque codice può portarlo.
    await withRetry(() => err(AppError.of('net.rateLimited', { retryAfterMs: 9000 })), {
      retries: 1,
      baseDelayMs: 500,
      maxDelayMs: 15_000,
      ...deterministic,
      sleep: timing.sleep
    })
    expect(timing.waits).toEqual([9000])
  })

  it('un 429 può aspettare oltre maxDelayMs, ma non oltre quattro volte tanto', async () => {
    const timing = fakeSleep()
    await withRetry(
      () => err(AppError.of('net.rateLimited', { retryAfterMs: 10 * 60_000 })),
      { retries: 1, maxDelayMs: 15_000, ...deterministic, sleep: timing.sleep }
    )
    expect(timing.waits).toEqual([60_000])
  })

  it('restituisce il valore appena arriva, senza attese superflue', async () => {
    const timing = fakeSleep()
    let calls = 0
    const result = await withRetry(
      () => {
        calls++
        return calls < 3 ? err(AppError.of('net.timeout', {})) : ok('finito')
      },
      { retries: 5, ...deterministic, sleep: timing.sleep }
    )
    expect(result).toEqual({ ok: true, value: 'finito' })
    expect(timing.waits).toHaveLength(2)
  })

  it('conserva l\'ultimo errore e dice quanti tentativi ha bruciato', async () => {
    const result = await withRetry(
      () => err(AppError.of('net.http', { status: 503, url: 'https://mb.org' })),
      { retries: 2, what: 'musicbrainz.lookup', ...fakeSleep() }
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      // L'identità dell'errore non si perde nel ciclo di retry.
      expect(result.error.code).toBe('net.http')
      expect(result.error.params['status']).toBe(503)
      expect(result.error.context?.['attempts']).toBe(3)
      expect(result.error.context?.['operation']).toBe('musicbrainz.lookup')
      expect(result.error.context?.['retriesExhausted']).toBe(true)
    }
  })

  it('un throw dentro la funzione non rompe il ciclo', async () => {
    let calls = 0
    const result = await withRetry(
      () => {
        calls++
        if (calls === 1) throw Object.assign(new Error('rete giù'), { code: 'ECONNREFUSED' })
        return ok('ok')
      },
      { retries: 2, ...fakeSleep() }
    )
    expect(result).toEqual({ ok: true, value: 'ok' })
    expect(calls).toBe(2)
  })

  it('avvisa a ogni ritentativo, con errore e attesa', async () => {
    const onRetry = vi.fn()
    await withRetry(() => err(AppError.of('net.offline')), {
      retries: 2,
      baseDelayMs: 100,
      ...deterministic,
      sleep: fakeSleep().sleep,
      onRetry
    })
    expect(onRetry).toHaveBeenCalledTimes(2)
    expect(onRetry.mock.calls[0]?.[0].code).toBe('net.offline')
    expect(onRetry.mock.calls[0]?.[2]).toBe(100)
    expect(onRetry.mock.calls[1]?.[2]).toBe(200)
  })

  it('passa il numero di tentativo alla funzione', async () => {
    const seen: number[] = []
    await withRetry(
      (attempt) => {
        seen.push(attempt)
        return err(AppError.of('net.offline'))
      },
      { retries: 2, ...fakeSleep() }
    )
    expect(seen).toEqual([0, 1, 2])
  })

  it('si ferma subito se il segnale è già annullato', async () => {
    const controller = new AbortController()
    controller.abort()
    const fn = vi.fn(() => ok(1))

    const result = await withRetry(fn, { signal: controller.signal, what: 'scan' })

    expect(fn).not.toHaveBeenCalled()
    expect(result.ok).toBe(false)
    if (!result.ok) {
      // Annullare non è un guasto: severità info e non ritentabile.
      expect(result.error.code).toBe('internal.aborted')
      expect(result.error.severity).toBe('info')
      expect(result.error.retryable).toBe(false)
      expect(result.error.params['what']).toBe('scan')
    }
  })

  it('se l\'annullamento arriva durante l\'attesa vince sull\'errore in corso', async () => {
    const controller = new AbortController()
    const result = await withRetry(() => err(AppError.of('net.offline')), {
      retries: 3,
      signal: controller.signal,
      sleep: () => {
        controller.abort()
        return sleep(0, controller.signal)
      }
    })
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('internal.aborted')
  })

  it('retryThrowing avvolge il codice che ancora lancia', async () => {
    let calls = 0
    const result = await retryThrowing(
      async () => {
        calls++
        if (calls < 2) throw Object.assign(new Error('giù'), { code: 'ETIMEDOUT' })
        return 'contenuto'
      },
      { retries: 2, ...fakeSleep() }
    )
    expect(result).toEqual({ ok: true, value: 'contenuto' })
  })
})

describe('sleep', () => {
  it('si risolve dopo l\'attesa', async () => {
    expect(await sleep(1)).toEqual({ ok: true, value: undefined })
  })

  it('si interrompe se il segnale scatta durante l\'attesa', async () => {
    const controller = new AbortController()
    const waiting = sleep(10_000, controller.signal, 'download')
    controller.abort()
    const result = await waiting
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('internal.aborted')
  })
})

describe('CircuitBreaker', () => {
  const fail = (): Result<never, AppError> => err(AppError.of('net.http', { status: 500, url: 'x' }))

  function breaker(overrides: Partial<{ now: () => number }> = {}) {
    let clock = 1_000_000
    const b = new CircuitBreaker({
      name: 'musicbrainz',
      failureThreshold: 3,
      cooldownMs: 60_000,
      now: overrides.now ?? (() => clock)
    })
    return { b, advance: (ms: number) => (clock += ms) }
  }

  it('resta chiuso sotto la soglia', async () => {
    const { b } = breaker()
    await b.exec(fail)
    await b.exec(fail)
    expect(b.state).toBe('closed')
    expect(b.failureCount).toBe(2)
  })

  it('si apre alla soglia e poi fallisce subito, senza chiamare', async () => {
    const { b } = breaker()
    const fn = vi.fn(fail)
    for (let i = 0; i < 3; i++) await b.exec(fn)
    expect(b.state).toBe('open')

    const blocked = await b.exec(fn)
    expect(fn).toHaveBeenCalledTimes(3)
    expect(blocked.ok).toBe(false)
    if (!blocked.ok) {
      expect(blocked.error.code).toBe('net.circuitOpen')
      expect(blocked.error.params['service']).toBe('musicbrainz')
      // Porta il tempo d'attesa: chi ritenta non deve bussare a vuoto.
      expect(blocked.error.params['retryAfterMs']).toBe(60_000)
    }
  })

  it('un successo azzera il conteggio', async () => {
    const { b } = breaker()
    await b.exec(fail)
    await b.exec(fail)
    await b.exec(() => ok('bene'))
    expect(b.failureCount).toBe(0)
    expect(b.state).toBe('closed')
  })

  it('non conta i guasti che riguardano la richiesta, non il servizio', async () => {
    // Parità esatta con countsAsFailure del legacy, ma dedotta dal catalogo:
    // 4xx e schema invalido non dicono niente sulla salute del servizio.
    const { b } = breaker()
    for (let i = 0; i < 5; i++) {
      await b.exec(() => err(AppError.of('net.http', { status: 404, url: 'x' })))
      await b.exec(() => err(AppError.of('net.badSchema', {})))
    }
    expect(b.failureCount).toBe(0)
    expect(b.state).toBe('closed')
  })

  it('dopo il cooldown passa una sola sonda', async () => {
    const { b, advance } = breaker()
    for (let i = 0; i < 3; i++) await b.exec(fail)
    advance(60_000)
    expect(b.state).toBe('half-open')

    // Due chiamate in volo insieme: solo la prima è la sonda.
    let release: (() => void) | undefined
    const gate = new Promise<void>((resolve) => (release = resolve))
    const probe = b.exec(async () => {
      await gate
      return ok('vivo')
    })
    const second = await b.exec(() => ok('non dovrebbe passare'))

    expect(second.ok).toBe(false)
    if (!second.ok) expect(second.error.code).toBe('net.circuitOpen')

    release?.()
    expect(await probe).toEqual({ ok: true, value: 'vivo' })
    expect(b.state).toBe('closed')
  })

  it('se la sonda cade per un motivo che non conta, riarma il cooldown', async () => {
    // Il caso che il legacy aveva già preso: senza il riarmo, l'interruttore
    // resterebbe semiaperto e OGNI chiamata successiva passerebbe come nuova
    // sonda — cioè non interromperebbe più niente.
    const { b, advance } = breaker()
    for (let i = 0; i < 3; i++) await b.exec(fail)
    advance(60_000)
    expect(b.state).toBe('half-open')

    await b.exec(() => err(AppError.of('net.http', { status: 404, url: 'x' })))
    expect(b.state).toBe('open')
  })

  it('non conta l\'apertura di un interruttore a monte', async () => {
    // Due interruttori in serie non devono aprirsi a vicenda a catena.
    const { b } = breaker()
    for (let i = 0; i < 5; i++) {
      await b.exec(() => err(AppError.of('net.circuitOpen', { service: 'a monte' })))
    }
    expect(b.state).toBe('closed')
  })

  it('cattura un throw e lo conta come guasto se è ritentabile', async () => {
    const { b } = breaker()
    for (let i = 0; i < 3; i++) {
      await b.exec(() => { throw Object.assign(new Error('giù'), { code: 'ECONNREFUSED' }) })
    }
    expect(b.state).toBe('open')
  })

  it('annuncia i cambi di stato una volta sola', async () => {
    const onStateChange = vi.fn()
    const b = new CircuitBreaker({
      name: 'spotify',
      failureThreshold: 2,
      onStateChange
    })
    await b.exec(fail)
    await b.exec(fail)
    await b.exec(fail)
    expect(onStateChange.mock.calls.map((c) => c[0])).toEqual(['open'])

    b.reset()
    expect(onStateChange.mock.calls.map((c) => c[0])).toEqual(['open', 'closed'])
  })

  it('composto con withRetry: l\'apertura ferma i tentativi', async () => {
    const b = new CircuitBreaker({ name: 'yt', failureThreshold: 2, cooldownMs: 60_000 })
    const fn = vi.fn(fail)
    const result = await withRetry(() => b.exec(fn), { retries: 5, ...fakeSleep() })

    // Due chiamate vere, poi l'interruttore risponde da sé.
    expect(fn).toHaveBeenCalledTimes(2)
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('net.circuitOpen')
  })
})

describe('RateLimiter', () => {
  it('esegue in ordine di arrivo', async () => {
    const limiter = new RateLimiter({ name: 'mb', minIntervalMs: 0 })
    const order: number[] = []
    await Promise.all(
      [1, 2, 3, 4].map((n) =>
        limiter.schedule(() => {
          order.push(n)
          return ok(n)
        })
      )
    )
    expect(order).toEqual([1, 2, 3, 4])
    limiter.dispose()
  })

  it('non supera la concorrenza massima', async () => {
    const limiter = new RateLimiter({ name: 'fs', minIntervalMs: 0, maxConcurrent: 2 })
    let peak = 0
    let current = 0
    const tasks = Array.from({ length: 8 }, () =>
      limiter.schedule(async () => {
        current++
        peak = Math.max(peak, current)
        await new Promise((r) => setTimeout(r, 1))
        current--
        return ok(true)
      })
    )
    await Promise.all(tasks)
    expect(peak).toBe(2)
    limiter.dispose()
  })

  it('un throw sincrono non lascia il permesso occupato', async () => {
    const limiter = new RateLimiter({ name: 'mb', minIntervalMs: 0 })
    const bad = await limiter.schedule(() => { throw new Error('esploso subito') })
    expect(bad.ok).toBe(false)
    // Se il permesso fosse rimasto occupato, questa non si risolverebbe mai.
    expect(await limiter.schedule(() => ok('dopo'))).toEqual({ ok: true, value: 'dopo' })
    expect(limiter.pending).toBe(0)
    limiter.dispose()
  })

  it('distanzia gli avvii dell\'intervallo minimo', async () => {
    const limiter = new RateLimiter({ name: 'mb', minIntervalMs: 20 })
    const starts: number[] = []
    const t0 = Date.now()
    await Promise.all(
      [0, 1, 2].map(() =>
        limiter.schedule(() => {
          starts.push(Date.now() - t0)
          return ok(true)
        })
      )
    )
    expect(starts).toHaveLength(3)
    // Margine generoso: si verifica la distanza, non la precisione del timer.
    expect(starts[2] ?? 0).toBeGreaterThanOrEqual(30)
    limiter.dispose()
  })

  it('notifyRateLimited sposta il permesso per tutta la coda', async () => {
    const limiter = new RateLimiter({ name: 'mb', minIntervalMs: 1 })
    limiter.notifyRateLimited(40)
    const t0 = Date.now()
    await limiter.schedule(() => ok(true))
    expect(Date.now() - t0).toBeGreaterThanOrEqual(25)
    limiter.dispose()
  })

  it('conta ciò che è in attesa, per la diagnostica', async () => {
    const limiter = new RateLimiter({ name: 'mb', minIntervalMs: 0, maxConcurrent: 1 })
    let release: (() => void) | undefined
    const gate = new Promise<void>((resolve) => (release = resolve))
    const first = limiter.schedule(async () => {
      await gate
      return ok(1)
    })
    const second = limiter.schedule(() => ok(2))

    expect(limiter.pending).toBe(2)
    expect(limiter.inFlight).toBe(1)

    release?.()
    await Promise.all([first, second])
    expect(limiter.pending).toBe(0)
    limiter.dispose()
  })
})

describe('withDeadline', () => {
  it('lascia passare il lavoro che finisce in tempo', async () => {
    expect(await withDeadline(() => ok('svelto'), { timeoutMs: 500, what: 'db.open' })).toEqual({
      ok: true,
      value: 'svelto'
    })
  })

  it('libera il chiamante quando il lavoro non risponde', async () => {
    // Il fallimento peggiore è il silenzio: nel legacy il desktop non aveva
    // alcun rilevamento di stallo, e un getDb() appeso lasciava la UI sugli
    // scheletri per sempre.
    const result = await withDeadline(() => new Promise<Result<string, AppError>>(() => {}), {
      timeoutMs: 5,
      what: 'db.open'
    })
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('internal.timeout')
      expect(result.error.params['what']).toBe('db.open')
      expect(result.error.retryable).toBe(true)
    }
  })

  it('un throw resta un errore classificato, non una scadenza', async () => {
    const result = await withDeadline(
      () => { throw Object.assign(new Error('x'), { code: 'ENOENT' }) },
      { timeoutMs: 500, what: 'fs.stat' }
    )
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('fs.notFound')
  })
})

describe('createDeadline', () => {
  it('misura ciò che resta e scade una volta sola', () => {
    let clock = 0
    const deadline = createDeadline(1000, () => clock)
    expect(deadline.remainingMs()).toBe(1000)
    expect(deadline.check('upload').ok).toBe(true)

    clock = 400
    expect(deadline.remainingMs()).toBe(600)
    expect(deadline.expired()).toBe(false)

    clock = 1500
    expect(deadline.remainingMs()).toBe(0)
    expect(deadline.expired()).toBe(true)
    const check = deadline.check('commit')
    expect(check.ok).toBe(false)
    if (!check.ok) {
      expect(check.error.code).toBe('internal.timeout')
      expect(check.error.params['what']).toBe('commit')
    }
  })
})

describe('decideRetry', () => {
  it('si ferma subito su un errore permanente', () => {
    const decision = decideRetry(AppError.of('download.unrecognizedUrl'), 0)
    expect(decision.action).toBe('fail')
    expect(decision.failureClass).toBe('permanent')
    expect(decision.error.code).toBe('download.unrecognizedUrl')
  })

  it('tratta un errore sconosciuto come permanente', () => {
    // I bug che non sappiamo nominare non devono entrare in un ciclo di retry.
    const decision = decideRetry(new Error('boom'), 0)
    expect(decision.action).toBe('fail')
    expect(decision.failureClass).toBe('permanent')
  })

  it('arretra in modo esponenziale sui transitori: 30s, 60s, 120s', () => {
    for (const [attempts, expected] of [
      [0, RETRY_BASE_MS],
      [1, RETRY_BASE_MS * 2],
      [2, RETRY_BASE_MS * 4]
    ] as const) {
      const decision = decideRetry(AppError.of('download.network'), attempts)
      expect(decision).toMatchObject({
        action: 'retry',
        failureClass: 'transient',
        attempts: attempts + 1,
        delayMs: expected
      })
    }
  })

  it('si arrende dopo MAX_ATTEMPTS', () => {
    const decision = decideRetry(AppError.of('download.network'), MAX_ATTEMPTS)
    expect(decision.action).toBe('fail')
    expect(decision.failureClass).toBe('transient')
  })

  it('pausa di almeno cinque minuti sui rate limit, a qualunque tentativo', () => {
    const decision = decideRetry(AppError.of('download.rateLimited'), 2)
    expect(decision).toMatchObject({
      action: 'retry',
      failureClass: 'rate-limited',
      delayMs: RATE_LIMIT_PAUSE_MS
    })
  })

  it('collassa un pacchetto yt-dlp corrotto sul suo codice, e lo ritenta', () => {
    const raw = new Error("zipimport.ZipImportError: bad local file header: '/data/yt-dlp'")
    const decision = decideRetry(raw, 0)

    expect(decision).toMatchObject({ action: 'retry', failureClass: 'transient' })
    // Il traceback non si persiste: sul filo va il codice, la frase la fa la UI.
    expect(decision.error.code).toBe('download.ytdlpCorrupted')
    // Ma resta nei log, come causa.
    expect(decision.error.causes[0]?.message).toContain('zipimport')
  })

  it('anche al tetto dei tentativi il codice persistito resta quello', () => {
    const decision = decideRetry(new Error('zipimport: cannot open file'), MAX_ATTEMPTS)
    expect(decision.action).toBe('fail')
    expect(decision.error.code).toBe('download.ytdlpCorrupted')
  })

  it('riconosce il codice legacy già persistito in SQLite', () => {
    // Le righe di download salvate dalla versione precedente contengono la
    // stringa, non il codice: devono restare leggibili.
    expect(normalizeDownloadError('DL_RATE_LIMITED').code).toBe('download.rateLimited')
    expect(decideRetry('DL_RATE_LIMITED', 0).failureClass).toBe('rate-limited')
    expect(decideRetry('YTDLP_CORRUPTED', 0).error.code).toBe('download.ytdlpCorrupted')
    expect(decideRetry('DL_AGE_RESTRICTED', 0).failureClass).toBe('permanent')
  })

  it('non c\'è più un default divergente fra desktop e mobile', () => {
    // classifyDownloadFailure aveva default 'permanent' sul desktop e
    // 'transient' sul mobile: lo stesso errore veniva ritentato su un
    // dispositivo e no sull'altro. Ora la classe viene dal catalogo, che è uno.
    const codes = ['download.network', 'download.failed', 'download.ytdlpTimeout'] as const
    for (const code of codes) {
      expect(decideRetry(AppError.of(code), 0).failureClass).toBe('transient')
    }
    for (const code of ['download.forbidden', 'download.private'] as const) {
      expect(decideRetry(AppError.of(code), 0).failureClass).toBe('permanent')
    }
  })
})
