import { describe, expect, it } from 'vitest'
import {
  NOTHING_SKIN_SOURCE,
  PLAIN_SKIN_SOURCE,
  parseColor,
  tokensInGroup
} from '@aether/skin'
import { AA_NORMAL, auditContrast, levelFor, minimumAlphaFor } from './contrast'
import { createDraft, forkSkin, panelFor, type SkinSource } from './draft'

function plainSource(): SkinSource {
  return JSON.parse(JSON.stringify(PLAIN_SKIN_SOURCE)) as SkinSource
}

function documentOf(source: SkinSource) {
  const draft = createDraft(source)
  if (draft.state.document === null) {
    throw new Error(String(draft.state.error?.params['detail']))
  }
  return draft.state.document
}

describe('contrasto', () => {
  it('classifica secondo le soglie WCAG', () => {
    expect(levelFor(8)).toBe('AAA')
    expect(levelFor(5)).toBe('AA')
    expect(levelFor(3.2)).toBe('AA-large')
    expect(levelFor(2)).toBe('fail')
  })

  it('coglie il problema che sul mobile è stato scoperto sul dispositivo', () => {
    // --color-text-3 era rgba(255,255,255,0.38): su un telefono non si leggeva, e
    // la correzione a 0.5 è arrivata dopo aver guardato uno schermo.
    const report = auditContrast(documentOf(plainSource()))
    const text3 = report.pairs.find(
      (pair) => pair.foreground === 'color.text.3' && pair.background === 'color.surface.0'
    )
    expect(text3).toBeDefined()
    // 'AA-large' e non 'fail': quel bianco al 38% basta per un titolo grande e NON
    // per il testo del corpo, che è esattamente il modo in cui il problema si
    // manifestava — leggibile abbastanza da passare inosservato in sviluppo.
    expect(text3?.level).toBe('AA-large')
    expect(text3?.ratio).toBeLessThan(AA_NORMAL)
    // E compare fra i fallimenti, non sepolto in un elenco.
    expect(report.failures.some((pair) => pair.foreground === 'color.text.3')).toBe(true)
  })

  it('il testo primario passa, come deve', () => {
    const report = auditContrast(documentOf(plainSource()))
    const text1 = report.pairs.find(
      (pair) => pair.foreground === 'color.text.1' && pair.background === 'color.surface.0'
    )
    expect(text1?.level).toBe('AAA')
  })

  it('suggerisce l\'opacità minima che risolve', () => {
    // È il suggerimento concreto: sul mobile trovarlo ha richiesto tentativi su un
    // dispositivo, qui è un calcolo.
    const white = parseColor('rgba(255,255,255,1)')
    const surface = parseColor('#09090d')
    if (!white || !surface) throw new Error('colori di prova non validi')

    const alpha = minimumAlphaFor(white, surface)
    expect(alpha).not.toBeNull()
    if (alpha === null) return
    expect(alpha).toBeGreaterThan(0.38)
    expect(alpha).toBeLessThan(1)
    // Alla soglia trovata il contrasto passa davvero.
    const report = auditContrast(
      documentOf({
        ...plainSource(),
        tokens: {
          ...(plainSource()['tokens'] as SkinSource),
          'color.text.3': `rgba(255, 255, 255, ${alpha})`
        }
      })
    )
    const fixed = report.pairs.find(
      (pair) => pair.foreground === 'color.text.3' && pair.background === 'color.surface.0'
    )
    expect(fixed?.ratio).toBeGreaterThanOrEqual(AA_NORMAL)
  })

  it('restituisce null quando nemmeno l\'opacità piena basta', () => {
    // In quel caso il problema è il colore, non la sua trasparenza, e dirlo è più
    // utile che suggerire un'opacità che non esiste.
    const grey = parseColor('#4a4a4a')
    const surface = parseColor('#3f3f3f')
    if (!grey || !surface) throw new Error('colori di prova non validi')
    expect(minimumAlphaFor(grey, surface)).toBeNull()
  })

  it('verifica anche il tema chiaro, che è quello meno provato', () => {
    const report = auditContrast(documentOf(plainSource()), 'light')
    // Il tema chiaro di plain sovrascrive solo i semantici: le superfici restano
    // scure, quindi le coppie vanno valutate sul documento FUSO e non solo sulle
    // sovrascritture.
    expect(report.pairs.length).toBeGreaterThan(0)
  })

  it('non finge di poter valutare un colore che segue la copertina', () => {
    // Approssimarlo con un valore plausibile darebbe un verde falso.
    const report = auditContrast(
      documentOf({
        ...plainSource(),
        tokens: {
          ...(plainSource()['tokens'] as SkinSource),
          'color.accent': { $source: 'albumArt.vibrant' }
        }
      })
    )
    expect(report.unverifiable.some((entry) => entry.foreground === 'color.accent')).toBe(true)
    expect(report.unverifiable[0]?.reason).toContain('copertina')
  })

  it('segue i riferimenti fra token e la tavolozza locale', () => {
    // nothing usa --accent-like -> rosso, e cyberpunk la tavolozza: senza seguirli
    // il report sarebbe pieno di "non verificabile" per colori che sono noti.
    const report = auditContrast(documentOf(JSON.parse(JSON.stringify(NOTHING_SKIN_SOURCE))))
    const accent = report.pairs.find((pair) => pair.foreground === 'color.accent')
    // Bianco su nero: il massimo possibile.
    expect(accent?.level).toBe('AAA')
  })
})

describe('bozza', () => {
  it('parte pulita e diventa sporca alla prima modifica', () => {
    const draft = createDraft(plainSource())
    expect(draft.state.dirty).toBe(false)
    expect(draft.setToken('color.accent', '#ff0000').state.dirty).toBe(true)
  })

  it('modificare un token ricompila e cambia il CSS', () => {
    const draft = createDraft(plainSource()).setToken('color.accent', '#ff0000')
    expect(draft.state.error).toBeNull()
    expect(draft.state.css).toContain('--accent: rgb(255 0 0);')
    // E la tripla derivata segue, senza che l'editor debba saperlo.
    expect(draft.state.css).toContain('--accent-rgb: 255 0 0;')
  })

  it('un valore invalido non rompe l\'editor: diventa uno stato con un messaggio', () => {
    // Una sorgente a metà modifica è invalida per la maggior parte del tempo —
    // l'utente sta scrivendo — e un editor che si rompe mentre si scrive è
    // inutilizzabile.
    // `#ff00zz` e non `#ff00`: quest'ultimo è un esadecimale a quattro cifre
    // valido (rgba abbreviato), e sceglierlo come esempio di valore invalido
    // avrebbe provato il contrario di ciò che il test dice.
    const draft = createDraft(plainSource()).setToken('color.accent', '#ff00zz')
    expect(draft.state.document).toBeNull()
    expect(draft.state.error?.code).toBe('skin.manifestInvalid')
    expect(String(draft.state.error?.params['detail'])).toContain('color.accent')
    // Il CSS resta vuoto: l'anteprima terrà quello precedente invece di svuotarsi.
    expect(draft.state.css).toBe('')
  })

  it('rimuovere un token lo riporta a ereditato, non a vuoto', () => {
    // Lo schema è strict(): accetta la chiave assente, non la chiave indefinita.
    // E «non l'ho scelto» deve restare distinguibile da «l'ho scelto vuoto».
    const draft = createDraft(plainSource()).setToken('color.accent.soft', undefined)
    expect(draft.state.error).toBeNull()
    expect((draft.state.source['tokens'] as SkinSource)['color.accent.soft']).toBeUndefined()
    expect('color.accent.soft' in (draft.state.source['tokens'] as SkinSource)).toBe(false)
  })

  it('rimuovere l\'ultimo token del tema chiaro fa sparire il blocco', () => {
    // Un `themes: {}` vuoto è rumore in un file che qualcuno leggerà.
    let draft = createDraft(plainSource())
    const lightKeys = Object.keys(
      ((draft.state.source['themes'] as SkinSource)['light'] as SkinSource)
    )
    for (const key of lightKeys) {
      draft = draft.setThemeToken(key as never, undefined)
    }
    expect(draft.state.source['themes']).toBeUndefined()
    expect(draft.state.error).toBeNull()
  })

  it('gli avvisi arrivano assieme al CSS, non su richiesta', () => {
    const draft = createDraft(plainSource()).setToken('radius.card', undefined)
    expect(draft.state.error).toBeNull()
    expect(draft.state.warnings.some((warning) => warning.path === 'tokens.radius.card')).toBe(true)
  })

  it('reset riporta al punto di partenza', () => {
    const draft = createDraft(plainSource())
      .setToken('color.accent', '#ff0000')
      .setMeta('name', 'Altro nome')
    expect(draft.state.dirty).toBe(true)

    const back = draft.reset()
    expect(back.state.dirty).toBe(false)
    expect(back.state.css).toBe(createDraft(plainSource()).state.css)
  })

  it('la bozza è immutabile: ogni modifica è una bozza nuova', () => {
    // Serve all'annulla e alla React: uno stato condiviso mutato in posto
    // renderebbe impossibile confrontare il prima e il dopo.
    const first = createDraft(plainSource())
    const second = first.setToken('color.accent', '#ff0000')
    expect(first.state.css).toContain('--accent: rgb(139 124 246);')
    expect(second.state.css).toContain('--accent: rgb(255 0 0);')
  })

  it('riporta il contrasto a ogni modifica, così il problema si vede subito', () => {
    const draft = createDraft(plainSource()).setToken(
      'color.text.3',
      'rgba(255, 255, 255, 0.5)'
    )
    const text3 = draft.state.contrast?.pairs.find(
      (pair) => pair.foreground === 'color.text.3' && pair.background === 'color.surface.0'
    )
    expect(text3?.level).not.toBe('fail')
  })

  it('riporta il costo, per il budget prestazionale', () => {
    const plain = createDraft(plainSource())
    expect(plain.state.cost).toBe(0)

    const heavy = plain.setToken('color.accent', '#ff0000')
    expect(heavy.state.cost).toBe(0)
  })
})

describe('fork', () => {
  it('deriva una skin da una di serie', () => {
    // Possibile solo perché le skin sono dati: nel legacy «parti da cyberpunk e
    // cambia due colori» significava copiare 1.879 righe di CSS a mano.
    const draft = forkSkin(plainSource(), 'notturno', 'Notturno')
    expect(draft.state.error).toBeNull()
    expect(draft.state.document?.id).toBe('notturno')
    expect(draft.state.document?.meta.name).toBe('Notturno')
    // La versione ricomincia: è una skin nuova, non un aggiornamento.
    expect(draft.state.document?.meta.version).toBe('1.0.0')
    // E registra la provenienza, che serve all'allineamento fra dispositivi.
    expect(draft.state.document?.meta.basedOn).toBe('plain')
  })

  it('il CSS del fork è scopato sul nuovo id', () => {
    const draft = forkSkin(plainSource(), 'notturno', 'Notturno')
    expect(draft.state.css).toContain(":root[data-skin='notturno']")
    expect(draft.state.css).not.toContain("data-skin='plain'")
  })

  it('un id non valido viene rifiutato con un messaggio, non ignorato', () => {
    const draft = forkSkin(plainSource(), 'Notturno Bello', 'x')
    expect(draft.state.document).toBeNull()
    expect(draft.state.error?.code).toBe('skin.manifestInvalid')
  })
})

describe('pannelli generati dal registro', () => {
  it('un pannello si costruisce dai token del gruppo, non a mano', () => {
    // Aggiungere un token al registro lo rende automaticamente editabile: è metà
    // del motivo per cui il registro esiste.
    const draft = createDraft(plainSource())
    const panel = panelFor(draft.state, tokensInGroup('surface'))

    expect(panel.length).toBeGreaterThanOrEqual(4)
    expect(panel[0]?.id).toBe('color.surface.0')
    expect(panel[0]?.kind).toBe('color')
    expect(panel[0]?.required).toBe(true)
    expect(panel[0]?.inherited).toBe(false)
    expect(panel[0]?.description.length).toBeGreaterThan(10)
  })

  it('distingue un token scelto da uno ereditato', () => {
    const draft = createDraft(plainSource()).setToken('color.accent.soft', undefined)
    const panel = panelFor(draft.state, tokensInGroup('accent'))
    const soft = panel.find((entry) => entry.id === 'color.accent.soft')
    expect(soft?.inherited).toBe(true)
  })
})
