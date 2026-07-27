/**
 * La libreria delle skin installate.
 *
 * È il pezzo che mancava fra il formato di pacchetto e tutto ciò che lo usa: lo
 * Studio deve salvare ed esportare, il trasporto deve ricevere e installare, il
 * selettore delle impostazioni deve elencare. Senza, il formato è un formato e
 * nient'altro.
 *
 * Il filesystem arriva iniettato, per due ragioni concrete e non per purismo:
 * la stessa logica gira sul desktop con `node:fs` e sul mobile attraverso lo
 * Storage Access Framework, che non è un filesystem POSIX; e i casi che contano —
 * un pacchetto corrotto già installato, un id che collide, un disco pieno a metà
 * scrittura — si provano solo se il filesystem si può controllare.
 *
 * Le skin di serie NON stanno qui: arrivano dal bundle, non si installano e non si
 * cancellano. Tenerle nella stessa collezione delle installate significherebbe
 * poterle sovrascrivere con una versione rotta e non avere più modo di tornare.
 */

import { AppError } from '@aether/core'
import { err, ok, partition, type Result } from '@aether/core'
import { libraryEntryFor, type LibraryEntry } from './library'
import { readSkinPackage, writeSkinPackage, type SkinPackage } from './package'
import { parseSkin } from './parse'
import type { SkinDocument } from './schema'

/**
 * Le operazioni sul filesystem che la libreria richiede.
 *
 * Deliberatamente poche: elencare, leggere, scrivere, cancellare. Nessuna
 * primitiva di percorso — la composizione dei nomi la fa questo file, così un id
 * non può diventare un percorso.
 */
export interface SkinStorage {
  /** I nomi dei file presenti. Non ricorsiva: la libreria è piatta. */
  list(): readonly string[]
  read(name: string): Uint8Array
  write(name: string, bytes: Uint8Array): void
  remove(name: string): void
  exists(name: string): boolean
}

/** Il suffisso dei pacchetti installati. Un nome per id, quindi uno per skin. */
const EXTENSION = '.aeskin'

/**
 * Il nome di file di una skin installata.
 *
 * Solo l'id, senza versione: la libreria tiene UNA copia per skin, e la versione
 * sta dentro il manifest. Metterla nel nome darebbe due file per la stessa skin
 * dopo un aggiornamento, e nessuno dei due sarebbe autorevole.
 */
function fileNameFor(id: string): string {
  return `${id}${EXTENSION}`
}

/** Gli id dei pacchetti installati, dedotti dai nomi dei file. */
function installedIds(storage: SkinStorage): string[] {
  return storage
    .list()
    .filter((name) => name.endsWith(EXTENSION))
    .map((name) => name.slice(0, -EXTENSION.length))
    .sort()
}

export interface InstalledSkin {
  readonly document: SkinDocument
  readonly source: unknown
  readonly entry: LibraryEntry
  readonly preview: Uint8Array | null
}

export interface SkinLibrary {
  /** Le skin installate leggibili, più gli errori di quelle che non lo sono. */
  list(): { installed: readonly InstalledSkin[]; broken: readonly AppError[] }
  get(id: string): Result<InstalledSkin, AppError>
  /** Installa un pacchetto. `overwrite` va chiesto all'utente, non deciso qui. */
  install(archive: Uint8Array, options?: { overwrite?: boolean }): Result<InstalledSkin, AppError>
  /** Salva una skin dallo Studio, senza passare da un archivio su disco. */
  save(source: unknown, options?: { preview?: Uint8Array }): Result<InstalledSkin, AppError>
  remove(id: string): Result<true, AppError>
  /** Il pacchetto da esportare o mandare al telefono. */
  export(id: string): Result<Uint8Array, AppError>
  /** Le voci per l'allineamento con l'altro dispositivo. */
  entries(): readonly LibraryEntry[]
}

/**
 * Gli id che non si possono usare per una skin installata.
 *
 * Sono quelli delle skin di serie: una installata con lo stesso id vincerebbe nel
 * selettore e renderebbe irraggiungibile quella di serie, senza modo di accorgersene
 * — il selettore mostrerebbe un nome familiare con un aspetto diverso.
 */
export interface SkinLibraryOptions {
  readonly storage: SkinStorage
  readonly reservedIds?: readonly string[]
}

export function createSkinLibrary(options: SkinLibraryOptions): SkinLibrary {
  const { storage } = options
  const reserved = new Set(options.reservedIds ?? [])

  function readOne(id: string): Result<InstalledSkin, AppError> {
    const name = fileNameFor(id)
    if (!storage.exists(name)) return err(AppError.of('skin.notFound', { id }))

    let archive: Uint8Array
    try {
      archive = storage.read(name)
    } catch (cause) {
      // Un errore di lettura è di filesystem, non di formato: permessi, file
      // scomparso, disco che non risponde. Il codice lo dice, così l'utente sa se
      // riprovare o reinstallare.
      return err(AppError.from(cause).withContext({ skinId: id }))
    }

    const parsed = readSkinPackage(archive)
    if (!parsed.ok) return err(parsed.error.withContext({ skinId: id }))

    return ok(toInstalled(parsed.value))
  }

  function toInstalled(pkg: SkinPackage): InstalledSkin {
    return {
      document: pkg.document,
      source: pkg.source,
      entry: libraryEntryFor(pkg.document, pkg.source, false),
      preview: pkg.preview
    }
  }

  function writePackage(
    source: unknown,
    preview: Uint8Array | undefined,
    expectedId: string
  ): Result<InstalledSkin, AppError> {
    const written = writeSkinPackage(preview === undefined ? { source } : { source, preview })
    if (!written.ok) return err(written.error)

    try {
      storage.write(fileNameFor(expectedId), written.value)
    } catch (cause) {
      // Il disco pieno arriva qui, e va detto come errore di filesystem: «skin non
      // salvata» senza il motivo manderebbe l'utente a cercare nel posto sbagliato.
      return err(AppError.from(cause).withContext({ skinId: expectedId }))
    }

    return readOne(expectedId)
  }

  return {
    list() {
      /*
       * Le rotte e le buone si separano invece di far fallire tutto.
       *
       * Un pacchetto corrotto — trasferimento interrotto, disco che ha perso un
       * settore — non deve rendere invisibili le altre nove skin installate. È lo
       * stesso ragionamento di `partition` sulla scansione della libreria: in
       * un'operazione in blocco, un elemento rotto non annulla gli altri.
       */
      const results = installedIds(storage).map((id) => readOne(id))
      const { values, errors } = partition(results)
      return { installed: values, broken: errors }
    },

    get: readOne,

    install(archive, installOptions) {
      const parsed = readSkinPackage(archive)
      if (!parsed.ok) return err(parsed.error)

      const id = parsed.value.document.id

      if (reserved.has(id)) {
        // Una skin di serie non si sovrascrive: il selettore mostrerebbe un nome
        // familiare con un aspetto diverso, e non ci sarebbe modo di tornare.
        return err(AppError.of('skin.builtinReadOnly', { id }))
      }

      if (storage.exists(fileNameFor(id)) && installOptions?.overwrite !== true) {
        // La sovrascrittura si chiede: chi importa può non sapere di avere già una
        // skin con quell'id, e perderla senza un avviso è una perdita di lavoro.
        return err(AppError.of('skin.idConflict', { id }))
      }

      return writePackage(parsed.value.source, parsed.value.preview ?? undefined, id)
    },

    save(source, saveOptions) {
      // Si valida prima di toccare il disco: una bozza invalida non deve lasciare
      // un file a metà, e lo Studio salva spesso.
      const validated = parseSkin(source)
      if (!validated.ok) return err(validated.error)

      const id = validated.value.id
      if (reserved.has(id)) return err(AppError.of('skin.builtinReadOnly', { id }))

      return writePackage(source, saveOptions?.preview, id)
    },

    remove(id) {
      if (reserved.has(id)) return err(AppError.of('skin.builtinReadOnly', { id }))
      const name = fileNameFor(id)
      if (!storage.exists(name)) return err(AppError.of('skin.notFound', { id }))
      try {
        storage.remove(name)
      } catch (cause) {
        return err(AppError.from(cause).withContext({ skinId: id }))
      }
      return ok(true)
    },

    export(id) {
      const name = fileNameFor(id)
      if (!storage.exists(name)) return err(AppError.of('skin.notFound', { id }))
      try {
        // Si rilegge il pacchetto dal disco invece di ricostruirlo: ciò che parte
        // è esattamente ciò che è installato, comprese le miniature e gli asset che
        // questo codice non ha bisogno di conoscere.
        return ok(storage.read(name))
      } catch (cause) {
        return err(AppError.from(cause).withContext({ skinId: id }))
      }
    },

    entries() {
      const { installed } = this.list()
      return installed.map((skin) => skin.entry)
    }
  }
}

/**
 * Uno storage in memoria.
 *
 * Sta nel codice di produzione e non in un file di test perché serve a più suite e
 * perché è anche la prova che `SkinStorage` è implementabile da qualcosa che non è
 * un filesystem — che è la condizione per far girare la stessa libreria attraverso
 * lo Storage Access Framework su Android.
 */
export function createMemoryStorage(
  initial: Readonly<Record<string, Uint8Array>> = {}
): SkinStorage & { failWrites?: boolean } {
  const files = new Map<string, Uint8Array>(Object.entries(initial))
  const storage = {
    failWrites: false,
    list: () => [...files.keys()],
    read: (name: string) => {
      const bytes = files.get(name)
      if (bytes === undefined) {
        throw Object.assign(new Error(`ENOENT: ${name}`), { code: 'ENOENT', path: name })
      }
      return bytes
    },
    write: (name: string, bytes: Uint8Array) => {
      if (storage.failWrites) {
        throw Object.assign(new Error('ENOSPC: no space left on device'), {
          code: 'ENOSPC',
          path: name
        })
      }
      files.set(name, bytes)
    },
    remove: (name: string) => {
      files.delete(name)
    },
    exists: (name: string) => files.has(name)
  }
  return storage
}
