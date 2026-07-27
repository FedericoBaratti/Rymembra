import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { afterEach, describe, expect, it } from 'vitest'
import { BUILTIN_SKIN_IDS, BUILTIN_SKIN_SOURCES, PLAIN_SKIN_SOURCE } from './builtin'
import { alignLibraries, libraryEntryFor } from './library'
import { writeSkinPackage } from './package'
import { parseSkin } from './parse'
import { runAlignment } from './sync'
import { createSkinTransferRouter } from './transfer'
import { createMemoryStorage, createSkinLibrary, type SkinLibrary } from './store'
import { JSON_BODY_LIMIT, createHttpSkinPeer, serveSkinRoutes, type FetchLike } from './http'

const started: Server[] = []

afterEach(async () => {
  await Promise.all(
    started.splice(0).map(
      (server) =>
        new Promise<void>((resolve) => {
          server.closeAllConnections()
          server.close(() => resolve())
        })
    )
  )
})

function userSkin(id = 'notturno', version = '1.0.0', name = 'Notturno'): Record<string, unknown> {
  const source = JSON.parse(JSON.stringify(PLAIN_SKIN_SOURCE)) as Record<string, unknown>
  source['id'] = id
  source['meta'] = { ...(source['meta'] as object), name, version }
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

function libraryOf(sources: readonly unknown[]): SkinLibrary {
  const files: Record<string, Uint8Array> = {}
  for (const source of sources) {
    const parsed = parseSkin(source)
    if (!parsed.ok) throw parsed.error
    files[`${parsed.value.id}.aeskin`] = packageOf(source)
  }
  return createSkinLibrary({
    storage: createMemoryStorage(files),
    reservedIds: [...BUILTIN_SKIN_IDS]
  })
}

/**
 * Un server vero, con una rotta estranea accanto.
 *
 * L'estranea non è decorazione: serve a provare che l'innesto lascia passare ciò
 * che non è suo, che è la condizione per montarlo su un server che ha già le
 * proprie rotte.
 */
async function serve(
  library: SkinLibrary,
  allowRemove = true
): Promise<{ baseUrl: string; library: SkinLibrary }> {
  const router = createSkinTransferRouter({ library, allowRemove, builtins: builtinEntries() })
  const server = createServer((req, res) => {
    void serveSkinRoutes(router, req, res)
      .then((handled) => {
        if (handled) return
        res.writeHead(200, { 'Content-Type': 'text/plain' })
        res.end('rotta di casa')
      })
      .catch(() => {
        if (!res.writableEnded) {
          res.writeHead(500)
          res.end()
        }
      })
  })
  started.push(server)
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address() as AddressInfo
  return { baseUrl: `http://127.0.0.1:${address.port}`, library }
}

const realFetch = fetch as unknown as FetchLike

describe('innesto su un server vero', () => {
  it('lascia passare le rotte di casa', async () => {
    const { baseUrl } = await serve(libraryOf([]))
    const response = await fetch(`${baseUrl}/api/tracks`)

    expect(response.status).toBe(200)
    expect(await response.text()).toBe('rotta di casa')
  })

  it('allinea due dispositivi su una porta vera', async () => {
    const here = libraryOf([userSkin('alfa'), userSkin('comune', '2.0.0')])
    const remote = await serve(libraryOf([userSkin('beta'), userSkin('comune', '1.0.0')]))
    const peer = createHttpSkinPeer({ baseUrl: remote.baseUrl, fetch: realFetch })

    const listing = await peer.list()
    expect(listing.ok).toBe(true)
    if (!listing.ok) return

    const plan = alignLibraries([...builtinEntries(), ...here.entries()], listing.value.entries)
    const report = await runAlignment(plan, { local: here, peer })

    expect(report.failed).toBe(0)
    expect(report.diverged).toBe(0)
    expect(report.sent).toBe(2)
    expect(report.received).toBe(1)
    expect(here.entries().map((entry) => entry.id)).toEqual(['alfa', 'beta', 'comune'])
    expect(remote.library.entries().map((entry) => entry.id)).toEqual(['alfa', 'beta', 'comune'])

    const dopo = remote.library.get('comune')
    expect(dopo.ok && dopo.value.document.meta.version).toBe('2.0.0')
  })

  it('un pacchetto scaricato arriva byte per byte identico', async () => {
    const archive = packageOf(userSkin())
    const remote = await serve(libraryOf([userSkin()]))
    const peer = createHttpSkinPeer({ baseUrl: remote.baseUrl, fetch: realFetch })

    const downloaded = await peer.download('notturno')
    expect(downloaded.ok).toBe(true)
    if (downloaded.ok) expect(downloaded.value).toEqual(archive)
  })
})

describe('errori attraverso la rete', () => {
  it('arrivano con il codice, non con una stringa', async () => {
    const remote = await serve(libraryOf([]))
    const peer = createHttpSkinPeer({ baseUrl: remote.baseUrl, fetch: realFetch })

    const missing = await peer.download('notturno')
    expect(missing.ok).toBe(false)
    if (missing.ok) return
    expect(missing.error.code).toBe('skin.notFound')
    expect(missing.error.params['id']).toBe('notturno')
  })

  it('il desktop rifiuta la cancellazione anche via HTTP', async () => {
    const remote = await serve(libraryOf([userSkin()]), false)
    const peer = createHttpSkinPeer({ baseUrl: remote.baseUrl, fetch: realFetch })

    const removed = await peer.remove('notturno')
    expect(removed.ok).toBe(false)
    if (!removed.ok) expect(removed.error.code).toBe('transfer.methodNotSupported')
    expect(remote.library.entries()).toHaveLength(1)
  })

  it('un messaggio con accenti non viene troncato', async () => {
    /*
     * Content-Length va calcolato sui BYTE. Con `payload.length` ogni accento
     * dichiarerebbe un byte in meno del vero: il corpo arriverebbe tagliato e
     * `response.json()` fallirebbe. Il messaggio di questo errore contiene «più»,
     * quindi il test cade se il conteggio torna a essere sui caratteri.
     */
    const remote = await serve(libraryOf([]))
    const peer = createHttpSkinPeer({ baseUrl: remote.baseUrl, fetch: realFetch })

    const orfano = await peer.commit('upl-1-aaaaaa', false)
    expect(orfano.ok).toBe(false)
    if (orfano.ok) return
    expect(orfano.error.code).toBe('transfer.aborted')
    expect(String(orfano.error.params['reason'])).toContain('più')
  })

  it('un corpo oltre il tetto viene rifiutato', async () => {
    const remote = await serve(libraryOf([]))
    const response = await fetch(`${remote.baseUrl}/api/skins/commit`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ uploadId: 'x'.repeat(JSON_BODY_LIMIT + 1) })
    })

    expect(response.status).toBe(413)
    const body = (await response.json()) as { error: { code: string } }
    expect(body.error.code).toBe('skin.tooLarge')
  })

  it('un corpo che non è JSON viene rifiutato', async () => {
    const remote = await serve(libraryOf([]))
    const response = await fetch(`${remote.baseUrl}/api/skins/commit`, {
      method: 'POST',
      body: 'non sono JSON'
    })

    expect(response.status).toBe(400)
    const body = (await response.json()) as { error: { code: string } }
    expect(body.error.code).toBe('ipc.payloadInvalid')
  })

  it('un server irraggiungibile diventa un errore di rete, non un\'eccezione', async () => {
    /*
     * `fetch` non lascia trasparire l'errno: quel che lancia è
     * `TypeError: fetch failed`, con ECONNREFUSED un anello più sotto. È il caso
     * che ha portato AppError.from a cercare l'errno lungo la catena invece che
     * solo in cima — senza, «l'altro dispositivo non risponde» arrivava come
     * internal.unexpected, non ritentabile.
     */
    const scratch = createServer()
    await new Promise<void>((resolve) => scratch.listen(0, '127.0.0.1', resolve))
    const port = (scratch.address() as AddressInfo).port
    await new Promise<void>((resolve) => scratch.close(() => resolve()))

    const peer = createHttpSkinPeer({ baseUrl: `http://127.0.0.1:${port}`, fetch: realFetch })
    const listing = await peer.list()

    expect(listing.ok).toBe(false)
    if (listing.ok) return
    expect(listing.error.code).toBe('net.offline')
    expect(listing.error.retryable).toBe(true)
  })
})
