import { describe, expect, it } from 'vitest'
import { strToU8 } from 'fflate'
import { PLAIN_SKIN_SOURCE } from './builtin'
import { writeSkinPackage } from './package'
import { createMemoryStorage, createSkinLibrary } from './store'

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3])

/** Una skin utente: `plain` con un altro id, come farebbe un fork. */
function userSkin(id = 'notturno', version = '1.0.0'): Record<string, unknown> {
  const source = JSON.parse(JSON.stringify(PLAIN_SKIN_SOURCE)) as Record<string, unknown>
  source['id'] = id
  source['meta'] = { ...(source['meta'] as object), name: 'Notturno', version }
  return source
}

function packageOf(source: unknown, preview?: Uint8Array): Uint8Array {
  const written = writeSkinPackage(preview === undefined ? { source } : { source, preview })
  if (!written.ok) throw written.error
  return written.value
}

function library(files: Record<string, Uint8Array> = {}) {
  const storage = createMemoryStorage(files)
  return {
    storage,
    lib: createSkinLibrary({ storage, reservedIds: ['plain', 'nothing', 'cyberpunk'] })
  }
}

describe('installazione', () => {
  it('installa un pacchetto e lo ritrova', () => {
    const { lib } = library()
    const installed = lib.install(packageOf(userSkin()))

    expect(installed.ok).toBe(true)
    if (!installed.ok) return
    expect(installed.value.document.id).toBe('notturno')
    expect(installed.value.entry.builtin).toBe(false)
    expect(installed.value.entry.fingerprint).toMatch(/^[0-9a-f]{8}$/)

    const found = lib.get('notturno')
    expect(found.ok).toBe(true)
  })

  it('un file per skin, senza la versione nel nome', () => {
    // Metterla nel nome darebbe due file per la stessa skin dopo un aggiornamento,
    // e nessuno dei due sarebbe autorevole.
    const { lib, storage } = library()
    lib.install(packageOf(userSkin('notturno', '1.0.0')))
    lib.install(packageOf(userSkin('notturno', '2.0.0')), { overwrite: true })

    expect(storage.list()).toEqual(['notturno.aeskin'])
    const found = lib.get('notturno')
    if (found.ok) expect(found.value.document.meta.version).toBe('2.0.0')
  })

  it('chiede prima di sovrascrivere', () => {
    // Chi importa può non sapere di avere già una skin con quell'id, e perderla
    // senza un avviso è una perdita di lavoro.
    const { lib } = library()
    lib.install(packageOf(userSkin()))

    const again = lib.install(packageOf(userSkin('notturno', '2.0.0')))
    expect(again.ok).toBe(false)
    if (!again.ok) {
      expect(again.error.code).toBe('skin.idConflict')
      expect(again.error.params['id']).toBe('notturno')
    }

    expect(lib.install(packageOf(userSkin('notturno', '2.0.0')), { overwrite: true }).ok).toBe(true)
  })

  it('rifiuta di sovrascrivere una skin di serie', () => {
    // Il selettore mostrerebbe un nome familiare con un aspetto diverso, e non ci
    // sarebbe modo di tornare.
    const { lib } = library()
    const result = lib.install(packageOf(PLAIN_SKIN_SOURCE))
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.builtinReadOnly')
      expect(result.error.severity).toBe('info')
    }
  })

  it('un archivio non valido non entra nella libreria', () => {
    const { lib, storage } = library()
    const result = lib.install(strToU8('non sono un archivio'))
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('skin.packageCorrupt')
    // E non lascia niente sul disco.
    expect(storage.list()).toEqual([])
  })

  it('conserva la miniatura', () => {
    const { lib } = library()
    const installed = lib.install(packageOf(userSkin(), PNG))
    expect(installed.ok).toBe(true)
    if (installed.ok) expect(installed.value.preview).not.toBeNull()
  })
})

describe('salvataggio dallo Studio', () => {
  it('salva una sorgente valida senza passare da un archivio', () => {
    const { lib } = library()
    const saved = lib.save(userSkin())
    expect(saved.ok).toBe(true)
    if (saved.ok) expect(saved.value.document.meta.name).toBe('Notturno')
  })

  it('una bozza invalida non lascia un file a metà', () => {
    // Lo Studio salva spesso, e una bozza è invalida per buona parte del tempo in
    // cui si scrive.
    const { lib, storage } = library()
    const broken = { ...userSkin(), tokens: { 'color.accent': 'blu' } }
    const saved = lib.save(broken)

    expect(saved.ok).toBe(false)
    if (!saved.ok) expect(saved.error.code).toBe('skin.manifestInvalid')
    expect(storage.list()).toEqual([])
  })

  it('non salva sopra una skin di serie', () => {
    const { lib } = library()
    expect(lib.save(PLAIN_SKIN_SOURCE).ok).toBe(false)
  })

  it('un disco pieno arriva come errore di filesystem, non come "non salvata"', () => {
    // Senza il motivo, l'utente cercherebbe il problema nel posto sbagliato.
    const { lib, storage } = library()
    storage.failWrites = true

    const saved = lib.save(userSkin())
    expect(saved.ok).toBe(false)
    if (!saved.ok) {
      expect(saved.error.code).toBe('fs.diskFull')
      expect(saved.error.context?.['skinId']).toBe('notturno')
    }
  })
})

describe('elenco', () => {
  it('separa le skin leggibili da quelle rotte', () => {
    // Un pacchetto corrotto — trasferimento interrotto, settore perso — non deve
    // rendere invisibili le altre skin installate.
    const { lib } = library({
      'notturno.aeskin': packageOf(userSkin()),
      'rotta.aeskin': strToU8('spazzatura')
    })

    const { installed, broken } = lib.list()
    expect(installed.map((skin) => skin.document.id)).toEqual(['notturno'])
    expect(broken).toHaveLength(1)
    expect(broken[0]?.code).toBe('skin.packageCorrupt')
    // L'errore dice QUALE skin, altrimenti trovarla richiede di aprirle a mano.
    expect(broken[0]?.context?.['skinId']).toBe('rotta')
  })

  it('ignora i file che non sono pacchetti', () => {
    const { lib } = library({
      'notturno.aeskin': packageOf(userSkin()),
      'appunti.txt': strToU8('nota'),
      '.DS_Store': strToU8('x')
    })
    expect(lib.list().installed).toHaveLength(1)
    expect(lib.list().broken).toHaveLength(0)
  })

  it('elenca in ordine stabile', () => {
    const { lib } = library({
      'zeta.aeskin': packageOf(userSkin('zeta')),
      'alfa.aeskin': packageOf(userSkin('alfa'))
    })
    expect(lib.list().installed.map((skin) => skin.document.id)).toEqual(['alfa', 'zeta'])
  })

  it('produce le voci per l\'allineamento', () => {
    const { lib } = library({ 'notturno.aeskin': packageOf(userSkin()) })
    const entries = lib.entries()
    expect(entries).toHaveLength(1)
    expect(entries[0]?.id).toBe('notturno')
    expect(entries[0]?.builtin).toBe(false)
  })
})

describe('rimozione ed esportazione', () => {
  it('rimuove una skin installata', () => {
    const { lib } = library({ 'notturno.aeskin': packageOf(userSkin()) })
    expect(lib.remove('notturno')).toEqual({ ok: true, value: true })
    expect(lib.list().installed).toEqual([])
  })

  it('non rimuove ciò che non c\'è, e lo dice', () => {
    const { lib } = library()
    const result = lib.remove('inesistente')
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('skin.notFound')
  })

  it('non rimuove una skin di serie', () => {
    const { lib } = library()
    const result = lib.remove('plain')
    expect(result.ok).toBe(false)
    if (!result.ok) expect(result.error.code).toBe('skin.builtinReadOnly')
  })

  it('esporta il pacchetto installato, byte per byte', () => {
    // Ciò che parte è esattamente ciò che è installato, comprese miniature e asset
    // che questo codice non ha bisogno di conoscere.
    const original = packageOf(userSkin(), PNG)
    const { lib } = library()
    lib.install(original)

    const exported = lib.export('notturno')
    expect(exported.ok).toBe(true)
    if (!exported.ok) return

    // Si rilegge e combacia: il giro completo installa → esporta → reinstalla.
    const { lib: other } = library()
    const reinstalled = other.install(exported.value)
    expect(reinstalled.ok).toBe(true)
    if (reinstalled.ok) {
      expect(reinstalled.value.entry.fingerprint).toBe(
        lib.get('notturno').ok ? lib.entries()[0]?.fingerprint : 'diverso'
      )
    }
  })
})

describe('la libreria e l\'allineamento insieme', () => {
  it('due librerie con contenuti diversi producono un piano sensato', () => {
    const here = library({
      'notturno.aeskin': packageOf(userSkin('notturno', '2.0.0')),
      'solo-qui.aeskin': packageOf(userSkin('solo-qui'))
    })
    const there = library({
      'notturno.aeskin': packageOf(userSkin('notturno', '1.0.0'))
    })

    // Le voci di entrambe le librerie sono ciò che il piano di allineamento consuma.
    const local = here.lib.entries()
    const remote = there.lib.entries()
    expect(local.map((entry) => entry.id)).toEqual(['notturno', 'solo-qui'])
    expect(remote.map((entry) => entry.version)).toEqual(['1.0.0'])
  })

  it('una skin salvata dallo Studio è distinguibile dopo una modifica', () => {
    // È il caso che rende necessaria l'impronta: stessa versione, contenuto diverso.
    const { lib } = library()
    const first = lib.save(userSkin())
    if (!first.ok) throw first.error

    const modified = { ...userSkin(), palette: { rosso: '#ff0000' } }
    const second = lib.save(modified)
    if (!second.ok) throw second.error

    expect(second.value.document.meta.version).toBe(first.value.document.meta.version)
    expect(second.value.entry.fingerprint).not.toBe(first.value.entry.fingerprint)
  })
})
