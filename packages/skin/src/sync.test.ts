import { describe, expect, it } from 'vitest'
import { AppError, err, ok, type Result } from '@aether/core'
import { BUILTIN_SKIN_IDS, BUILTIN_SKIN_SOURCES, PLAIN_SKIN_SOURCE } from './builtin'
import { alignLibraries, libraryEntryFor, type AlignmentPlan } from './library'
import { writeSkinPackage } from './package'
import { parseSkin } from './parse'
import { createMemoryStorage, createSkinLibrary, type SkinLibrary } from './store'
import {
  createSkinTransferRouter,
  type CommitDone,
  type SkinListing,
  type SkinTransferRouter,
  type UploadAccepted
} from './transfer'
import { runAlignment, type SkinPeer } from './sync'

/** Una skin utente: `plain` con un altro id, come farebbe un fork. */
function userSkin(id = 'notturno', version = '1.0.0', name = 'Notturno'): Record<string, unknown> {
  const source = JSON.parse(JSON.stringify(PLAIN_SKIN_SOURCE)) as Record<string, unknown>
  source['id'] = id
  source['meta'] = { ...(source['meta'] as object), name, version }
  return source
}

/** Stessa versione, contenuto diverso: il caso che rende necessarie le impronte. */
function variant(id: string, version: string): Record<string, unknown> {
  const source = userSkin(id, version)
  source['meta'] = { ...(source['meta'] as object), description: `variante ${Math.random()}` }
  return source
}

function packageOf(source: unknown): Uint8Array {
  const written = writeSkinPackage({ source })
  if (!written.ok) throw written.error
  return written.value
}

function builtinEntries() {
  return BUILTIN_SKIN_SOURCES.map((source) => {
    const parsed = parseSkin(source)
    if (!parsed.ok) throw parsed.error
    return libraryEntryFor(parsed.value, source, true)
  })
}

interface Device {
  readonly library: SkinLibrary
  readonly router: SkinTransferRouter
}

function device(sources: readonly unknown[] = [], allowRemove = true): Device {
  const files: Record<string, Uint8Array> = {}
  for (const source of sources) {
    const parsed = parseSkin(source)
    if (!parsed.ok) throw parsed.error
    files[`${parsed.value.id}.aeskin`] = packageOf(source)
  }
  const library = createSkinLibrary({
    storage: createMemoryStorage(files),
    reservedIds: [...BUILTIN_SKIN_IDS]
  })
  return {
    library,
    router: createSkinTransferRouter({ library, allowRemove, builtins: builtinEntries() })
  }
}

/**
 * Il canale, sopra le rotte vere.
 *
 * Non un doppio inventato: la stessa `handle` che monterebbe un server HTTP, con
 * la serializzazione degli errori inclusa. Così un disaccordo fra le due metà
 * del trasporto emerge qui invece che sul telefono di qualcuno.
 */
function peerOver(remote: Device): SkinPeer & { calls: string[] } {
  const calls: string[] = []

  function call(
    method: string,
    path: string,
    payload?: { bytes?: Uint8Array; json?: unknown; headers?: Record<string, string> }
  ) {
    calls.push(`${method} ${path}`)
    const response = remote.router.handle({ method, path, ...payload })
    if (response === null) throw new Error(`rotta non riconosciuta: ${method} ${path}`)
    return response
  }

  function unwrap<T>(response: ReturnType<typeof call>): Result<T, AppError> {
    if (response.body.kind === 'bytes') {
      return response.status === 200
        ? ok(response.body.value as unknown as T)
        : err(AppError.of('internal.unexpected', { detail: 'byte con stato non 200' }))
    }
    const body = response.body.value as Record<string, unknown>
    if (response.status >= 400) {
      // Il payload di rete è un AppErrorPayload valido: si ricostruisce, non si
      // indovina. È la promessa del confine, provata qui.
      return err(AppError.from(body['error']))
    }
    return ok(body as unknown as T)
  }

  return {
    calls,
    list: () => Promise.resolve(unwrap<SkinListing>(call('GET', '/api/skins'))),
    download: (id) => Promise.resolve(unwrap<Uint8Array>(call('GET', `/api/skins/${id}`))),
    upload: (archive, fingerprint) =>
      Promise.resolve(
        unwrap<UploadAccepted>(
          call('POST', '/api/skins/upload', {
            bytes: archive,
            headers: { 'x-aether-skin-fingerprint': fingerprint }
          })
        )
      ),
    commit: (uploadId, overwrite) =>
      Promise.resolve(
        unwrap<CommitDone>(call('POST', '/api/skins/commit', { json: { uploadId, overwrite } }))
      ),
    remove: (id) => {
      const done = unwrap<unknown>(call('DELETE', `/api/skins/${id}`))
      return Promise.resolve(done.ok ? ok(true as const) : err(done.error))
    }
  }
}

/** Il piano fra due dispositivi, come lo costruirebbe chi guida. */
async function planBetween(here: Device, peer: SkinPeer): Promise<AlignmentPlan> {
  const listing = await peer.list()
  if (!listing.ok) throw listing.error
  return alignLibraries([...builtinEntries(), ...here.library.entries()], listing.value.entries)
}

function ids(plan: AlignmentPlan, action: string): string[] {
  return plan.items.filter((item) => item.action === action).map((item) => item.id)
}

describe('il giro completo', () => {
  it('allinea due librerie nei due sensi', async () => {
    const here = device([userSkin('alfa'), userSkin('comune', '2.0.0')])
    const there = device([userSkin('beta'), userSkin('comune', '1.0.0')])
    const peer = peerOver(there)

    const plan = await planBetween(here, peer)
    expect(ids(plan, 'send')).toEqual(['alfa'])
    expect(ids(plan, 'receive')).toEqual(['beta'])
    expect(ids(plan, 'sendNewer')).toEqual(['comune'])

    const report = await runAlignment(plan, { local: here.library, peer })

    expect(report.sent).toBe(2)
    expect(report.received).toBe(1)
    expect(report.failed).toBe(0)
    expect(report.diverged).toBe(0)

    // Dopo, un secondo piano non trova più niente da fare.
    const dopo = await planBetween(here, peerOver(there))
    expect(dopo.automatic).toEqual([])
    expect(here.library.entries().map((entry) => entry.id)).toEqual(['alfa', 'beta', 'comune'])
    expect(there.library.entries().map((entry) => entry.id)).toEqual(['alfa', 'beta', 'comune'])
  })

  it('le skin di serie restano fuori', async () => {
    const here = device()
    const there = device()
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)

    expect(ids(plan, 'skipBuiltin').sort()).toEqual(['cyberpunk', 'nothing', 'plain'])
    const report = await runAlignment(plan, { local: here.library, peer })
    expect(report.sent + report.received).toBe(0)
    // Nessuna chiamata di trasferimento: solo l'elenco iniziale.
    expect(peer.calls).toEqual(['GET /api/skins'])
  })

  it('non cancella mai niente', async () => {
    // Un piano non contiene cancellazioni, e non deve poterne inventare: una skin
    // che sta solo da una parte si copia, non si toglie.
    const here = device()
    const there = device([userSkin('solo-la')])
    const peer = peerOver(there)

    const plan = await planBetween(here, peer)
    await runAlignment(plan, { local: here.library, peer })

    expect(peer.calls.filter((call) => call.startsWith('DELETE'))).toEqual([])
    expect(there.library.entries().map((entry) => entry.id)).toEqual(['solo-la'])
  })

  it('lascia i conflitti alla persona', async () => {
    const here = device([variant('contesa', '1.0.0')])
    const there = device([variant('contesa', '1.0.0')])
    const peer = peerOver(there)

    const plan = await planBetween(here, peer)
    expect(ids(plan, 'conflict')).toEqual(['contesa'])

    const report = await runAlignment(plan, { local: here.library, peer })
    expect(report.results[0]?.outcome).toBe('skipped')
    expect(report.sent + report.received).toBe(0)
    expect(peer.calls).toEqual(['GET /api/skins'])
  })
})

describe('un piano è una fotografia', () => {
  it('non sovrascrive quel che è comparso dall\'altra parte dopo lo scatto', async () => {
    /*
     * Il piano dice «manda: là non c'è». Fra lo scatto e l'esecuzione qualcuno
     * installa una skin con quell'id sull'altro dispositivo. Mandarla e
     * sovrascriverla distruggerebbe un lavoro che l'utente non ha mai visto
     * nell'elenco che ha approvato.
     */
    const here = device([userSkin('notturno', '2.0.0')])
    const there = device()
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)
    expect(ids(plan, 'send')).toEqual(['notturno'])

    const intrusa = packageOf(userSkin('notturno', '9.9.9', 'Lavoro di qualcun altro'))
    expect(there.library.install(intrusa).ok).toBe(true)

    const report = await runAlignment(plan, { local: here.library, peer })
    expect(report.diverged).toBe(1)
    expect(report.sent).toBe(0)
    const remota = there.library.get('notturno')
    expect(remota.ok && remota.value.document.meta.version).toBe('9.9.9')
  })

  it('non sovrascrive una copia remota cambiata dopo lo scatto', async () => {
    const here = device([userSkin('notturno', '3.0.0')])
    const there = device([userSkin('notturno', '1.0.0')])
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)
    expect(ids(plan, 'sendNewer')).toEqual(['notturno'])

    // Là qualcuno modifica la skin: stessa versione, contenuto diverso — cioè il
    // caso che le sole versioni non distinguerebbero.
    expect(there.library.save(variant('notturno', '1.0.0')).ok).toBe(true)

    const report = await runAlignment(plan, { local: here.library, peer })
    expect(report.diverged).toBe(1)
    expect(report.results.find((r) => r.id === 'notturno')?.reason).toContain(
      'cambiata dopo il piano'
    )
    const remota = there.library.get('notturno')
    expect(remota.ok && remota.value.document.meta.version).toBe('1.0.0')
  })

  it('non sovrascrive la copia locale cambiata dopo lo scatto', async () => {
    const here = device([userSkin('notturno', '1.0.0')])
    const there = device([userSkin('notturno', '3.0.0')])
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)
    expect(ids(plan, 'receiveNewer')).toEqual(['notturno'])

    // Qui qualcuno salva dallo Studio mentre l'allineamento è in corso.
    expect(here.library.save(variant('notturno', '1.0.0')).ok).toBe(true)

    const report = await runAlignment(plan, { local: here.library, peer })
    expect(report.diverged).toBe(1)
    const locale = here.library.get('notturno')
    expect(locale.ok && locale.value.document.meta.version).toBe('1.0.0')
  })

  it('rifiuta un pacchetto diverso da quello annunciato', async () => {
    /*
     * L'utente ha approvato un contenuto preciso. Se quel che arriva non è
     * quello, installarlo sarebbe fare una cosa diversa da quella accettata —
     * e questa è anche la guardia contro un pacchetto sostituito per strada.
     */
    const here = device()
    const there = device([userSkin('notturno', '1.0.0')])
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)

    const sostituito = packageOf(userSkin('notturno', '1.0.0', 'Altro'))
    const report = await runAlignment(plan, {
      local: here.library,
      peer: { ...peer, download: () => Promise.resolve(ok(sostituito)) }
    })

    expect(report.diverged).toBe(1)
    expect(here.library.get('notturno').ok).toBe(false)
  })

  it('quel che è già identico non si riscrive', async () => {
    const source = userSkin('notturno')
    const here = device([source])
    const there = device()
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)

    // Fra il piano e l'esecuzione la stessa skin arriva là per un'altra strada.
    expect(there.library.install(packageOf(source)).ok).toBe(true)

    const report = await runAlignment(plan, { local: here.library, peer })
    expect(report.results.find((r) => r.id === 'notturno')?.outcome).toBe('alreadyInSync')
    expect(peer.calls.filter((call) => call.includes('commit'))).toEqual([])
  })
})

describe('riparazione', () => {
  it('un pacchetto locale illeggibile viene rimpiazzato da quello buono', async () => {
    // L'elenco omette gli illeggibili, quindi il piano li vede come mancanti:
    // riparare è un caso particolare dell'allineamento, senza codice dedicato.
    const there = device([userSkin('notturno', '1.0.0')])
    const peer = peerOver(there)
    const storage = createMemoryStorage({ 'notturno.aeskin': new Uint8Array([1, 2, 3]) })
    const rotta = createSkinLibrary({ storage, reservedIds: [...BUILTIN_SKIN_IDS] })

    const listing = await peer.list()
    if (!listing.ok) throw listing.error
    const plan = alignLibraries([...builtinEntries(), ...rotta.entries()], listing.value.entries)
    expect(ids(plan, 'receive')).toEqual(['notturno'])

    const report = await runAlignment(plan, { local: rotta, peer })
    expect(report.received).toBe(1)
    expect(rotta.get('notturno').ok).toBe(true)
  })
})

describe('guasti e annullamento', () => {
  it('una voce che fallisce non ferma le altre', async () => {
    const here = device([userSkin('alfa'), userSkin('beta'), userSkin('gamma')])
    const there = device()
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)

    const zoppo: SkinPeer = {
      ...peer,
      upload: (archive, fingerprint) =>
        fingerprint === here.library.entries().filter((entry) => entry.id === 'beta')[0]?.fingerprint
          ? Promise.resolve(err(AppError.of('net.timeout', { timeoutMs: 5000 })))
          : peer.upload(archive, fingerprint)
    }

    const report = await runAlignment(plan, { local: here.library, peer: zoppo })
    expect(report.sent).toBe(2)
    expect(report.failed).toBe(1)
    const fallita = report.results.find((result) => result.outcome === 'failed')
    expect(fallita?.id).toBe('beta')
    expect(fallita?.error?.code).toBe('net.timeout')
    expect(there.library.entries().map((entry) => entry.id)).toEqual(['alfa', 'gamma'])
  })

  it('un canale che lancia diventa un guasto di quella voce', async () => {
    const here = device([userSkin('alfa')])
    const there = device()
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)

    const report = await runAlignment(plan, {
      local: here.library,
      peer: {
        ...peer,
        upload: () => {
          throw new Error('socket chiuso')
        }
      }
    })
    expect(report.failed).toBe(1)
    expect(report.results.find((r) => r.outcome === 'failed')?.error?.code).toBe(
      'internal.unexpected'
    )
  })

  it('l\'errore dell\'altro dispositivo arriva con il suo codice', async () => {
    /*
     * Il payload di rete è un AppErrorPayload valido, quindi il codice
     * attraversa la rete intatto: niente regex sul messaggio, che era il modo in
     * cui il legacy ricostruiva l'identità di un errore dopo un salto.
     *
     * Il caso: un lato ha `plain` installata come skin utente — una build
     * vecchia, un id diventato di serie dopo — e prova a mandarla.
     */
    const peer = peerOver(device())
    const storage = createMemoryStorage({ 'plain.aeskin': packageOf(PLAIN_SKIN_SOURCE) })
    const locale = createSkinLibrary({ storage })

    const plan = alignLibraries(locale.entries(), [])
    expect(ids(plan, 'send')).toEqual(['plain'])

    const report = await runAlignment(plan, { local: locale, peer })
    expect(report.failed).toBe(1)
    expect(report.results[0]?.error?.code).toBe('skin.builtinReadOnly')
    expect(report.results[0]?.error?.params['id']).toBe('plain')
  })

  it('annulla senza toccare le voci rimaste', async () => {
    const here = device([userSkin('alfa'), userSkin('beta'), userSkin('gamma')])
    const there = device()
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)

    const controller = new AbortController()
    const report = await runAlignment(plan, {
      local: here.library,
      peer,
      signal: controller.signal,
      onProgress: (_result, done) => {
        if (done === 1) controller.abort()
      }
    })

    expect(report.sent).toBe(1)
    expect(report.aborted).toBe(true)
    // Le cinque voci rimaste — beta e gamma incluse — non vengono toccate.
    expect(report.results.filter((result) => result.outcome === 'aborted')).toHaveLength(5)
    expect(there.library.entries().map((entry) => entry.id)).toEqual(['alfa'])
  })

  it('riporta l\'avanzamento una voce alla volta', async () => {
    const here = device([userSkin('alfa'), userSkin('beta')])
    const there = device()
    const peer = peerOver(there)
    const plan = await planBetween(here, peer)

    const visto: string[] = []
    await runAlignment(plan, {
      local: here.library,
      peer,
      onProgress: (result, done, total) => visto.push(`${done}/${total} ${result.id}`)
    })

    expect(visto).toEqual([
      '1/5 alfa',
      '2/5 beta',
      '3/5 cyberpunk',
      '4/5 nothing',
      '5/5 plain'
    ])
  })
})
