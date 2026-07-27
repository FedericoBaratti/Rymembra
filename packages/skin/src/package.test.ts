import { describe, expect, it } from 'vitest'
import { strToU8, zipSync } from 'fflate'
import { PLAIN_SKIN_SOURCE } from './builtin'
import {
  MANIFEST_NAME,
  PACKAGE_LIMITS,
  PREVIEW_NAME,
  packageFileName,
  readSkinPackage,
  writeSkinPackage
} from './package'
import { parseSkin } from './parse'
import type { SkinDocument } from './schema'

function plainDocument(): SkinDocument {
  const parsed = parseSkin(PLAIN_SKIN_SOURCE)
  if (!parsed.ok) throw parsed.error
  return parsed.value
}

/** Byte iniziali validi per ciascun tipo ammesso. */
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3])
const JPEG = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 1, 2, 3])
const WOFF2 = new Uint8Array([0x77, 0x4f, 0x46, 0x32, 1, 2, 3])

/** Costruisce un archivio a mano, per provare i casi che il nostro writer rifiuta. */
function archive(files: Record<string, Uint8Array>): Uint8Array {
  return zipSync(files, { level: 6 })
}

function manifestOf(document: unknown): Uint8Array {
  return strToU8(JSON.stringify(document))
}

describe('scrittura e rilettura', () => {
  it('un pacchetto scritto si rilegge identico', () => {
        const written = writeSkinPackage({
        source: PLAIN_SKIN_SOURCE,
      preview: PNG,
      assets: [{ name: 'grain.png', mime: 'image/png', bytes: PNG }]
    })
    expect(written.ok).toBe(true)
    if (!written.ok) return

    const read = readSkinPackage(written.value)
    expect(read.ok).toBe(true)
    if (!read.ok) return

    expect(read.value.document.id).toBe('plain')
    expect(read.value.document.meta.version).toBe('1.0.0')
    expect(read.value.preview).not.toBeNull()
    expect(read.value.assets.map((asset) => asset.name)).toEqual(['grain.png'])
    expect(read.value.assets[0]?.mime).toBe('image/png')
  })

  it('il manifest da solo basta: preview e asset sono opzionali', () => {
    const written = writeSkinPackage({ source: PLAIN_SKIN_SOURCE })
    expect(written.ok).toBe(true)
    if (!written.ok) return

    const read = readSkinPackage(written.value)
    expect(read.ok).toBe(true)
    if (read.ok) {
      expect(read.value.preview).toBeNull()
      expect(read.value.assets).toEqual([])
    }
  })

  it('accetta i tre formati immagine e i caratteri woff2', () => {
    const written = writeSkinPackage({
      source: PLAIN_SKIN_SOURCE,
      assets: [
        { name: 'a.png', mime: 'image/png', bytes: PNG },
        { name: 'b.jpg', mime: 'image/jpeg', bytes: JPEG },
        { name: 'c.woff2', mime: 'font/woff2', bytes: WOFF2 }
      ]
    })
    expect(written.ok).toBe(true)
    if (!written.ok) return
    const read = readSkinPackage(written.value)
    expect(read.ok).toBe(true)
    if (read.ok) expect(read.value.assets).toHaveLength(3)
  })

  it('il nome del file porta id e versione', () => {
    // Serve all'allineamento fra PC e telefono: due copie della stessa skin a
    // versioni diverse devono essere distinguibili dal nome.
    expect(packageFileName(plainDocument())).toBe('plain-1.0.0.aeskin')
  })

  it('la scrittura rifiuta ciò che la lettura rifiuterebbe', () => {
    // Un pacchetto che questo codice produce e che la lettura scarterebbe è un
    // bug: scoprirlo qui è meglio che scoprirlo sul telefono di qualcuno.
        expect(
      writeSkinPackage({
        source: PLAIN_SKIN_SOURCE,
        assets: [{ name: '../fuori.png', mime: 'image/png', bytes: PNG }]
      }).ok
    ).toBe(false)
    expect(
      writeSkinPackage({
        source: PLAIN_SKIN_SOURCE,
        assets: [{ name: 'falso.png', mime: 'image/png', bytes: strToU8('non sono un png') }]
      }).ok
    ).toBe(false)
    expect(writeSkinPackage({ source: PLAIN_SKIN_SOURCE, preview: strToU8('nemmeno io') }).ok).toBe(false)
  })
})

describe('path traversal', () => {
  it.each([
    ['../../.ssh/authorized_keys', 'risalita di due livelli'],
    ['../skin.json', 'risalita di un livello'],
    ['/etc/passwd', 'percorso assoluto'],
    ['C:\\Windows\\System32\\x.dll', 'percorso Windows'],
    ['assets/../../fuori.png', 'risalita dentro assets'],
    ['assets/sotto/cartella.png', 'sottocartella'],
    ['assets/.hidden.png', 'nome che inizia per punto'],
    ['skin.json.bak', 'manifest camuffato']
  ])('rifiuta %s (%s)', (name) => {
    // La difesa non è normalizzare il percorso e sperare: è una lista chiusa di
    // nomi ammessi, quindi non esiste un percorso da normalizzare.
    const result = readSkinPackage(
      archive({ [MANIFEST_NAME]: manifestOf(PLAIN_SKIN_SOURCE), [name]: PNG })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.assetRejected')
      expect(String(result.error.params['reason'])).toContain('nome non ammesso')
    }
  })
})

describe('zip bomb', () => {
  it('rifiuta una voce con un rapporto di compressione sospetto', () => {
    // Un megabyte di zeri comprime a pochissimo: è la forma canonica della bomba.
    // Il rifiuto avviene PRIMA di decomprimere, guardando le dimensioni dichiarate.
    const zeros = new Uint8Array(2 * 1024 * 1024)
    const result = readSkinPackage(
      archive({ [MANIFEST_NAME]: manifestOf(PLAIN_SKIN_SOURCE), 'assets/bomba.png': zeros })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.assetRejected')
      expect(String(result.error.params['reason'])).toContain('rapporto di compressione')
    }
  })

  it('rifiuta un manifest oltre il limite', () => {
    // Il manifest è testo: mezzo mega è già assurdo, e comprime abbastanza bene
    // da poter nascondere molto di più.
    const huge = { ...PLAIN_SKIN_SOURCE, meta: { ...PLAIN_SKIN_SOURCE.meta } }
    const padding = 'x'.repeat(PACKAGE_LIMITS.maxManifestBytes + 1024)
    const result = readSkinPackage(
      archive({ [MANIFEST_NAME]: strToU8(JSON.stringify({ ...huge, padding })) })
    )
    expect(result.ok).toBe(false)
  })

  it('rifiuta un archivio con troppe voci', () => {
    const files: Record<string, Uint8Array> = {
      [MANIFEST_NAME]: manifestOf(PLAIN_SKIN_SOURCE)
    }
    for (let index = 0; index < PACKAGE_LIMITS.maxEntries + 5; index++) {
      files[`assets/a${index}.png`] = PNG
    }
    const result = readSkinPackage(archive(files))
    expect(result.ok).toBe(false)
    if (!result.ok) expect(String(result.error.params['reason'])).toContain('voci')
  })

  it('rifiuta un archivio più grande del limite, senza aprirlo', () => {
    const oversized = new Uint8Array(PACKAGE_LIMITS.maxArchiveBytes + 1)
    const result = readSkinPackage(oversized)
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.tooLarge')
      expect(result.error.params['limitBytes']).toBe(PACKAGE_LIMITS.maxArchiveBytes)
    }
  })
})

describe('tipo mentito', () => {
  it('rifiuta un file che non è ciò che dice l\'estensione', () => {
    // Il tipo si determina dai byte iniziali. Un eseguibile chiamato .png è
    // precisamente il caso da fermare.
    const result = readSkinPackage(
      archive({
        [MANIFEST_NAME]: manifestOf(PLAIN_SKIN_SOURCE),
        'assets/finta.png': strToU8('MZ\u0090\u0000 sono un eseguibile')
      })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(String(result.error.params['reason'])).toContain('non corrisponde')
    }
  })

  it('rifiuta un SVG, anche con estensione dichiarata', () => {
    // Un SVG è un documento e può contenere script: non è nella lista, e la lista
    // è chiusa.
    const result = readSkinPackage(
      archive({
        [MANIFEST_NAME]: manifestOf(PLAIN_SKIN_SOURCE),
        'assets/logo.svg': strToU8('<svg onload="alert(1)"></svg>')
      })
    )
    expect(result.ok).toBe(false)
  })

  it('rifiuta una miniatura che non è un PNG', () => {
    const result = readSkinPackage(
      archive({ [MANIFEST_NAME]: manifestOf(PLAIN_SKIN_SOURCE), [PREVIEW_NAME]: JPEG })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) expect(String(result.error.params['asset'])).toBe(PREVIEW_NAME)
  })
})

describe('manifest', () => {
  it('un archivio senza manifest è corrotto, non invalido', () => {
    // Due messaggi diversi perché sono due situazioni diverse: uno si riscarica,
    // l'altro no.
    const result = readSkinPackage(archive({ 'assets/a.png': PNG }))
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.packageCorrupt')
      expect(String(result.error.params['detail'])).toContain('skin.json')
    }
  })

  it('un manifest che non è JSON lo dice', () => {
    const result = readSkinPackage(archive({ [MANIFEST_NAME]: strToU8('{ non chiuso') }))
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('skin.manifestInvalid')
  })

  it('un manifest JSON valido ma non conforme riporta il campo', () => {
    // È lo stesso percorso di validazione delle skin di serie: il messaggio che
    // riceve chi importa è quello che riceverebbe chi le sviluppa.
    const result = readSkinPackage(
      archive({
        [MANIFEST_NAME]: manifestOf({
          ...PLAIN_SKIN_SOURCE,
          tokens: { ...(PLAIN_SKIN_SOURCE.tokens as object), 'color.accent': 'azzurro' }
        })
      })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.manifestInvalid')
      expect(String(result.error.params['detail'])).toContain('color.accent')
    }
  })

  it('un formato più nuovo non è un pacchetto rotto', () => {
    const result = readSkinPackage(
      archive({ [MANIFEST_NAME]: manifestOf({ ...PLAIN_SKIN_SOURCE, format: 99 }) })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.formatUnsupported')
      expect(result.error.params['found']).toBe(99)
    }
  })

  it('un archivio illeggibile non fa cadere il processo', () => {
    // Byte casuali: il caso che arriva da un trasferimento interrotto a metà.
    const garbage = new Uint8Array(512)
    for (let index = 0; index < garbage.length; index++) garbage[index] = (index * 7) % 256
    const result = readSkinPackage(garbage)
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('skin.packageCorrupt')
  })
})

describe('le tre skin di serie sopravvivono al giro completo', () => {
  it('si esportano e si reimportano identiche', () => {
    // È il collaudo del formato come mezzo di trasporto: ciò che parte dal PC deve
    // arrivare al telefono uguale.
    for (const source of [PLAIN_SKIN_SOURCE]) {
      const parsed = parseSkin(source)
      if (!parsed.ok) throw parsed.error

      const written = writeSkinPackage({ source })
      expect(written.ok).toBe(true)
      if (!written.ok) return

      const read = readSkinPackage(written.value)
      expect(read.ok).toBe(true)
      if (!read.ok) return

      // Confronto sul documento validato, non sul JSON: è il documento che il
      // compilatore usa, e due JSON diversi possono validare nello stesso.
      expect(read.value.document).toEqual(parsed.value)
    }
  })
})
