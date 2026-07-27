import { describe, expect, it } from 'vitest'
import { PLAIN_SKIN_SOURCE } from './builtin'
import {
  alignLibraries,
  canonicalJson,
  compareVersions,
  libraryEntryFor,
  skinFingerprint,
  type LibraryEntry
} from './library'
import { parseSkin } from './parse'

function entry(overrides: Partial<LibraryEntry> & { id: string }): LibraryEntry {
  return {
    version: '1.0.0',
    fingerprint: 'aaaaaaaa',
    builtin: false,
    ...overrides
  }
}

describe('JSON canonico', () => {
  it('ordina le chiavi a qualunque profondità', () => {
    // Senza, la stessa skin salvata da due editor darebbe due impronte diverse per
    // il solo ordine delle chiavi, e l'allineamento vedrebbe differenze inesistenti.
    const a = { b: 1, a: { d: 2, c: 3 } }
    const b = { a: { c: 3, d: 2 }, b: 1 }
    expect(canonicalJson(a)).toBe(canonicalJson(b))
  })

  it('NON ordina gli array: qui l\'ordine è significativo', () => {
    // I livelli di un'ombra e le fermate di un gradiente non sono commutativi.
    expect(canonicalJson([1, 2])).not.toBe(canonicalJson([2, 1]))
  })

  it('ignora le chiavi indefinite', () => {
    expect(canonicalJson({ a: 1, b: undefined })).toBe(canonicalJson({ a: 1 }))
  })
})

describe('impronta', () => {
  it('è stabile e indipendente dall\'ordine di scrittura', () => {
    const source = PLAIN_SKIN_SOURCE as unknown as Record<string, unknown>
    const shuffled = Object.fromEntries(Object.entries(source).reverse())
    expect(skinFingerprint(shuffled)).toBe(skinFingerprint(source))
  })

  it('cambia al cambiare di un singolo valore', () => {
    const source = JSON.parse(JSON.stringify(PLAIN_SKIN_SOURCE)) as Record<string, unknown>
    const before = skinFingerprint(source)
    ;(source['tokens'] as Record<string, unknown>)['color.accent'] = '#ff0000'
    expect(skinFingerprint(source)).not.toBe(before)
  })

  it('ha sempre otto cifre esadecimali', () => {
    // Serve al confronto testuale e alla visualizzazione: una lunghezza variabile
    // renderebbe l'allineamento sensibile alla formattazione.
    for (const value of ['', 'a', PLAIN_SKIN_SOURCE]) {
      expect(skinFingerprint(value)).toMatch(/^[0-9a-f]{8}$/)
    }
  })

  it('la voce di libreria si calcola sulla sorgente, non sul documento', () => {
    // Due dispositivi che validano la stessa sorgente con versioni diverse del
    // formato potrebbero produrre documenti diversi da un contenuto identico.
    const parsed = parseSkin(PLAIN_SKIN_SOURCE)
    if (!parsed.ok) throw parsed.error
    const item = libraryEntryFor(parsed.value, PLAIN_SKIN_SOURCE, true)

    expect(item.id).toBe('plain')
    expect(item.version).toBe('1.0.0')
    expect(item.builtin).toBe(true)
    expect(item.fingerprint).toBe(skinFingerprint(PLAIN_SKIN_SOURCE))
  })
})

describe('confronto di versioni', () => {
  it.each([
    ['1.0.0', '1.0.0', 0],
    ['1.0.1', '1.0.0', 1],
    ['1.0.0', '1.0.1', -1],
    ['2.0.0', '1.9.9', 1],
    ['1.2.0', '1.10.0', -1]
  ])('%s vs %s = %i', (a, b, expected) => {
    expect(compareVersions(a, b)).toBe(expected)
  })

  it('non è alfabetico — è l\'errore che sovrascrive il nuovo col vecchio', () => {
    // '1.10.0' < '1.9.0' come stringhe, e su un allineamento significherebbe
    // rimpiazzare la versione più recente con quella più vecchia.
    expect('1.10.0' < '1.9.0').toBe(true)
    expect(compareVersions('1.10.0', '1.9.0')).toBe(1)
  })
})

describe('allineamento', () => {
  it('manda ciò che manca all\'altro lato', () => {
    const plan = alignLibraries([entry({ id: 'mia' })], [])
    expect(plan.items[0]?.action).toBe('send')
    expect(plan.automatic).toHaveLength(1)
    expect(plan.conflicts).toHaveLength(0)
  })

  it('riceve ciò che manca a questo lato', () => {
    const plan = alignLibraries([], [entry({ id: 'tua' })])
    expect(plan.items[0]?.action).toBe('receive')
  })

  it('non fa niente quando le impronte combaciano', () => {
    const same = { id: 'x', fingerprint: 'deadbeef' }
    const plan = alignLibraries([entry(same)], [entry(same)])
    expect(plan.items[0]?.action).toBe('inSync')
    expect(plan.automatic).toHaveLength(0)
  })

  it('manda la versione più recente e riceve quella più recente', () => {
    const newer = alignLibraries(
      [entry({ id: 'x', version: '1.2.0', fingerprint: 'aaaa1111' })],
      [entry({ id: 'x', version: '1.1.0', fingerprint: 'bbbb2222' })]
    )
    expect(newer.items[0]?.action).toBe('sendNewer')
    expect(newer.items[0]?.reason).toContain('1.2.0')

    const older = alignLibraries(
      [entry({ id: 'x', version: '1.1.0', fingerprint: 'aaaa1111' })],
      [entry({ id: 'x', version: '1.2.0', fingerprint: 'bbbb2222' })]
    )
    expect(older.items[0]?.action).toBe('receiveNewer')
  })

  it('stessa versione e contenuto diverso è un CONFLITTO, non una scelta automatica', () => {
    // È il caso che rende il problema non banale, e succede appena qualcuno
    // modifica una skin nello Studio senza alzare la versione — cioè quasi sempre
    // durante il lavoro. Confrontare solo le versioni direbbe «allineate», e la
    // modifica sparirebbe.
    const plan = alignLibraries(
      [entry({ id: 'x', version: '1.0.0', fingerprint: 'aaaa1111' })],
      [entry({ id: 'x', version: '1.0.0', fingerprint: 'bbbb2222' })]
    )
    expect(plan.items[0]?.action).toBe('conflict')
    expect(plan.conflicts).toHaveLength(1)
    // E NON entra fra le azioni automatiche: qualunque scelta butterebbe via il
    // lavoro di uno dei due lati.
    expect(plan.automatic).toHaveLength(0)
    expect(plan.items[0]?.reason).toContain('scelta')
  })

  it('lascia fuori le skin di serie', () => {
    // Esistono su entrambi i lati per costruzione: arrivano dal bundle.
    const plan = alignLibraries(
      [entry({ id: 'plain', builtin: true, fingerprint: 'aaaa1111' })],
      [entry({ id: 'plain', builtin: true, fingerprint: 'bbbb2222' })]
    )
    expect(plan.items[0]?.action).toBe('skipBuiltin')
    expect(plan.automatic).toHaveLength(0)
    expect(plan.conflicts).toHaveLength(0)
  })

  it('una skin di serie su un lato solo resta fuori', () => {
    // Se l'altro dispositivo ha una build più vecchia senza quella built-in,
    // mandarla non serve: la riceverà aggiornando l'app.
    const plan = alignLibraries([entry({ id: 'nothing', builtin: true })], [])
    expect(plan.items[0]?.action).toBe('skipBuiltin')
  })

  it('produce un piano in ordine stabile', () => {
    // Un piano che cambia ordine fra due letture è un piano che l'utente non può
    // ricontrollare prima di premere «allinea tutto».
    const plan = alignLibraries(
      [entry({ id: 'zeta' }), entry({ id: 'alfa' })],
      [entry({ id: 'mezzo' })]
    )
    expect(plan.items.map((item) => item.id)).toEqual(['alfa', 'mezzo', 'zeta'])
  })

  it('separa ciò che si può fare da solo da ciò che va chiesto', () => {
    const plan = alignLibraries(
      [
        entry({ id: 'solo-qui' }),
        entry({ id: 'piu-nuova', version: '2.0.0', fingerprint: 'aaaa1111' }),
        entry({ id: 'contesa', fingerprint: 'aaaa1111' }),
        entry({ id: 'uguale', fingerprint: 'cccc3333' })
      ],
      [
        entry({ id: 'solo-la' }),
        entry({ id: 'piu-nuova', version: '1.0.0', fingerprint: 'bbbb2222' }),
        entry({ id: 'contesa', fingerprint: 'bbbb2222' }),
        entry({ id: 'uguale', fingerprint: 'cccc3333' })
      ]
    )

    expect(plan.automatic.map((item) => item.id).sort()).toEqual([
      'piu-nuova',
      'solo-la',
      'solo-qui'
    ])
    expect(plan.conflicts.map((item) => item.id)).toEqual(['contesa'])
    // 'uguale' non compare in nessuna delle due liste: non c'è niente da fare.
    expect(plan.items.find((item) => item.id === 'uguale')?.action).toBe('inSync')
  })

  it('due librerie vuote danno un piano vuoto', () => {
    const plan = alignLibraries([], [])
    expect(plan.items).toEqual([])
    expect(plan.automatic).toEqual([])
    expect(plan.conflicts).toEqual([])
  })
})
