/**
 * Le due estremità del trasporto skin sopra HTTP.
 *
 * `transfer.ts` definisce le rotte senza sapere cos'è una richiesta HTTP, e
 * `sync.ts` esegue un piano senza sapere cos'è una connessione. Qui i due
 * incontrano la rete: da una parte l'innesto su un server `node:http`, dall'altra
 * un `SkinPeer` sopra `fetch`.
 *
 * Stanno nello stesso file di proposito. I nomi delle rotte, l'intestazione
 * dell'impronta e il tipo MIME devono coincidere fra chi serve e chi chiama, e
 * separarli in due moduli è il modo classico per farli divergere di una lettera
 * e scoprirlo su un dispositivo. Il test li fa parlare fra loro su una porta
 * vera, quindi una divergenza non compila o non passa.
 *
 * Questo modulo NON è esportato da `index.ts`: il renderer importa
 * `@aether/skin` per compilare una skin, e non ha né un server né un motivo per
 * conoscerne uno. Si importa esplicitamente come `@aether/skin/http`.
 *
 * L'import di `node:http` è solo di tipo, quindi il file non ha dipendenze a
 * runtime: la parte client gira anche nel renderer e sul backend mobile, dove
 * `fetch` arriva dal polyfill.
 */

import type { IncomingMessage, ServerResponse } from 'node:http'
import { AppError, err, ok, type Result } from '@aether/core'
import { PACKAGE_LIMITS } from './package'
import type { SkinPeer } from './sync'
import {
  FINGERPRINT_HEADER,
  SKIN_PACKAGE_MIME,
  failureResponse,
  type CommitDone,
  type SkinListing,
  type SkinTransferResponse,
  type SkinTransferRouter,
  type UploadAccepted
} from './transfer'

/**
 * Il tetto per i corpi JSON.
 *
 * Un commit è tre campi: qualunque cosa più grande non è un commit. Il tetto
 * dell'upload è invece quello del pacchetto, perché è lo stesso oggetto.
 */
export const JSON_BODY_LIMIT = 64 * 1024

const encoder = new TextEncoder()

// ── il lato che serve ───────────────────────────────────────────────────────

/**
 * Legge il corpo, con un tetto che vale prima e durante.
 *
 * Prima, su `content-length`: se chi chiama dichiara più del consentito si
 * risponde senza ricevere un byte. Durante, sul conteggio reale: `content-length`
 * è un'affermazione di chi manda, e su un corpo in chunked non c'è affatto.
 */
function readBody(req: IncomingMessage, limit: number): Promise<Result<Uint8Array, AppError>> {
  const declared = Number(req.headers['content-length'] ?? 0)
  if (Number.isFinite(declared) && declared > limit) {
    return Promise.resolve(err(AppError.of('skin.tooLarge', { bytes: declared, limitBytes: limit })))
  }

  return new Promise((resolve) => {
    const chunks: Uint8Array[] = []
    let received = 0
    let settled = false

    const finish = (result: Result<Uint8Array, AppError>): void => {
      if (settled) return
      settled = true
      resolve(result)
    }

    req.on('data', (chunk: Uint8Array) => {
      received += chunk.byteLength
      if (received > limit) {
        // Si taglia la connessione invece di continuare a ricevere: il tetto non
        // serve a niente se i byte oltre il tetto arrivano lo stesso.
        req.destroy()
        finish(err(AppError.of('skin.tooLarge', { bytes: received, limitBytes: limit })))
        return
      }
      chunks.push(chunk)
    })

    req.on('end', () => {
      const body = new Uint8Array(received)
      let offset = 0
      for (const chunk of chunks) {
        body.set(chunk, offset)
        offset += chunk.byteLength
      }
      finish(ok(body))
    })

    req.on('error', (cause) => {
      // Un errore mentre si legge il corpo vuol dire che la richiesta non è mai
      // arrivata intera: è un trasferimento interrotto, non un guasto di questo
      // dispositivo, e va detto con il codice giusto.
      finish(err(AppError.of('transfer.aborted', { reason: 'corpo della richiesta interrotto' }, { cause })))
    })
  })
}

/**
 * Scrive la risposta.
 *
 * `Content-Length` si calcola sui BYTE, non sui caratteri. Non è pedanteria: i
 * messaggi d'errore sono in italiano, e con `payload.length` ogni accento
 * dichiarerebbe un byte in meno del vero, troncando la risposta di quel tanto.
 */
export function sendSkinResponse(res: ServerResponse, response: SkinTransferResponse): void {
  const bytes =
    response.body.kind === 'bytes' ? response.body.value : encoder.encode(JSON.stringify(response.body.value))
  const contentType =
    response.body.kind === 'bytes' ? response.body.contentType : 'application/json; charset=utf-8'

  res.writeHead(response.status, {
    'Content-Type': contentType,
    'Content-Length': String(bytes.byteLength)
  })
  res.end(bytes)
}

/**
 * Innesta le rotte su un server `node:http`.
 *
 * Restituisce `false` quando il percorso non è del gruppo: chi ospita continua
 * con le proprie rotte, e questa funzione non ha scritto niente sulla risposta.
 * L'autenticazione resta a monte — il token bearer dell'accoppiamento va
 * verificato prima di chiamare qui.
 */
export async function serveSkinRoutes(
  router: SkinTransferRouter,
  req: IncomingMessage,
  res: ServerResponse
): Promise<boolean> {
  const url = new URL(req.url ?? '/', 'http://skin.invalid')
  if (!router.owns(url.pathname)) return false

  const method = (req.method ?? 'GET').toUpperCase()
  let bytes: Uint8Array | undefined
  let json: unknown

  if (method === 'POST') {
    // Il tetto dipende da cosa si sta ricevendo: un archivio o tre campi.
    const isUpload = url.pathname.endsWith('/upload')
    const body = await readBody(req, isUpload ? PACKAGE_LIMITS.maxArchiveBytes : JSON_BODY_LIMIT)
    if (!body.ok) {
      // La connessione può essere già chiusa: non si scrive su una risposta morta.
      if (!res.writableEnded) sendSkinResponse(res, failureResponse(body.error))
      return true
    }

    if (isUpload) {
      bytes = body.value
    } else if (body.value.byteLength > 0) {
      try {
        json = JSON.parse(new TextDecoder().decode(body.value))
      } catch (cause) {
        sendSkinResponse(
          res,
          failureResponse(
            AppError.of(
              'ipc.payloadInvalid',
              { channel: `POST ${url.pathname}`, detail: 'corpo non JSON' },
              { cause }
            )
          )
        )
        return true
      }
    }
  }

  const fingerprint = req.headers[FINGERPRINT_HEADER]
  const response = router.handle({
    method,
    path: url.pathname,
    ...(typeof fingerprint === 'string' ? { headers: { [FINGERPRINT_HEADER]: fingerprint } } : {}),
    ...(bytes === undefined ? {} : { bytes }),
    ...(json === undefined ? {} : { json })
  })

  // `owns` ha già detto di sì, quindi `handle` non può tornare null: se succede
  // le due funzioni sono uscite d'accordo, ed è un bug da vedere subito.
  if (response === null) {
    sendSkinResponse(
      res,
      failureResponse(AppError.of('internal.invariantViolated', { what: 'owns e handle in disaccordo' }))
    )
    return true
  }

  sendSkinResponse(res, response)
  return true
}

// ── il lato che chiama ──────────────────────────────────────────────────────

/**
 * Il minimo di `fetch` che serve.
 *
 * Descritto qui invece di usare il tipo globale perché questo file viene
 * tipizzato anche dal progetto senza DOM, e perché un `fetch` iniettato è ciò
 * che rende provabile il lato client senza rete.
 */
export interface HttpResponseLike {
  readonly ok: boolean
  readonly status: number
  json(): Promise<unknown>
  arrayBuffer(): Promise<ArrayBuffer>
}

export interface FetchInit {
  method?: string
  headers?: Record<string, string>
  body?: Uint8Array | string
  signal?: AbortSignal
}

export type FetchLike = (url: string, init?: FetchInit) => Promise<HttpResponseLike>

export interface HttpSkinPeerOptions {
  /** Radice del server dell'altro dispositivo, es. `http://192.168.1.7:8080`. */
  readonly baseUrl: string
  readonly fetch: FetchLike
  /** Il token bearer dell'accoppiamento, quando il server lo richiede. */
  readonly token?: string
  readonly signal?: AbortSignal
}

/** Un `SkinPeer` sopra HTTP. Rispecchia le rotte di `transfer.ts` una a una. */
export function createHttpSkinPeer(options: HttpSkinPeerOptions): SkinPeer {
  const base = options.baseUrl.replace(/\/+$/, '')

  function headers(extra?: Record<string, string>): Record<string, string> {
    return {
      ...(options.token === undefined ? {} : { Authorization: `Bearer ${options.token}` }),
      ...extra
    }
  }

  /**
   * Traduce una risposta non riuscita in un AppError.
   *
   * Il corpo è un `AppErrorPayload`, quindi l'errore dell'altro dispositivo
   * arriva con il suo codice e i suoi parametri: `AppError.from` lo riconosce e
   * lo ricostruisce. Solo se il corpo non è quello — un proxy in mezzo, un
   * server di un'altra versione — si ripiega su `net.http`, che almeno porta lo
   * stato.
   */
  async function failure(response: HttpResponseLike, url: string): Promise<AppError> {
    try {
      const body = (await response.json()) as { error?: unknown }
      const rebuilt = AppError.from(body.error)
      if (rebuilt.code !== 'internal.unexpected') return rebuilt
    } catch {
      /* il corpo non è JSON: si ripiega sullo stato */
    }
    return AppError.of('net.http', { status: response.status, url })
  }

  async function call<T>(
    method: string,
    path: string,
    init?: { body?: Uint8Array | string; headers?: Record<string, string>; bytes?: boolean }
  ): Promise<Result<T, AppError>> {
    const url = `${base}${path}`
    try {
      const response = await options.fetch(url, {
        method,
        headers: headers(init?.headers),
        ...(init?.body === undefined ? {} : { body: init.body }),
        ...(options.signal === undefined ? {} : { signal: options.signal })
      })
      if (!response.ok) return err(await failure(response, url))
      if (init?.bytes === true) {
        return ok(new Uint8Array(await response.arrayBuffer()) as unknown as T)
      }
      return ok((await response.json()) as T)
    } catch (cause) {
      // Un errore di rete arriva come errno (ECONNREFUSED, ETIMEDOUT): il
      // catalogo lo mappa già su net.offline / net.timeout.
      return err(AppError.from(cause, { context: { url } }))
    }
  }

  // Gli id sono vincolati a minuscole, cifre e trattini, quindi non c'è nulla da
  // codificare. Si codifica lo stesso: il giorno in cui il vincolo cambia, questo
  // non diventa una falla.
  const idPath = (id: string): string => `/api/skins/${encodeURIComponent(id)}`

  return {
    list: () => call<SkinListing>('GET', '/api/skins'),
    download: (id) => call<Uint8Array>('GET', idPath(id), { bytes: true }),
    upload: (archive, fingerprint) =>
      call<UploadAccepted>('POST', '/api/skins/upload', {
        body: archive,
        headers: { 'Content-Type': SKIN_PACKAGE_MIME, [FINGERPRINT_HEADER]: fingerprint }
      }),
    commit: (uploadId, overwrite) =>
      call<CommitDone>('POST', '/api/skins/commit', {
        body: JSON.stringify({ uploadId, overwrite }),
        headers: { 'Content-Type': 'application/json' }
      }),
    remove: async (id) => {
      const done = await call<unknown>('DELETE', idPath(id))
      return done.ok ? ok(true as const) : err(done.error)
    }
  }
}
