/**
 * Il pacchetto `.aeskin`: leggerlo e scriverlo.
 *
 * Un pacchetto arriva da fuori — da un file scelto dall'utente, o dalla rete
 * locale mandato da un telefono — quindi questo è il confine dove si assume che
 * chi ha costruito l'archivio possa averlo fatto in malafede. Le difese non sono
 * ipotetiche: un archivio è il vettore classico per tre attacchi concreti, e
 * ognuno ha qui una guardia dedicata.
 *
 * **Path traversal.** Una voce chiamata `../../.ssh/authorized_keys` scritta con
 * un estrattore ingenuo finisce fuori dalla cartella di destinazione. La difesa
 * non è normalizzare il percorso e sperare: è una lista chiusa di nomi ammessi.
 * `skin.json`, `preview.png`, e `assets/<nome>.<estensione>` — nient'altro passa,
 * quindi non esiste un percorso da normalizzare.
 *
 * **Zip bomb.** Un archivio da 40 KB può espandersi in gigabyte e far cadere il
 * processo per esaurimento di memoria — che sul backend mobile significa app
 * uccisa da Android. Le guardie sono tre e agiscono PRIMA di decomprimere:
 * dimensione dichiarata per voce, numero di voci, e rapporto di compressione.
 *
 * **Tipo mentito.** Un file chiamato `.png` che contiene un eseguibile, o un SVG
 * (che è un documento, e può contenere script). Il tipo si determina dai byte
 * iniziali, non dall'estensione, e l'SVG non è nella lista.
 *
 * Nota su cosa NON serve difendere: nessun asset finisce in un `url()`, perché il
 * compilatore non emette `url()` in nessun caso. Gli asset diventano `blob:`
 * interni, quindi una skin non può fare una richiesta di rete nemmeno con un
 * asset costruito ad arte.
 */

import { AppError } from '@aether/core'
import { err, ok, type Result } from '@aether/core'
import { strFromU8, strToU8, unzipSync, zipSync, type Unzipped } from 'fflate'
import { parseSkin } from './parse'
import type { SkinDocument } from './schema'

/** Il nome del manifest. È l'unica voce obbligatoria. */
export const MANIFEST_NAME = 'skin.json'
export const PREVIEW_NAME = 'preview.png'
const ASSET_PREFIX = 'assets/'

/**
 * I limiti.
 *
 * Scelti sul contenuto reale: una skin è testo più qualche immagine e un paio di
 * caratteri woff2. Un pacchetto da 20 MB non è una skin, è qualcos'altro.
 */
export const PACKAGE_LIMITS = {
  /** Archivio compresso. */
  maxArchiveBytes: 20 * 1024 * 1024,
  /** Somma di tutto ciò che si decomprime. */
  maxTotalBytes: 60 * 1024 * 1024,
  /** Una singola voce. */
  maxEntryBytes: 8 * 1024 * 1024,
  /** Il manifest: è testo, e un manifest da mezzo mega è già assurdo. */
  maxManifestBytes: 512 * 1024,
  maxEntries: 64,
  /**
   * Rapporto massimo fra decompresso e compresso, per voce.
   *
   * Il testo comprime bene — un JSON può arrivare a 15× — quindi la soglia non
   * può essere bassa. 200× non ostacola nessun contenuto legittimo e taglia le
   * bombe, che stanno negli ordini di 1.000× e oltre.
   */
  maxCompressionRatio: 200
} as const

/** Estensioni ammesse per gli asset, e la loro firma nei byte iniziali. */
const ASSET_TYPES = {
  png: { mime: 'image/png', magic: [0x89, 0x50, 0x4e, 0x47] },
  jpg: { mime: 'image/jpeg', magic: [0xff, 0xd8, 0xff] },
  webp: { mime: 'image/webp', magic: [0x52, 0x49, 0x46, 0x46] },
  // WOFF2: 'wOF2'. I caratteri di sistema restano fuori discussione — un carattere
  // in un pacchetto viene caricato con FontFace e un nome namespaced per skin.
  woff2: { mime: 'font/woff2', magic: [0x77, 0x4f, 0x46, 0x32] }
} as const

type AssetExtension = keyof typeof ASSET_TYPES

export interface SkinAsset {
  /** Il nome dentro `assets/`, senza prefisso. */
  readonly name: string
  readonly mime: string
  readonly bytes: Uint8Array
}

export interface SkinPackage {
  /**
   * Il documento validato, nella forma interna: colori scomposti in canali,
   * lunghezze come numero più unità. È ciò che il compilatore consuma.
   */
  readonly document: SkinDocument
  /**
   * Il JSON come è stato scritto.
   *
   * Serve, e la ragione l'ho scoperta sbagliando: la forma interna **non è
   * serializzabile come sorgente.** Un colore validato è `{r:139,g:124,b:246,a:1}`,
   * e riscriverlo nel pacchetto produrrebbe un manifest che la validazione
   * rifiuta — un pacchetto illeggibile dallo stesso codice che l'ha prodotto. Lo
   * Studio ha bisogno della sorgente anche per un'altra ragione: è la forma che
   * l'autore ha scritto, e un editor deve modificare quella.
   */
  readonly source: unknown
  readonly preview: Uint8Array | null
  readonly assets: readonly SkinAsset[]
}

/** Il nome è ammesso? Lista chiusa: non c'è nulla da normalizzare. */
function classifyEntry(
  name: string
): { kind: 'manifest' | 'preview' } | { kind: 'asset'; extension: AssetExtension } | null {
  if (name === MANIFEST_NAME) return { kind: 'manifest' }
  if (name === PREVIEW_NAME) return { kind: 'preview' }

  if (!name.startsWith(ASSET_PREFIX)) return null

  const leaf = name.slice(ASSET_PREFIX.length)
  // Un solo livello: nessuna sottocartella, quindi nessun percorso da risolvere.
  // E un nome di file vincolato, che esclude i punti doppi per costruzione.
  if (!/^[a-z0-9][a-z0-9_-]*\.[a-z0-9]+$/.test(leaf)) return null

  const extension = leaf.slice(leaf.lastIndexOf('.') + 1) as AssetExtension
  if (!Object.prototype.hasOwnProperty.call(ASSET_TYPES, extension)) return null

  return { kind: 'asset', extension }
}

function startsWithMagic(bytes: Uint8Array, magic: readonly number[]): boolean {
  if (bytes.length < magic.length) return false
  for (let index = 0; index < magic.length; index++) {
    if (bytes[index] !== magic[index]) return false
  }
  return true
}

function rejected(asset: string, reason: string): AppError {
  return AppError.of('skin.assetRejected', { asset, reason })
}

/**
 * Legge un pacchetto. Non lancia mai.
 *
 * L'ordine dei controlli è deliberato: prima la dimensione dell'archivio, poi i
 * nomi e le dimensioni DICHIARATE delle voci, e solo dopo la decompressione. Un
 * controllo fatto dopo aver decompresso non protegge da niente.
 */
export function readSkinPackage(archive: Uint8Array): Result<SkinPackage, AppError> {
  if (archive.byteLength > PACKAGE_LIMITS.maxArchiveBytes) {
    return err(
      AppError.of('skin.tooLarge', {
        bytes: archive.byteLength,
        limitBytes: PACKAGE_LIMITS.maxArchiveBytes
      })
    )
  }

  let entries: Unzipped
  const problems: AppError[] = []
  let totalBytes = 0
  let seen = 0

  try {
    entries = unzipSync(archive, {
      /*
       * Il filtro riceve le dimensioni DICHIARATE nella struttura dell'archivio,
       * prima che i dati vengano espansi. È il punto in cui una bomba viene
       * fermata: dopo, la memoria è già stata allocata.
       */
      filter: (file) => {
        seen++
        if (seen > PACKAGE_LIMITS.maxEntries) {
          problems.push(rejected(file.name, `il pacchetto supera le ${PACKAGE_LIMITS.maxEntries} voci`))
          return false
        }

        const kind = classifyEntry(file.name)
        if (kind === null) {
          // Copre anche il path traversal: `../x` non è un nome ammesso, quindi
          // non arriva mai a essere un percorso.
          problems.push(rejected(file.name, 'nome non ammesso dal formato'))
          return false
        }

        const limit =
          kind.kind === 'manifest' ? PACKAGE_LIMITS.maxManifestBytes : PACKAGE_LIMITS.maxEntryBytes
        if (file.originalSize > limit) {
          problems.push(
            rejected(file.name, `voce di ${file.originalSize} byte, oltre il limite di ${limit}`)
          )
          return false
        }

        if (file.size > 0 && file.originalSize / file.size > PACKAGE_LIMITS.maxCompressionRatio) {
          problems.push(
            rejected(
              file.name,
              `rapporto di compressione ${Math.round(file.originalSize / file.size)}×, sospetto`
            )
          )
          return false
        }

        totalBytes += file.originalSize
        if (totalBytes > PACKAGE_LIMITS.maxTotalBytes) {
          problems.push(rejected(file.name, 'il contenuto decompresso supera il limite totale'))
          return false
        }

        return true
      }
    })
  } catch (cause) {
    // Un archivio illeggibile è corrotto, non malevolo: sono due messaggi diversi
    // e l'utente deve poterli distinguere (uno si riscarica, l'altro no).
    return err(AppError.of('skin.packageCorrupt', { detail: 'archivio non leggibile' }, { cause }))
  }

  const firstProblem = problems[0]
  if (firstProblem !== undefined) {
    // Il primo problema è quello che si riporta, ma il conteggio va nel contesto:
    // un pacchetto con quaranta voci rifiutate è un'altra cosa da uno con una.
    return err(firstProblem.withContext({ rejectedEntries: problems.length }))
  }

  const manifestBytes = entries[MANIFEST_NAME]
  if (manifestBytes === undefined) {
    return err(AppError.of('skin.packageCorrupt', { detail: `manca ${MANIFEST_NAME}` }))
  }

  let raw: unknown
  try {
    raw = JSON.parse(strFromU8(manifestBytes))
  } catch (cause) {
    return err(
      AppError.of('skin.manifestInvalid', { detail: 'il manifest non è JSON valido' }, { cause })
    )
  }

  const document = parseSkin(raw)
  if (!document.ok) return err(document.error)

  const assets: SkinAsset[] = []
  for (const name of Object.keys(entries).sort()) {
    if (name === MANIFEST_NAME || name === PREVIEW_NAME) continue
    const bytes = entries[name]
    if (bytes === undefined) continue

    const kind = classifyEntry(name)
    if (kind === null || kind.kind !== 'asset') continue

    const type = ASSET_TYPES[kind.extension]
    // Il tipo dai byte, non dall'estensione: un file chiamato .png che contiene
    // altro è precisamente il caso da fermare.
    if (!startsWithMagic(bytes, type.magic)) {
      return err(
        rejected(name, `il contenuto non corrisponde all'estensione .${kind.extension}`)
      )
    }

    assets.push({ name: name.slice(ASSET_PREFIX.length), mime: type.mime, bytes })
  }

  const preview = entries[PREVIEW_NAME]
  if (preview !== undefined && !startsWithMagic(preview, ASSET_TYPES.png.magic)) {
    return err(rejected(PREVIEW_NAME, 'la miniatura non è un PNG'))
  }

  return ok({
    document: document.value,
    source: raw,
    preview: preview ?? null,
    assets
  })
}

export interface WritePackageInput {
  /**
   * Il manifest nella forma SORGENTE, cioè come lo scrive un autore.
   *
   * Non il documento validato: la forma interna non si può riserializzare —
   * un colore validato è un oggetto di canali, e un manifest costruito da quello
   * verrebbe rifiutato dalla lettura. Viene validato prima di essere scritto,
   * quindi un pacchetto non valido non esce da qui.
   */
  readonly source: unknown
  readonly preview?: Uint8Array
  readonly assets?: readonly SkinAsset[]
}

/**
 * Scrive un pacchetto.
 *
 * Passa dalle STESSE guardie della lettura, applicate al proprio output: un
 * pacchetto che questo codice produce e che la lettura rifiuterebbe è un bug, e
 * scoprirlo qui è meglio che scoprirlo sul telefono di qualcuno dopo il
 * trasferimento.
 */
export function writeSkinPackage(input: WritePackageInput): Result<Uint8Array, AppError> {
  // Si valida prima di scrivere: un pacchetto non valido non deve poter esistere,
  // e scoprirlo all'esportazione è incomparabilmente meglio che scoprirlo
  // all'importazione sull'altro dispositivo.
  const validated = parseSkin(input.source)
  if (!validated.ok) return err(validated.error)

  const files: Record<string, Uint8Array> = {
    [MANIFEST_NAME]: strToU8(`${JSON.stringify(input.source, null, 2)}\n`)
  }

  if (input.preview !== undefined) {
    if (!startsWithMagic(input.preview, ASSET_TYPES.png.magic)) {
      return err(rejected(PREVIEW_NAME, 'la miniatura deve essere un PNG'))
    }
    files[PREVIEW_NAME] = input.preview
  }

  for (const asset of input.assets ?? []) {
    const name = `${ASSET_PREFIX}${asset.name}`
    const kind = classifyEntry(name)
    if (kind === null || kind.kind !== 'asset') {
      return err(rejected(asset.name, 'nome di asset non ammesso dal formato'))
    }
    if (!startsWithMagic(asset.bytes, ASSET_TYPES[kind.extension].magic)) {
      return err(rejected(asset.name, `il contenuto non corrisponde all'estensione`))
    }
    if (asset.bytes.byteLength > PACKAGE_LIMITS.maxEntryBytes) {
      return err(
        AppError.of('skin.tooLarge', {
          bytes: asset.bytes.byteLength,
          limitBytes: PACKAGE_LIMITS.maxEntryBytes
        })
      )
    }
    files[name] = asset.bytes
  }

  try {
    // Livello 9 sul manifest, nessuna compressione sugli asset: sono già formati
    // compressi, e ricomprimerli costa tempo per guadagnare nulla.
    return ok(zipSync(files, { level: 6, mem: 8 }))
  } catch (cause) {
    return err(
      AppError.of('internal.unexpected', { detail: 'creazione dell\'archivio non riuscita' }, { cause })
    )
  }
}

/** Il nome del file per una skin. Usato dall'esportazione e dal trasferimento. */
export function packageFileName(document: SkinDocument): string {
  // L'id è già vincolato a minuscole, cifre e trattini, quindi è un nome di file
  // sicuro per costruzione: nessuna sanificazione da fare qui.
  return `${document.id}-${document.meta.version}.aeskin`
}
