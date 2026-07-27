import { describe, expect, it, vi } from 'vitest'
import { z } from 'zod'
import {
  assertHandlersComplete,
  channel,
  channelNames,
  defineHandlers,
  type ApiFor,
  type HandlerContext,
  type HandlerMap
} from './contract'
import { AppError } from './errors'
import { err, ok } from './result'

/** Contratto di prova: le stesse forme dei canali reali, in piccolo. */
const TEST_CONTRACT = {
  getSettings: channel(z.void(), z.object({ skin: z.string(), volume: z.number() })),
  setVolume: channel(z.object({ value: z.number().min(0).max(1) }), z.boolean()),
  getTrack: channel(
    z.object({ id: z.number().int().positive() }),
    z.object({ id: z.number(), title: z.string() })
  ),
  // Canale con validazione dell'uscita: il dato viene da fuori.
  importSkin: channel(
    z.object({ bytes: z.number() }),
    z.object({ id: z.string() }),
    { validateOutput: true }
  )
}

const ctx: HandlerContext = { origin: 'renderer' }

function handlers(
  overrides: Partial<HandlerMap<typeof TEST_CONTRACT>> = {}
): HandlerMap<typeof TEST_CONTRACT> {
  return {
    getSettings: () => ({ skin: 'plain', volume: 0.8 }),
    setVolume: () => true,
    getTrack: ({ id }) => ({ id, title: 'Traccia' }),
    importSkin: () => ({ id: 'nocturne' }),
    ...overrides
  }
}

describe('contratto', () => {
  it('deriva i nomi dei canali — sostituisce INVOKE_METHODS', () => {
    expect(channelNames(TEST_CONTRACT).sort()).toEqual([
      'getSettings',
      'getTrack',
      'importSkin',
      'setVolume'
    ])
  })

  it('deriva il tipo dell\'API dal contratto', () => {
    // Asserzione a livello di TIPI: la funzione non viene mai eseguita, conta
    // solo che compili (e che i @ts-expect-error scattino). Sostituisce le ~200
    // righe scritte a mano di AetherAPI e il cast `as unknown as` che le teneva
    // insieme senza garantire nulla.
    function _typeAssertions(api: ApiFor<typeof TEST_CONTRACT>): void {
      const settings: Promise<{ skin: string; volume: number }> = api.getSettings()
      const saved: Promise<boolean> = api.setVolume({ value: 0.5 })
      const track: Promise<{ id: number; title: string }> = api.getTrack({ id: 1 })
      void settings
      void saved
      void track

      // @ts-expect-error un canale con ingresso void non accetta argomenti
      api.getSettings({ nope: true })
      // @ts-expect-error il tipo dell'ingresso è verificato
      api.setVolume({ value: 'alto' })
      // @ts-expect-error un campo obbligatorio non può mancare
      api.getTrack({})
      // @ts-expect-error un canale inesistente non compila
      api.canaleCheNonEsiste()
    }

    expect(typeof _typeAssertions).toBe('function')
  })

  it('un handler mancante non compila', () => {
    const incomplete = {
      getSettings: () => ({ skin: 'plain', volume: 1 }),
      setVolume: () => true,
      getTrack: ({ id }: { id: number }) => ({ id, title: 'x' })
      // importSkin manca di proposito
    }
    // È il guadagno principale del contratto: nel legacy un canale dimenticato
    // era un reject a runtime, e su mobile un 'unknown channel: X'
    // indistinguibile da un guasto vero.
    // @ts-expect-error manca la proprietà importSkin
    defineHandlers(TEST_CONTRACT, incomplete)
  })
})

describe('validazione degli argomenti', () => {
  it('rifiuta un ingresso malformato con un codice preciso', async () => {
    // Nel legacy validava UN SOLO handler su 105 (thermal.ipc.ts).
    const bound = defineHandlers(TEST_CONTRACT, handlers())
    const result = await bound.setVolume({ value: 42 }, ctx)

    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('ipc.payloadInvalid')
      expect(result.error.params['channel']).toBe('setVolume')
      expect(String(result.error.params['detail'])).toContain('value')
    }
  })

  it.each([
    ['null', null],
    ['stringa al posto di oggetto', 'ciao'],
    ['campo assente', {}],
    ['id negativo', { id: -3 }],
    ['id non intero', { id: 1.5 }]
  ])('rifiuta %s senza raggiungere l\'handler', async (_label, input) => {
    const spy = vi.fn(({ id }: { id: number }) => ({ id, title: 'x' }))
    const bound = defineHandlers(TEST_CONTRACT, handlers({ getTrack: spy }))

    const result = await bound.getTrack(input, ctx)
    expect(result.ok).toBe(false)
    expect(spy).not.toHaveBeenCalled()
  })

  it('lascia passare un ingresso valido, già tipizzato', async () => {
    const bound = defineHandlers(TEST_CONTRACT, handlers())
    const result = await bound.getTrack({ id: 7 }, ctx)
    expect(result).toEqual({ ok: true, value: { id: 7, title: 'Traccia' } })
  })
})

describe('handler — qualunque esito diventa un Result', () => {
  it('accetta un valore nudo', async () => {
    const bound = defineHandlers(TEST_CONTRACT, handlers())
    const result = await bound.getSettings(undefined, ctx)
    expect(result).toEqual({ ok: true, value: { skin: 'plain', volume: 0.8 } })
  })

  it('accetta un Result positivo', async () => {
    const bound = defineHandlers(TEST_CONTRACT, handlers({ setVolume: () => ok(true) }))
    expect(await bound.setVolume({ value: 0.3 }, ctx)).toEqual({ ok: true, value: true })
  })

  it('accetta un Result negativo e conserva l\'errore', async () => {
    const bound = defineHandlers(TEST_CONTRACT, handlers({
      setVolume: () => err(AppError.of('playback.deviceLost'))
    }))
    const result = await bound.setVolume({ value: 0.3 }, ctx)
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('playback.deviceLost')
      expect(result.error.context?.['channel']).toBe('setVolume')
    }
  })

  it.each([
    ['un Error', () => { throw new Error('boom') }],
    ['una stringa legacy', () => { throw 'TRACK_NOT_FOUND' }],
    ['un errno', () => { throw Object.assign(new Error('x'), { code: 'ENOENT' }) }],
    ['un null', () => { throw null }]
  ])('cattura %s sollevato dall\'handler', async (_label, thrower) => {
    const bound = defineHandlers(TEST_CONTRACT, handlers({
      setVolume: thrower as () => boolean
    }))
    const result = await bound.setVolume({ value: 0.3 }, ctx)
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error).toBeInstanceOf(AppError)
      expect(result.error.context?.['channel']).toBe('setVolume')
      expect(result.error.context?.['origin']).toBe('renderer')
    }
  })

  it('cattura anche un rigetto asincrono', async () => {
    const bound = defineHandlers(TEST_CONTRACT, handlers({
      setVolume: async () => { await Promise.reject(new Error('async boom')); return true }
    }))
    const result = await bound.setVolume({ value: 0.3 }, ctx)
    expect(result.ok).toBe(false)
  })

  it('riceve il contesto della chiamata, compresa l\'origine LAN', async () => {
    const spy = vi.fn(() => true)
    const bound = defineHandlers(TEST_CONTRACT, handlers({ setVolume: spy }))
    await bound.setVolume({ value: 0.2 }, { origin: 'lan', deviceId: 'phone-1' })
    expect(spy).toHaveBeenCalledWith({ value: 0.2 }, { origin: 'lan', deviceId: 'phone-1' })
  })
})

describe('validazione dell\'uscita', () => {
  it('segnala come invariante violata un\'uscita fuori contratto', async () => {
    const bound = defineHandlers(TEST_CONTRACT, handlers({
      importSkin: () => ({ id: 42 } as unknown as { id: string })
    }))
    const result = await bound.importSkin({ bytes: 100 }, ctx)
    expect(result.ok).toBe(false)
    if (!result.ok) {
      // Un'uscita sbagliata è un bug nostro, non un input cattivo: il codice lo
      // dice, invece di far arrivare al renderer un dato della forma sbagliata.
      expect(result.error.code).toBe('internal.invariantViolated')
      expect(String(result.error.params['what'])).toContain('importSkin')
    }
  })

  it('non valida l\'uscita dove non richiesto', async () => {
    // Ri-validare 100k tracce a ogni chiamata costerebbe più della chiamata.
    const bound = defineHandlers(TEST_CONTRACT, handlers({
      getTrack: () => ({ id: 'sbagliato' } as unknown as { id: number; title: string })
    }))
    const result = await bound.getTrack({ id: 1 }, ctx)
    expect(result.ok).toBe(true)
  })
})

describe('assertHandlersComplete', () => {
  it('passa quando il contratto è coperto', () => {
    const bound = defineHandlers(TEST_CONTRACT, handlers())
    expect(assertHandlersComplete(TEST_CONTRACT, bound)).toEqual({ ok: true, value: true })
  })

  it('nomina i canali scoperti', () => {
    // Serve per i contratti assemblati da più moduli di dominio a runtime,
    // dove i tipi da soli non bastano.
    const partial = { getSettings: async () => ok(null) }
    const result = assertHandlersComplete(TEST_CONTRACT, partial)
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('ipc.handlerMissing')
      const listed = String(result.error.params['channel'])
      expect(listed).toContain('setVolume')
      expect(listed).toContain('getTrack')
      expect(listed).toContain('importSkin')
    }
  })
})
