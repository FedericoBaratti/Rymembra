import { describe, expect, it } from 'vitest'
import { BUILTIN_SKIN_IDS, BUILTIN_SKIN_SOURCES, PLAIN_SKIN_SOURCE } from './builtin'
import { libraryEntryFor, skinFingerprint } from './library'
import { writeSkinPackage } from './package'
import { parseSkin } from './parse'
import { createMemoryStorage, createSkinLibrary, type SkinStorage } from './store'
import {
  FINGERPRINT_HEADER,
  SKIN_PACKAGE_MIME,
  SKIN_TRANSFER_PROTOCOL,
  createSkinTransferRouter,
  type SkinListing,
  type SkinTransferError,
  type SkinTransferRequest,
  type SkinTransferResponse,
  type UploadAccepted
} from './transfer'

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3])

/** Una skin utente: `plain` con un altro id, come farebbe un fork. */
function userSkin(id = 'notturno', version = '1.0.0', name = 'Notturno'): Record<string, unknown> {
  const source = JSON.parse(JSON.stringify(PLAIN_SKIN_SOURCE)) as Record<string, unknown>
  source['id'] = id
  source['meta'] = { ...(source['meta'] as object), name, version }
  return source
}

function packageOf(source: unknown, preview?: Uint8Array): Uint8Array {
  const written = writeSkinPackage(preview === undefined ? { source } : { source, preview })
  if (!written.ok) throw written.error
  return written.value
}

/** Le voci di serie, come le passa chi monta le rotte. */
function builtinEntries() {
  return BUILTIN_SKIN_SOURCES.map((source) => {
    const parsed = parseSkin(source)
    if (!parsed.ok) throw parsed.error
    return libraryEntryFor(parsed.value, source, true)
  })
}

interface Harness {
  readonly storage: SkinStorage
  readonly router: ReturnType<typeof createSkinTransferRouter>
  readonly tick: (ms: number) => void
}

function harness(
  options: {
    files?: Record<string, Uint8Array>
    allowRemove?: boolean
    limits?: { maxUploads?: number; maxBytes?: number; ttlMs?: number }
  } = {}
): Harness {
  const storage = createMemoryStorage(options.files ?? {})
  const library = createSkinLibrary({ storage, reservedIds: [...BUILTIN_SKIN_IDS] })
  let clock = 1_000_000
  const router = createSkinTransferRouter({
    library,
    allowRemove: options.allowRemove ?? true,
    builtins: builtinEntries(),
    now: () => clock,
    ...(options.limits === undefined ? {} : { limits: options.limits })
  })
  return {
    storage,
    router,
    tick: (ms) => {
      clock += ms
    }
  }
}

function jsonBody(response: SkinTransferResponse): Record<string, unknown> {
  expect(response.body.kind).toBe('json')
  if (response.body.kind !== 'json') throw new Error('non è JSON')
  return response.body.value as Record<string, unknown>
}

function errorOf(response: SkinTransferResponse): SkinTransferError {
  return jsonBody(response)['error'] as SkinTransferError
}

function must(response: SkinTransferResponse | null): SkinTransferResponse {
  if (response === null) throw new Error('la rotta non è stata riconosciuta')
  return response
}

function send(
  router: Harness['router'],
  request: SkinTransferRequest
): SkinTransferResponse {
  return must(router.handle(request))
}

/** Il giro completo: manda un archivio e lo committa. */
function push(
  router: Harness['router'],
  archive: Uint8Array,
  overwrite = false
): { upload: SkinTransferResponse; commit: SkinTransferResponse } {
  const upload = send(router, { method: 'POST', path: '/api/skins/upload', bytes: archive })
  const accepted = jsonBody(upload) as unknown as UploadAccepted
  const commit = send(router, {
    method: 'POST',
    path: '/api/skins/commit',
    json: { uploadId: accepted.uploadId, overwrite }
  })
  return { upload, commit }
}

describe('montaggio', () => {
  it('non risponde su ciò che non è suo', () => {
    // Il server che ospita deve poter continuare con le proprie rotte: un router
    // che risponde 404 su tutto non si può montare accanto a nient'altro.
    const { router } = harness()
    expect(router.handle({ method: 'GET', path: '/api/tracks' })).toBeNull()
    expect(router.handle({ method: 'GET', path: '/health' })).toBeNull()
    expect(router.handle({ method: 'GET', path: '/' })).toBeNull()
  })

  it('risponde 405 su un metodo che non ha su un percorso suo', () => {
    const { router } = harness()
    const response = send(router, { method: 'PUT', path: '/api/skins' })
    expect(response.status).toBe(405)
    expect(errorOf(response).code).toBe('transfer.methodNotSupported')
  })
})

describe('elenco', () => {
  it('elenca le installate insieme a quelle di serie, in ordine', () => {
    const { router } = harness({ files: { 'notturno.aeskin': packageOf(userSkin()) } })
    const listing = jsonBody(send(router, { method: 'GET', path: '/api/skins' })) as unknown as SkinListing

    expect(listing.protocol).toBe(SKIN_TRANSFER_PROTOCOL)
    expect(listing.entries.map((entry) => entry.id)).toEqual([
      'cyberpunk',
      'nothing',
      'notturno',
      'plain'
    ])
    expect(listing.entries.filter((entry) => entry.builtin)).toHaveLength(3)
  })

  it('include le skin di serie perché l\'altro lato non provi a mandarle', () => {
    // Se un lato ha una skin di serie che l'altro non ha ancora (build diverse),
    // l'allineamento deve marcarla skipBuiltin, non «manca: mandala» — quell'invio
    // finirebbe rifiutato con un errore che sembra un guasto.
    const { router } = harness()
    const listing = jsonBody(send(router, { method: 'GET', path: '/api/skins' })) as unknown as SkinListing
    const plain = listing.entries.find((entry) => entry.id === 'plain')
    expect(plain?.builtin).toBe(true)
  })

  it('conta i pacchetti illeggibili invece di fallire', () => {
    const { router } = harness({
      files: {
        'notturno.aeskin': packageOf(userSkin()),
        'rotta.aeskin': new Uint8Array([1, 2, 3, 4])
      }
    })
    const listing = jsonBody(send(router, { method: 'GET', path: '/api/skins' })) as unknown as SkinListing

    expect(listing.unreadable).toBe(1)
    expect(listing.entries.some((entry) => entry.id === 'notturno')).toBe(true)
  })
})

describe('upload e commit', () => {
  it('installa una skin nuova', () => {
    const { router, storage } = harness()
    const { upload, commit } = push(router, packageOf(userSkin()))

    const accepted = jsonBody(upload) as unknown as UploadAccepted
    expect(accepted.id).toBe('notturno')
    expect(accepted.installed).toBeNull()
    expect(accepted.identical).toBe(false)

    expect(commit.status).toBe(200)
    expect(jsonBody(commit)['id']).toBe('notturno')
    expect(storage.list()).toEqual(['notturno.aeskin'])
    expect(router.pendingUploads()).toBe(0)
  })

  it('conserva miniature e asset: quel che si committa è quel che è arrivato', () => {
    const { router, storage } = harness()
    const archive = packageOf(userSkin(), PNG)
    push(router, archive)

    expect(storage.read('notturno.aeskin')).toEqual(archive)
  })

  it('dice cosa c\'è già, e il commit chiede la conferma una volta sola', () => {
    /*
     * È il test che giustifica la divisione in due chiamate. Chi manda scopre il
     * conflitto DOPO aver trasferito i byte; se il rifiuto li buttasse, la
     * conferma dell'utente costerebbe un secondo trasferimento dell'intero
     * archivio — su Wi-Fi, per una skin che può arrivare a venti mega.
     */
    const { router } = harness({ files: { 'notturno.aeskin': packageOf(userSkin('notturno', '1.0.0')) } })
    const nuova = packageOf(userSkin('notturno', '2.0.0'))

    const upload = send(router, { method: 'POST', path: '/api/skins/upload', bytes: nuova })
    const accepted = jsonBody(upload) as unknown as UploadAccepted
    expect(accepted.installed?.version).toBe('1.0.0')
    expect(accepted.installed?.unreadable).toBe(false)
    expect(accepted.identical).toBe(false)

    const rifiutato = send(router, {
      method: 'POST',
      path: '/api/skins/commit',
      json: { uploadId: accepted.uploadId }
    })
    expect(rifiutato.status).toBe(409)
    expect(errorOf(rifiutato).code).toBe('skin.idConflict')

    // I byte sono ancora lì: la conferma non ricarica niente.
    expect(router.pendingUploads()).toBe(1)
    const confermato = send(router, {
      method: 'POST',
      path: '/api/skins/commit',
      json: { uploadId: accepted.uploadId, overwrite: true }
    })
    expect(confermato.status).toBe(200)
    expect(jsonBody(confermato)['version']).toBe('2.0.0')
    expect(router.pendingUploads()).toBe(0)
  })

  it('segnala quando il contenuto è già identico', () => {
    // Succede quando la stessa skin è stata installata sui due lati fra la
    // costruzione del piano e la sua esecuzione: il commit è lavoro inutile.
    const source = userSkin()
    const { router } = harness({ files: { 'notturno.aeskin': packageOf(source) } })

    const upload = send(router, { method: 'POST', path: '/api/skins/upload', bytes: packageOf(source) })
    const accepted = jsonBody(upload) as unknown as UploadAccepted
    expect(accepted.identical).toBe(true)
    expect(accepted.fingerprint).toBe(skinFingerprint(source))
  })

  it('segnala un pacchetto installato ma illeggibile', () => {
    // Senza, chi manda non chiederebbe la sovrascrittura e il commit fallirebbe
    // con un conflitto su una skin che l'utente non vede nemmeno nell'elenco.
    const { router } = harness({ files: { 'notturno.aeskin': new Uint8Array([9, 9, 9]) } })

    const upload = send(router, { method: 'POST', path: '/api/skins/upload', bytes: packageOf(userSkin()) })
    const accepted = jsonBody(upload) as unknown as UploadAccepted
    expect(accepted.installed).toEqual({ version: null, fingerprint: null, unreadable: true })
  })

  it('rifiuta un archivio corrotto senza occupare lo staging', () => {
    const { router } = harness()
    const response = send(router, {
      method: 'POST',
      path: '/api/skins/upload',
      bytes: new Uint8Array([1, 2, 3, 4, 5])
    })

    expect(response.status).toBe(400)
    expect(errorOf(response).code).toBe('skin.packageCorrupt')
    expect(router.pendingUploads()).toBe(0)
  })

  it('rifiuta un corpo vuoto', () => {
    const { router } = harness()
    const response = send(router, { method: 'POST', path: '/api/skins/upload' })
    expect(response.status).toBe(400)
    expect(errorOf(response).code).toBe('skin.packageCorrupt')
  })

  it('rifiuta una skin di serie prima di toccare lo staging', () => {
    const { router } = harness()
    const response = send(router, {
      method: 'POST',
      path: '/api/skins/upload',
      bytes: packageOf(PLAIN_SKIN_SOURCE)
    })

    expect(response.status).toBe(403)
    expect(errorOf(response).code).toBe('skin.builtinReadOnly')
    expect(router.pendingUploads()).toBe(0)
  })

  it('verifica l\'impronta dichiarata da chi manda', () => {
    // Il piano di allineamento decide sulle impronte: chi riceve deve poter
    // controllare che i byte arrivati siano quelli su cui il piano ha deciso.
    const { router } = harness()
    const response = send(router, {
      method: 'POST',
      path: '/api/skins/upload',
      headers: { [FINGERPRINT_HEADER]: 'deadbeef' },
      bytes: packageOf(userSkin())
    })

    expect(response.status).toBe(422)
    const error = errorOf(response)
    expect(error.code).toBe('transfer.integrityMismatch')
    expect(error.params['expected']).toBe('deadbeef')
    expect(error.params['actual']).toBe(skinFingerprint(userSkin()))
  })

  it('accetta l\'impronta quando corrisponde', () => {
    const { router } = harness()
    const source = userSkin()
    const response = send(router, {
      method: 'POST',
      path: '/api/skins/upload',
      headers: { [FINGERPRINT_HEADER]: skinFingerprint(source) },
      bytes: packageOf(source)
    })
    expect(response.status).toBe(200)
  })

  it('rifiuta un commit senza uploadId', () => {
    const { router } = harness()
    const response = send(router, { method: 'POST', path: '/api/skins/commit', json: {} })
    expect(response.status).toBe(400)
    expect(errorOf(response).code).toBe('ipc.payloadInvalid')
  })

  it('rifiuta un commit su un\'attesa che non esiste', () => {
    const { router } = harness()
    const response = send(router, {
      method: 'POST',
      path: '/api/skins/commit',
      json: { uploadId: 'upl-1-aaaaaa' }
    })
    expect(response.status).toBe(410)
    expect(errorOf(response).code).toBe('transfer.aborted')
    expect(errorOf(response).retryable).toBe(true)
  })
})

describe('staging', () => {
  it('butta un\'attesa scaduta', () => {
    // Il telefono esce dal Wi-Fi, l'utente chiude la finestra della domanda: i
    // byte non possono restare in memoria per sempre.
    const { router, tick } = harness({ limits: { ttlMs: 60_000 } })
    const upload = send(router, {
      method: 'POST',
      path: '/api/skins/upload',
      bytes: packageOf(userSkin())
    })
    const accepted = jsonBody(upload) as unknown as UploadAccepted

    tick(60_001)
    const response = send(router, {
      method: 'POST',
      path: '/api/skins/commit',
      json: { uploadId: accepted.uploadId }
    })
    expect(response.status).toBe(410)
    expect(router.pendingUploads()).toBe(0)
  })

  it('sfratta la più vecchia oltre il tetto delle attese', () => {
    const { router } = harness({ limits: { maxUploads: 2 } })
    const primo = jsonBody(
      send(router, { method: 'POST', path: '/api/skins/upload', bytes: packageOf(userSkin('alfa')) })
    ) as unknown as UploadAccepted
    send(router, { method: 'POST', path: '/api/skins/upload', bytes: packageOf(userSkin('beta')) })
    send(router, { method: 'POST', path: '/api/skins/upload', bytes: packageOf(userSkin('gamma')) })

    expect(router.pendingUploads()).toBe(2)
    const response = send(router, {
      method: 'POST',
      path: '/api/skins/commit',
      json: { uploadId: primo.uploadId }
    })
    expect(response.status).toBe(410)
  })

  it('sfratta anche sul tetto dei byte', () => {
    const { router } = harness({ limits: { maxBytes: 1 } })
    send(router, { method: 'POST', path: '/api/skins/upload', bytes: packageOf(userSkin('alfa')) })
    send(router, { method: 'POST', path: '/api/skins/upload', bytes: packageOf(userSkin('beta')) })
    expect(router.pendingUploads()).toBe(1)
  })
})

describe('scaricamento', () => {
  it('restituisce esattamente i byte installati', () => {
    const archive = packageOf(userSkin(), PNG)
    const { router } = harness({ files: { 'notturno.aeskin': archive } })
    const response = send(router, { method: 'GET', path: '/api/skins/notturno' })

    expect(response.status).toBe(200)
    expect(response.body.kind).toBe('bytes')
    if (response.body.kind !== 'bytes') return
    expect(response.body.contentType).toBe(SKIN_PACKAGE_MIME)
    expect(response.body.value).toEqual(archive)
  })

  it('404 su una skin che non c\'è', () => {
    const { router } = harness()
    const response = send(router, { method: 'GET', path: '/api/skins/notturno' })
    expect(response.status).toBe(404)
    expect(errorOf(response).code).toBe('skin.notFound')
  })

  it('404 su una skin di serie: non si trasferiscono', () => {
    const { router } = harness()
    expect(send(router, { method: 'GET', path: '/api/skins/plain' }).status).toBe(404)
  })

  it('non lascia che un id dalla rete diventi un percorso', () => {
    /*
     * L'id arriva dall'URL, cioè da fuori. La libreria compone il nome del file
     * dall'id e si fida perché lì l'id viene da un documento già validato: qui
     * no. Un id che il formato non potrebbe produrre non può nominare una skin
     * installata, quindi la risposta giusta è 404 — e lo storage non viene
     * toccato affatto.
     */
    const { storage } = harness({ files: { 'notturno.aeskin': packageOf(userSkin()) } })
    const letti: string[] = []
    const spia: SkinStorage = {
      ...storage,
      read: (name) => {
        letti.push(name)
        return storage.read(name)
      },
      exists: (name) => {
        letti.push(name)
        return storage.exists(name)
      }
    }
    const spiato = createSkinTransferRouter({
      library: createSkinLibrary({ storage: spia, reservedIds: [...BUILTIN_SKIN_IDS] }),
      allowRemove: true
    })

    for (const cattivo of ['..', '../notturno', 'NOTTURNO', 'x', 'notturno.aeskin', 'nott urno']) {
      const response = must(spiato.handle({ method: 'GET', path: `/api/skins/${cattivo}` }))
      expect(response.status).toBe(404)
    }
    expect(letti).toEqual([])
  })
})

describe('cancellazione', () => {
  it('il telefono la accetta', () => {
    const { router, storage } = harness({ files: { 'notturno.aeskin': packageOf(userSkin()) } })
    const response = send(router, { method: 'DELETE', path: '/api/skins/notturno' })

    expect(response.status).toBe(200)
    expect(storage.list()).toEqual([])
  })

  it('il desktop la rifiuta', () => {
    // È il posto dove le skin si creano: una cancellazione arrivata dalla rete
    // distruggerebbe un lavoro senza modo di tornare.
    const { router, storage } = harness({
      files: { 'notturno.aeskin': packageOf(userSkin()) },
      allowRemove: false
    })
    const response = send(router, { method: 'DELETE', path: '/api/skins/notturno' })

    expect(response.status).toBe(405)
    expect(errorOf(response).code).toBe('transfer.methodNotSupported')
    expect(storage.list()).toEqual(['notturno.aeskin'])
  })

  it('non cancella una skin di serie', () => {
    const { router } = harness()
    const response = send(router, { method: 'DELETE', path: '/api/skins/plain' })
    expect(response.status).toBe(403)
    expect(errorOf(response).code).toBe('skin.builtinReadOnly')
  })
})

describe('errori sul filo', () => {
  it('portano codice, parametri e traceId, ma non lo stack né il contesto', () => {
    // stack e context contengono percorsi locali e nomi di file di questo
    // dispositivo: l'altro capo non ha nulla da farci.
    const { router } = harness()
    const error = errorOf(send(router, { method: 'GET', path: '/api/skins/notturno' }))

    expect(error.code).toBe('skin.notFound')
    expect(error.params['id']).toBe('notturno')
    expect(error.i18nKey).toBe('errors.skin.notFound')
    expect(error.traceId).toMatch(/^[a-z0-9]+-[0-9a-f]+-[0-9a-f]{6}$/)
    expect(Object.keys(error)).not.toContain('stack')
    expect(Object.keys(error)).not.toContain('context')
    expect(JSON.stringify(error)).not.toContain('aeskin')
  })

  it('un id di skin legale non viene mangiato dalle azioni', () => {
    /*
     * `upload` e `commit` sono id di skin validi. Le azioni si riconoscono dal
     * metodo, quindi una skin chiamata così resta raggiungibile.
     */
    const { router } = harness({ files: { 'upload.aeskin': packageOf(userSkin('upload')) } })
    const response = send(router, { method: 'GET', path: '/api/skins/upload' })
    expect(response.status).toBe(200)
    expect(response.body.kind).toBe('bytes')
  })
})

describe('i due lati', () => {
  it('un giro completo: dal desktop al telefono', () => {
    const desktop = harness({
      files: { 'notturno.aeskin': packageOf(userSkin('notturno', '2.0.0')) },
      allowRemove: false
    })
    const telefono = harness({ files: { 'notturno.aeskin': packageOf(userSkin('notturno', '1.0.0')) } })

    // Il desktop legge la libreria del telefono, scarica la propria, la manda.
    const listing = jsonBody(
      send(telefono.router, { method: 'GET', path: '/api/skins' })
    ) as unknown as SkinListing
    const remota = listing.entries.find((entry) => entry.id === 'notturno')
    expect(remota?.version).toBe('1.0.0')

    const scaricata = send(desktop.router, { method: 'GET', path: '/api/skins/notturno' })
    if (scaricata.body.kind !== 'bytes') throw new Error('attesi byte')

    const { upload, commit } = push(telefono.router, scaricata.body.value, true)
    expect((jsonBody(upload) as unknown as UploadAccepted).installed?.version).toBe('1.0.0')
    expect(commit.status).toBe(200)

    const dopo = jsonBody(
      send(telefono.router, { method: 'GET', path: '/api/skins' })
    ) as unknown as SkinListing
    expect(dopo.entries.find((entry) => entry.id === 'notturno')?.version).toBe('2.0.0')
  })
})
