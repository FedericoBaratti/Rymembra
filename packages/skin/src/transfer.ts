/**
 * Il trasporto delle skin: le rotte, senza HTTP.
 *
 * I due server esistono già nel legacy — quello LAN del desktop
 * (`electron/modules/lan/server.ts`) e quello di trasferimento del telefono
 * (`node-backend/transfer/server.ts`) — con l'accoppiamento a QR, i token
 * bearer, mDNS e il rate limiting. Questo file NON è un terzo server: è il
 * gruppo di rotte che quei due montano, ed è scritto per essere innestato.
 *
 * Da qui vengono due decisioni di forma.
 *
 * **`handle` restituisce `null` quando il percorso non è suo.** È lo stesso
 * contratto di `handleApiRoute` nel legacy, che tornava `false`: il server che
 * ospita continua a provare le proprie rotte, e questo modulo possiede solo
 * `/api/skins`. Un router che risponde 404 su ciò che non gli appartiene non si
 * può montare accanto a nient'altro.
 *
 * **Il corpo arriva già letto.** Leggerlo è lavoro del server che ospita: ha lui
 * lo streaming, il tetto sui byte, l'annullamento a metà. Qui il corpo è un
 * `Uint8Array` o un JSON già decodificato, e tutto il file resta sincrono e
 * provabile senza aprire una porta. Ogni caso che conta — un archivio corrotto,
 * un id che collide, uno staging scaduto — si prova come una chiamata di
 * funzione.
 *
 * Sulle rotte simmetriche: i due lati montano le STESSE rotte, con una sola
 * differenza dichiarata (`allowRemove`). Il desktop è il posto dove le skin si
 * creano, e una cancellazione arrivata dalla rete distruggerebbe un lavoro senza
 * modo di tornare; il telefono è una destinazione, e gestirne la libreria dal PC
 * è esattamente il punto della funzione.
 *
 * Quel che questo file NON fa, perché è di chi ospita: autenticare (il token
 * bearer dell'accoppiamento), limitare la frequenza, e decidere su quale
 * interfaccia ascoltare.
 */

import { AppError } from '@aether/core'
import { skinFingerprint, type LibraryEntry } from './library'
import { readSkinPackage } from './package'
import { skinIdSchema } from './schema'
import type { SkinLibrary } from './store'

/**
 * La versione del protocollo di trasporto.
 *
 * Separata da `SKIN_FORMAT_VERSION`: il formato di un pacchetto e il dialogo fra
 * due dispositivi cambiano per ragioni diverse e in momenti diversi. Viaggia
 * nell'elenco così che un telefono con una build vecchia se ne accorga dal primo
 * scambio, invece di fallire su una rotta a metà trasferimento.
 */
export const SKIN_TRANSFER_PROTOCOL = 1

/** Il tipo MIME di un `.aeskin` sul filo. */
export const SKIN_PACKAGE_MIME = 'application/vnd.aether.skin'

/**
 * L'intestazione con cui chi manda dichiara l'impronta di ciò che sta mandando.
 *
 * Facoltativa, e serve a un caso preciso: il piano di allineamento è costruito
 * sulle impronte, quindi chi riceve deve poter verificare che i byte arrivati
 * siano quelli su cui il piano ha deciso. Senza, un pacchetto sostituito o
 * troncato in modo da restare un archivio valido si installerebbe come se fosse
 * quello approvato.
 */
export const FINGERPRINT_HEADER = 'x-aether-skin-fingerprint'

/**
 * I limiti dello staging.
 *
 * Lo staging tiene byte in memoria in attesa di una conferma che può non
 * arrivare mai — il telefono esce dal Wi-Fi, l'utente chiude la finestra della
 * domanda. Senza tetto e senza scadenza sarebbe una perdita di memoria comandata
 * dall'altro dispositivo, e sul backend mobile una perdita di memoria è l'app
 * uccisa da Android.
 */
export interface StagingLimits {
  /** Quante attese contemporanee. Un allineamento manda una skin per volta. */
  readonly maxUploads: number
  /** Tetto complessivo dei byte in attesa. */
  readonly maxBytes: number
  /** Dopo quanto un'attesa senza commit si butta. */
  readonly ttlMs: number
}

export const STAGING_LIMITS: StagingLimits = {
  maxUploads: 4,
  maxBytes: 24 * 1024 * 1024,
  ttlMs: 5 * 60 * 1000
}

// ── il confine ──────────────────────────────────────────────────────────────

export interface SkinTransferRequest {
  readonly method: string
  /** Il percorso, già senza la query. */
  readonly path: string
  /** Intestazioni con il nome in minuscolo. Le normalizza chi ospita. */
  readonly headers?: Readonly<Record<string, string | undefined>>
  /** I byte dell'archivio: solo per l'upload. */
  readonly bytes?: Uint8Array
  /** Il JSON già decodificato: solo per il commit. */
  readonly json?: unknown
}

export type SkinTransferBody =
  | { readonly kind: 'json'; readonly value: unknown }
  | { readonly kind: 'bytes'; readonly value: Uint8Array; readonly contentType: string }

export interface SkinTransferResponse {
  readonly status: number
  readonly body: SkinTransferBody
}

/** La forma di un errore sul filo. Vedi `wireError` per cosa NON contiene. */
export interface SkinTransferError {
  readonly code: string
  readonly domain: string
  readonly retryable: boolean
  readonly params: Record<string, unknown>
  readonly i18nKey: string
  readonly message: string
  readonly traceId: string
}

/** Quel che l'upload risponde: tutto ciò che serve a decidere se committare. */
export interface UploadAccepted {
  readonly uploadId: string
  readonly id: string
  readonly name: string
  readonly version: string
  readonly fingerprint: string
  /** Cosa c'è già installato con questo id, se c'è. */
  readonly installed: InstalledSummary | null
  /** Il contenuto è già identico a quello installato: il commit è inutile. */
  readonly identical: boolean
}

export interface InstalledSummary {
  /** `null` quando il pacchetto installato non si riesce a leggere. */
  readonly version: string | null
  readonly fingerprint: string | null
  /** Il file c'è ma è illeggibile: la sovrascrittura è la cura, non il rischio. */
  readonly unreadable: boolean
}

export interface SkinListing {
  readonly protocol: number
  readonly entries: readonly LibraryEntry[]
  /** Quanti pacchetti installati non si riescono a leggere su questo lato. */
  readonly unreadable: number
}

export interface CommitDone {
  readonly id: string
  readonly name: string
  readonly version: string
  readonly fingerprint: string
}

export interface SkinTransferOptions {
  readonly library: SkinLibrary
  /**
   * Se questo lato accetta `DELETE`. Il telefono sì, il desktop no: vedi la nota
   * in testa al file.
   */
  readonly allowRemove: boolean
  /**
   * Le voci delle skin di serie.
   *
   * Servono a due cose con lo stesso dato. Nell'elenco: se un lato ha una skin di
   * serie che l'altro non ha ancora (build diverse), l'allineamento la marca
   * `skipBuiltin` invece di provare a mandarla — un invio che finirebbe comunque
   * rifiutato, con un errore che sembra un guasto. E in ingresso: i loro id sono
   * quelli riservati, quindi un upload che ne usa uno si ferma prima di occupare
   * lo staging.
   */
  readonly builtins?: readonly LibraryEntry[]
  /** Orologio iniettato: la scadenza dello staging si prova solo se si controlla. */
  readonly now?: () => number
  readonly limits?: Partial<StagingLimits>
}

export interface SkinTransferRouter {
  /** `null` quando il percorso non appartiene a questo gruppo di rotte. */
  handle(request: SkinTransferRequest): SkinTransferResponse | null
  /** Quante attese di commit ci sono. Per la diagnostica e per i test. */
  pendingUploads(): number
}

// ── errori sul filo ─────────────────────────────────────────────────────────

/**
 * Lo stato HTTP di un errore.
 *
 * La mappa è esplicita invece che per dominio perché la differenza che conta è
 * fra «ritenta uguale» (410: rimanda i byte), «chiedi all'utente» (409: c'è già)
 * e «non insistere» (403, 400). Un 500 generico li appiattirebbe tutti su
 * «riprova più tardi», che per tre casi su quattro è il consiglio sbagliato.
 */
function statusFor(error: AppError): number {
  switch (error.code) {
    case 'skin.notFound':
      return 404
    case 'skin.idConflict':
      return 409
    case 'skin.builtinReadOnly':
      return 403
    case 'skin.tooLarge':
      return 413
    case 'transfer.integrityMismatch':
      return 422
    case 'transfer.methodNotSupported':
      return 405
    // Lo staging non c'è più: i byte vanno rimandati. È esattamente 410.
    case 'transfer.aborted':
      return 410
    case 'ipc.payloadInvalid':
      return 400
    default:
      // Il dominio skin è sempre «questo pacchetto non va bene», cioè colpa di
      // chi manda; tutto il resto è un guasto di questo lato.
      return error.domain === 'skin' ? 400 : 500
  }
}

/**
 * L'errore che attraversa la rete.
 *
 * È il payload di AppError meno `stack` e `context`. Non è prudenza generica:
 * quei due campi contengono percorsi locali e nomi di file di questo
 * dispositivo, e l'altro capo non ha nulla da farci. `params` invece resta —
 * sono i dati che l'i18n interpola, cioè il messaggio stesso — e con `code`,
 * `retryable` e `traceId` l'errore si ricostruisce dall'altra parte senza
 * indovinare nulla, che è la promessa di tutto lo strato degli errori.
 */
export function wireError(error: AppError): SkinTransferError {
  return {
    code: error.code,
    domain: error.domain,
    retryable: error.retryable,
    params: error.params,
    i18nKey: error.i18nKey,
    message: error.message,
    traceId: error.traceId
  }
}

function fail(error: AppError): SkinTransferResponse {
  return { status: statusFor(error), body: { kind: 'json', value: { error: wireError(error) } } }
}

function json(status: number, value: unknown): SkinTransferResponse {
  return { status, body: { kind: 'json', value } }
}

function methodNotSupported(method: string, path: string): SkinTransferResponse {
  return fail(AppError.of('transfer.methodNotSupported', { method: `${method} ${path}` }))
}

// ── lo staging ──────────────────────────────────────────────────────────────

interface Staged {
  readonly archive: Uint8Array
  readonly stagedAt: number
}

let uploadCounter = 0

/**
 * Un identificatore d'attesa.
 *
 * Non finisce in un percorso — i byte restano in memoria, non su disco — quindi
 * non c'è nulla da sanificare. Resta comunque una forma riconoscibile, così un
 * commit con un id inventato si distingue nei log da uno con un id scaduto.
 */
function newUploadId(): string {
  uploadCounter = (uploadCounter + 1) % 0xffff
  const rand = Math.floor(Math.random() * 0xffffff).toString(16).padStart(6, '0')
  return `upl-${uploadCounter.toString(16)}-${rand}`
}

// ── le rotte ────────────────────────────────────────────────────────────────

function readString(value: unknown, key: string): string | null {
  if (typeof value !== 'object' || value === null) return null
  const field = (value as Record<string, unknown>)[key]
  return typeof field === 'string' && field.length > 0 ? field : null
}

function readBoolean(value: unknown, key: string): boolean {
  if (typeof value !== 'object' || value === null) return false
  return (value as Record<string, unknown>)[key] === true
}

/** Un id che il formato non potrebbe produrre non può nominare una skin installata. */
function isValidId(id: string): boolean {
  return skinIdSchema.safeParse(id).success
}

export function createSkinTransferRouter(options: SkinTransferOptions): SkinTransferRouter {
  const { library, allowRemove } = options
  const clock = options.now ?? Date.now
  const limits: StagingLimits = { ...STAGING_LIMITS, ...options.limits }
  const builtins = options.builtins ?? []
  const reserved = new Set(builtins.map((entry) => entry.id))

  /*
   * Le attese, in ordine di arrivo. Le chiavi di una Map si scorrono nell'ordine
   * di inserimento, e nessun uploadId viene mai riusato: la prima chiave è
   * quindi sempre la più vecchia, che è tutto ciò che serve allo sfratto.
   */
  const pending = new Map<string, Staged>()

  function stagedBytes(): number {
    let total = 0
    for (const staged of pending.values()) total += staged.archive.byteLength
    return total
  }

  function prune(): void {
    const cutoff = clock() - limits.ttlMs
    for (const [key, staged] of pending) {
      if (staged.stagedAt <= cutoff) pending.delete(key)
    }
  }

  /** Sfratta le attese più vecchie finché la nuova ci sta. */
  function makeRoom(incoming: number): void {
    while (
      pending.size > 0 &&
      (pending.size >= limits.maxUploads || stagedBytes() + incoming > limits.maxBytes)
    ) {
      const oldest = pending.keys().next().value
      if (oldest === undefined) return
      pending.delete(oldest)
    }
  }

  /** Cosa c'è già installato con questo id. Distingue «niente» da «illeggibile». */
  function summarize(id: string): InstalledSummary | null {
    const found = library.get(id)
    if (found.ok) {
      return {
        version: found.value.entry.version,
        fingerprint: found.value.entry.fingerprint,
        unreadable: false
      }
    }
    if (found.error.code === 'skin.notFound') return null
    /*
     * Il file c'è ma non si legge: trasferimento interrotto, settore perso. Va
     * detto, perché altrimenti chi manda non chiederebbe la sovrascrittura e il
     * commit fallirebbe con un conflitto su una skin che l'utente non vede
     * nemmeno nell'elenco.
     */
    return { version: null, fingerprint: null, unreadable: true }
  }

  function list(): SkinTransferResponse {
    const { installed, broken } = library.list()
    /*
     * Le rotte restano fuori dall'elenco, e questo produce il comportamento
     * giusto senza codice dedicato: l'altro lato vede la skin come mancante, la
     * manda, il commit trova il file e chiede la sovrascrittura, e il pacchetto
     * illeggibile viene rimpiazzato da quello buono. La riparazione è un caso
     * particolare dell'allineamento.
     */
    const entries = [...builtins, ...installed.map((skin) => skin.entry)].sort((a, b) =>
      a.id < b.id ? -1 : a.id > b.id ? 1 : 0
    )
    const listing: SkinListing = {
      protocol: SKIN_TRANSFER_PROTOCOL,
      entries,
      unreadable: broken.length
    }
    return json(200, listing)
  }

  function download(id: string): SkinTransferResponse {
    if (!isValidId(id)) return fail(AppError.of('skin.notFound', { id }))
    const archive = library.export(id)
    if (!archive.ok) return fail(archive.error)
    return { status: 200, body: { kind: 'bytes', value: archive.value, contentType: SKIN_PACKAGE_MIME } }
  }

  function upload(request: SkinTransferRequest): SkinTransferResponse {
    const archive = request.bytes
    if (archive === undefined || archive.byteLength === 0) {
      return fail(
        AppError.of('skin.packageCorrupt', { detail: 'nessun archivio nel corpo della richiesta' })
      )
    }

    // Si valida PRIMA di occupare lo staging: un pacchetto che non si potrà mai
    // installare non deve poter riempire la memoria in attesa di un commit.
    const parsed = readSkinPackage(archive)
    if (!parsed.ok) return fail(parsed.error)

    const { document, source } = parsed.value
    const print = skinFingerprint(source)

    const declared = request.headers?.[FINGERPRINT_HEADER]
    if (declared !== undefined && declared !== print) {
      return fail(AppError.of('transfer.integrityMismatch', { expected: declared, actual: print }))
    }

    if (reserved.has(document.id)) {
      return fail(AppError.of('skin.builtinReadOnly', { id: document.id }))
    }

    prune()
    makeRoom(archive.byteLength)
    const uploadId = newUploadId()
    pending.set(uploadId, { archive, stagedAt: clock() })

    const installed = summarize(document.id)
    const accepted: UploadAccepted = {
      uploadId,
      id: document.id,
      name: document.meta.name,
      version: document.meta.version,
      fingerprint: print,
      installed,
      identical: installed !== null && installed.fingerprint === print
    }
    return json(200, accepted)
  }

  function commit(request: SkinTransferRequest): SkinTransferResponse {
    const uploadId = readString(request.json, 'uploadId')
    if (uploadId === null) {
      return fail(
        AppError.of('ipc.payloadInvalid', {
          channel: 'POST /api/skins/commit',
          detail: 'manca uploadId'
        })
      )
    }

    prune()
    const staged = pending.get(uploadId)
    if (staged === undefined) {
      return fail(
        AppError.of('transfer.aborted', {
          reason: 'upload non più in attesa: scaduto, sfrattato o mai ricevuto'
        })
      )
    }

    const installed = library.install(staged.archive, { overwrite: readBoolean(request.json, 'overwrite') })
    if (!installed.ok) {
      /*
       * I byte restano in attesa, ed è il motivo per cui l'upload e il commit
       * sono due chiamate. Il caso normale è il conflitto di id: chi manda
       * chiede all'utente e ricommitta con `overwrite`, senza rimandare
       * l'archivio. Vale anche per un disco pieno, dove il rimedio è liberare
       * spazio e riprovare — non ritrasferire venti mega sul Wi-Fi.
       */
      return fail(installed.error)
    }

    pending.delete(uploadId)
    const done: CommitDone = {
      id: installed.value.document.id,
      name: installed.value.document.meta.name,
      version: installed.value.document.meta.version,
      fingerprint: installed.value.entry.fingerprint
    }
    return json(200, done)
  }

  function remove(id: string): SkinTransferResponse {
    if (!allowRemove) {
      return fail(AppError.of('transfer.methodNotSupported', { method: 'DELETE /api/skins/:id' }))
    }
    if (!isValidId(id)) return fail(AppError.of('skin.notFound', { id }))
    const removed = library.remove(id)
    if (!removed.ok) return fail(removed.error)
    return json(200, { id, removed: true })
  }

  return {
    pendingUploads: () => pending.size,

    handle(request) {
      const segments = request.path.split('/').filter(Boolean)
      if (segments[0] !== 'api' || segments[1] !== 'skins') return null

      const rest = segments.slice(2)
      const method = request.method.toUpperCase()

      if (rest.length === 0) {
        if (method === 'GET') return list()
        return methodNotSupported(method, '/api/skins')
      }

      const leaf = rest[0]
      if (rest.length > 1 || leaf === undefined) {
        return fail(AppError.of('skin.notFound', { id: rest.join('/') }))
      }

      /*
       * `upload` e `commit` si riconoscono dal METODO prima che dal nome, e non
       * è un dettaglio: `upload` è un id di skin perfettamente legale, e una
       * rotta che guardasse solo il nome renderebbe irraggiungibile una skin
       * chiamata così. Con POST riservato alle due azioni e GET/DELETE riservati
       * agli id, la collisione non esiste.
       */
      if (method === 'POST') {
        if (leaf === 'upload') return upload(request)
        if (leaf === 'commit') return commit(request)
        return methodNotSupported(method, `/api/skins/${leaf}`)
      }
      if (method === 'GET') return download(leaf)
      if (method === 'DELETE') return remove(leaf)
      return methodNotSupported(method, '/api/skins/:id')
    }
  }
}
