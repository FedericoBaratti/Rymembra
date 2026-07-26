import { describe, expect, it, vi } from 'vitest'
import { AppError } from './errors'
import { err, ok } from './result'
import {
  errorEnvelope,
  fromEnvelope,
  toEnvelope,
  unwrapEnvelope,
  withIpcTimeout,
  wrapHandler,
  type IpcEnvelope
} from './serialize'

/**
 * Simula il trasporto Electron: `ipcMain.handle` restituisce, il valore passa
 * per structured clone, il renderer lo riceve. JSON è più severo di structured
 * clone (perde Date, Map, undefined), quindi se il round-trip regge qui regge
 * anche là.
 */
async function viaElectron<T>(
  handler: (...a: never[]) => Promise<IpcEnvelope<T>>
): Promise<unknown> {
  const envelope = await handler()
  return JSON.parse(JSON.stringify(envelope))
}

/**
 * Simula il bridge nodejs-mobile: messaggi JSON stringificati su un canale,
 * correlati per id. È la forma di `{ t:'res', id, ok, ... }` in bridge.ts.
 */
async function viaMobileBridge<T>(
  handler: (...a: never[]) => Promise<IpcEnvelope<T>>
): Promise<unknown> {
  const envelope = await handler()
  const wire = JSON.stringify({ t: 'res', id: 7, envelope })
  const parsed = JSON.parse(wire) as { envelope: unknown }
  return parsed.envelope
}

describe('busta IPC', () => {
  it('impacchetta un valore nudo', () => {
    expect(toEnvelope(42)).toEqual({ ok: true, value: 42 })
  })

  it('impacchetta un Result, in entrambi gli esiti', () => {
    expect(toEnvelope(ok('x'))).toEqual({ ok: true, value: 'x' })

    const packed = toEnvelope(err(AppError.of('db.locked')))
    expect(packed.ok).toBe(false)
    if (!packed.ok) expect(packed.error.code).toBe('db.locked')
  })

  it('non confonde un valore che somiglia a un Result', () => {
    // Un oggetto di dominio con un campo `ok` booleano non deve essere
    // interpretato come Result: servono anche `value` o `error`.
    const payload = { ok: true, name: 'stato del server' }
    const envelope = toEnvelope(payload)
    expect(envelope).toEqual({ ok: true, value: payload })
  })

  it('scarta una risposta che non è una busta con un codice preciso', () => {
    const result = fromEnvelope(undefined)
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('ipc.backendUnreachable')
  })
})

describe('wrapHandler — un handler non lancia mai', () => {
  it.each([
    ['un AppError', () => { throw AppError.of('fs.notFound', { path: 'a.flac' }) }],
    ['un Error nudo', () => { throw new Error('boom') }],
    ['un errno di Node', () => { throw Object.assign(new Error('x'), { code: 'ENOSPC' }) }],
    ['una stringa', () => { throw 'DL_FORBIDDEN' }],
    ['un null', () => { throw null }],
    ['un numero', () => { throw 500 }],
    ['un rigetto asincrono', async () => { await Promise.reject(new Error('async boom')) }]
  ])('cattura %s e risponde con una busta', async (_label, handler) => {
    const wrapped = wrapHandler('test:channel', handler as () => never)
    const envelope = await wrapped()
    expect(envelope.ok).toBe(false)
    if (!envelope.ok) {
      expect(envelope.error.__aetherError).toBe(true)
      expect(typeof envelope.error.code).toBe('string')
      // Il canale finisce nel contesto: senza, un errore generico non dice da dove viene.
      expect(envelope.error.context?.['channel']).toBe('test:channel')
    }
  })

  it('lascia passare il valore quando va tutto bene', async () => {
    const wrapped = wrapHandler('getTracks', async () => [{ id: 1 }, { id: 2 }])
    const envelope = await wrapped()
    expect(envelope).toEqual({ ok: true, value: [{ id: 1 }, { id: 2 }] })
  })

  it('passa gli argomenti all\'handler', async () => {
    const spy = vi.fn((a: number, b: string) => `${a}${b}`)
    const wrapped = wrapHandler('concat', spy)
    await wrapped(1, 'x')
    expect(spy).toHaveBeenCalledWith(1, 'x')
  })
})

describe('round-trip sui due trasporti — il criterio della Fase 1', () => {
  // L'errore ricco che nel legacy arrivava in UI come la stringa
  // "Error invoking remote method 'enrichTrack': Error: 429".
  const richFailure = () =>
    wrapHandler('enrichTrack', () => {
      throw AppError.of(
        'net.rateLimited',
        { service: 'musicbrainz', retryAfterMs: 12_000 },
        { context: { trackId: 91 }, cause: new Error('429 Too Many Requests') }
      )
    })

  it.each([
    ['Electron', viaElectron],
    ['bridge nodejs-mobile', viaMobileBridge]
  ])('via %s conserva tutti i campi', async (_label, transport) => {
    const wire = await transport(richFailure())
    const result = fromEnvelope(wire)

    expect(result.ok).toBe(false)
    if (result.ok) return

    const e = result.error
    expect(e).toBeInstanceOf(AppError)
    expect(e.code).toBe('net.rateLimited')
    expect(e.domain).toBe('net')
    expect(e.severity).toBe('warning')
    expect(e.retryable).toBe(true)
    expect(e.params['retryAfterMs']).toBe(12_000)
    expect(e.params['service']).toBe('musicbrainz')
    expect(e.context?.['trackId']).toBe(91)
    expect(e.context?.['channel']).toBe('enrichTrack')
    expect(e.causes[0]?.message).toBe('429 Too Many Requests')
    expect(e.i18nKey).toBe('errors.net.rateLimited')
    expect(e.traceId).toBeTruthy()
  })

  it.each([
    ['Electron', viaElectron],
    ['bridge nodejs-mobile', viaMobileBridge]
  ])('via %s conserva la ritentabilità dedotta dallo status (%s)', async (_label, transport) => {
    const notFound = wrapHandler('fetchArt', () => {
      throw AppError.of('net.http', { status: 404, url: 'https://coverart.test/x' })
    })
    const result = fromEnvelope(await transport(notFound))
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.retryable).toBe(false)
      expect(result.error.params['status']).toBe(404)
    }
  })

  it('unwrapEnvelope rigetta con un AppError vero, non con una stringa', async () => {
    const wire = await viaElectron(richFailure())
    await expect(unwrapEnvelope(wire)).rejects.toBeInstanceOf(AppError)

    // La forma che i siti di chiamata nel renderer possono usare da subito.
    try {
      await unwrapEnvelope(wire)
      expect.unreachable('doveva rigettare')
    } catch (caught) {
      const e = AppError.from(caught)
      expect(e.retryable).toBe(true)
      expect(e.params['retryAfterMs']).toBe(12_000)
    }
  })

  it('unwrapEnvelope restituisce il valore quando la busta è positiva', async () => {
    const wire = await viaElectron(wrapHandler('count', () => 128))
    await expect(unwrapEnvelope<number>(wire)).resolves.toBe(128)
  })

  it('un errno di filesystem arriva in UI già classificato', async () => {
    // Nel legacy questo era un logWarn più un messaggio generico.
    const handler = wrapHandler('writeTags', () => {
      throw Object.assign(new Error('ENOSPC: no space left on device'), {
        code: 'ENOSPC',
        path: 'D:/musica/x.flac'
      })
    })
    const result = fromEnvelope(await viaElectron(handler))
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('fs.diskFull')
      expect(result.error.severity).toBe('error')
      expect(result.error.i18nKey).toBe('errors.fs.diskFull')
    }
  })
})

describe('withIpcTimeout — il silenzio è il guasto peggiore', () => {
  it('rigetta con backendUnreachable se il backend non risponde', async () => {
    vi.useFakeTimers()
    try {
      const pending = withIpcTimeout(new Promise<never>(() => {}), 'getTracks', 5000)
      const assertion = expect(pending).rejects.toMatchObject({
        code: 'ipc.backendUnreachable'
      })
      await vi.advanceTimersByTimeAsync(5001)
      await assertion
    } finally {
      vi.useRealTimers()
    }
  })

  it('lascia passare una risposta arrivata in tempo', async () => {
    await expect(withIpcTimeout(Promise.resolve('ok'), 'ping', 1000)).resolves.toBe('ok')
  })

  it('normalizza in AppError anche un rigetto del trasporto', async () => {
    await expect(
      withIpcTimeout(Promise.reject('DL_NETWORK'), 'download', 1000)
    ).rejects.toMatchObject({ code: 'download.network' })
  })

  it('non lascia timer appesi dopo il successo', async () => {
    vi.useFakeTimers()
    try {
      await withIpcTimeout(Promise.resolve(1), 'ping', 1000)
      expect(vi.getTimerCount()).toBe(0)
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('errorEnvelope', () => {
  it('impacchetta qualunque causa', () => {
    const envelope = errorEnvelope('YTDLP_BUSY')
    expect(envelope.ok).toBe(false)
    if (!envelope.ok) expect(envelope.error.code).toBe('download.ytdlpBusy')
  })
})
