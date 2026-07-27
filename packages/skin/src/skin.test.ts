import { describe, expect, it } from 'vitest'
import { compileEffect, compileSkin } from './compile'
import { effectSchema, exceedsBudget, stackCost, type Effect } from './effects'
import { checkSkin, parseSkin } from './parse'
import { REQUIRED_TOKEN_IDS, TOKENS, TOKEN_IDS } from './tokens'
import { contrastRatio, parseColor, parseLength, parseDuration } from './values'
import type { SkinDocument } from './schema'

/** Una skin minima ma valida: solo i token obbligatori. */
function minimalSkin(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    format: 1,
    id: 'nocturne',
    meta: { name: 'Nocturne', author: 'Federico', version: '1.0.0' },
    tokens: {
      'font.sans': ['Inter Variable'],
      'color.surface.0': '#07070b',
      'color.surface.1': '#0e0e14',
      'color.surface.2': '#16161f',
      'color.surface.3': '#1e1e2a',
      'color.text.1': 'rgba(255, 255, 255, 0.92)',
      'color.text.2': 'rgba(255, 255, 255, 0.6)',
      'color.text.3': 'rgba(255, 255, 255, 0.38)',
      'color.accent': '#8b7cf6',
      'color.danger': '#e5484d',
      'color.success': '#34d399',
      'color.warning': '#facc15',
      'color.hairline': 'rgba(255, 255, 255, 0.07)',
      'radius.panel': '20px',
      'radius.card': '14px',
      'shadow.1': { layers: [{ x: '0px', y: '2px', blur: '12px', color: 'rgba(0,0,0,0.3)' }] },
      'shadow.2': { layers: [{ x: '0px', y: '8px', blur: '28px', color: 'rgba(0,0,0,0.45)' }] },
      'shadow.3': { layers: [{ x: '0px', y: '16px', blur: '56px', color: 'rgba(0,0,0,0.55)' }] },
      'motion.ease.outExpo': { kind: 'cubicBezier', points: [0.16, 1, 0.3, 1] },
      'motion.dur.1': '150ms',
      'motion.dur.2': '280ms'
    },
    ...overrides
  }
}

function parsed(input: Record<string, unknown> = minimalSkin()): SkinDocument {
  const result = parseSkin(input)
  if (!result.ok) throw result.error
  return result.value
}

function compiled(input: Record<string, unknown> = minimalSkin()): string {
  const result = compileSkin(parsed(input))
  if (!result.ok) throw result.error
  return result.value.css
}

describe('valori', () => {
  it.each([
    ['#fff', { r: 255, g: 255, b: 255, a: 1 }],
    ['#8b7cf6', { r: 139, g: 124, b: 246, a: 1 }],
    ['rgba(255, 255, 255, 0.6)', { r: 255, g: 255, b: 255, a: 0.6 }],
    ['rgb(0 240 255)', { r: 0, g: 240, b: 255, a: 1 }]
  ])('parsa %s nei suoi canali', (input, expected) => {
    expect(parseColor(input)).toEqual(expected)
  })

  it.each([
    ['red', 'nome di colore CSS'],
    ['currentColor', 'parola chiave'],
    ['var(--accent)', 'riferimento CSS grezzo'],
    ['color-mix(in srgb, red, blue)', 'funzione moderna'],
    ['#12345', 'esadecimale di lunghezza sbagliata'],
    ['rgb(1,2)', 'canali insufficienti'],
    ['url(https://evil.example/x.png)', 'url']
  ])('rifiuta %s (%s)', (input) => {
    // La lista chiusa è il punto: se non si sa cos'è, non entra. Da qui viene la
    // garanzia che il compilatore non copi mai testo nell'output.
    expect(parseColor(input)).toBeNull()
  })

  it.each([
    ['14px', { value: 14, unit: 'px' }],
    ['1.5rem', { value: 1.5, unit: 'rem' }],
    ['-4px', { value: -4, unit: 'px' }],
    ['100%', { value: 100, unit: '%' }]
  ])('parsa la lunghezza %s', (input, expected) => {
    expect(parseLength(input)).toEqual(expected)
  })

  it.each([
    ['calc(100% - 14px)', 'calc'],
    ['14', 'senza unità'],
    ['14pt', 'unità fuori lista'],
    ['14px; color: red', 'iniezione'],
    ['clamp(16px, 3cqw, 48px)', 'funzione']
  ])('rifiuta la lunghezza %s (%s)', (input) => {
    expect(parseLength(input)).toBeNull()
  })

  it('rifiuta una durata oltre il tetto', () => {
    expect(parseDuration('280ms')).toEqual({ ms: 280 })
    expect(parseDuration('0.4s')).toEqual({ ms: 400 })
    // Un\'animazione di trenta secondi non è uno stile, è un blocco.
    expect(parseDuration('30s')).toBeNull()
  })

  it('calcola il contrasto appiattendo il testo semitrasparente sulla superficie', () => {
    // È il controllo che sull\'albero mobile è stato scoperto tardi: --color-text-3
    // era rgba(255,255,255,0.38) e ha dovuto salire a 0.5 per essere leggibile.
    const surface = parseColor('#09090d')
    const text3 = parseColor('rgba(255,255,255,0.38)')
    const text3Fixed = parseColor('rgba(255,255,255,0.5)')
    if (!surface || !text3 || !text3Fixed) throw new Error('colori di prova non validi')

    const before = contrastRatio(text3, surface)
    const after = contrastRatio(text3Fixed, surface)
    expect(after).toBeGreaterThan(before)
    // 4.5 è la soglia AA per il testo normale: il valore originale non la passa.
    expect(before).toBeLessThan(4.5)
  })
})

describe('registro dei token', () => {
  it('ogni token ha un nome CSS unico', () => {
    const names = TOKEN_IDS.map((id) => TOKENS[id].css)
    expect(new Set(names).size).toBe(names.length)
  })

  it('ogni nome CSS è una proprietà personalizzata valida', () => {
    for (const id of TOKEN_IDS) {
      expect(TOKENS[id].css).toMatch(/^--[a-z][a-z0-9-]*$/)
    }
  })

  it('ogni token ha una descrizione utile allo Studio', () => {
    for (const id of TOKEN_IDS) {
      expect(TOKENS[id].description.length).toBeGreaterThan(10)
    }
  })

  it('i token obbligatori coprono ciò che rende una skin riconoscibile', () => {
    // Superfici, testo, accento, geometria e motion: senza uno di questi la skin
    // non è una skin, è una variazione.
    for (const id of [
      'color.surface.0',
      'color.text.1',
      'color.accent',
      'radius.panel',
      'motion.dur.1'
    ] as const) {
      expect(REQUIRED_TOKEN_IDS).toContain(id)
    }
  })
})

describe('validazione', () => {
  it('accetta una skin minima valida', () => {
    const result = parseSkin(minimalSkin())
    expect(result.ok).toBe(true)
  })

  it('rifiuta un token che non esiste nel registro', () => {
    // strict(): un token scritto male è un errore, non silenzio. Nel legacy era
    // CSS, quindi il browser lo ignorava e la skin restava rotta in un punto.
    const result = parseSkin(
      minimalSkin({ tokens: { ...(minimalSkin().tokens as object), 'color.surfaec.0': '#000' } })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.manifestInvalid')
      expect(String(result.error.params['detail'])).toContain('surfaec')
    }
  })

  it('nomina il token e il motivo quando un valore è sbagliato', () => {
    const result = parseSkin(
      minimalSkin({ tokens: { ...(minimalSkin().tokens as object), 'color.accent': 'blu' } })
    )
    expect(result.ok).toBe(false)
    if (!result.ok) {
      const detail = String(result.error.params['detail'])
      expect(detail).toContain('color.accent')
      expect(detail).toContain('colore non valido')
    }
  })

  it('distingue un formato più nuovo da un pacchetto malformato', () => {
    // Sono due cose diverse e vogliono due messaggi diversi — la stessa distinzione
    // che nel database separa una migrazione fallita da un file di una build futura.
    const result = parseSkin({ ...minimalSkin(), format: 2 })
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(result.error.code).toBe('skin.formatUnsupported')
      expect(result.error.params['found']).toBe(2)
    }
  })

  it.each([
    ['Nocturne', 'maiuscole'],
    ['noc turne', 'spazi'],
    ["noc'turne", 'apice'],
    ['../etc/passwd', 'traversal'],
    ['a', 'troppo corto']
  ])('rifiuta l\'id %s (%s)', (id) => {
    // L'id finisce in un selettore CSS, in un nome di file e in un URL della LAN:
    // uno con una virgoletta o una barra ne romperebbe uno dei tre.
    expect(parseSkin({ ...minimalSkin(), id }).ok).toBe(false)
  })

  it.each([
    ["Inter'; }", 'apice e graffa'],
    ['Inter"', 'virgoletta'],
    ['url(x)', 'funzione']
  ])('rifiuta il carattere %s (%s)', (family) => {
    const result = parseSkin(
      minimalSkin({ tokens: { ...(minimalSkin().tokens as object), 'font.sans': [family] } })
    )
    expect(result.ok).toBe(false)
  })

  it('avvisa quando manca un token obbligatorio, senza rifiutare', () => {
    // Non è un errore — erediterà il valore di base — ma quasi sempre è una
    // dimenticanza, e nel legacy costava settimane di silenzio.
    const tokens = { ...(minimalSkin().tokens as Record<string, unknown>) }
    delete tokens['radius.card']
    const warnings = checkSkin(parsed(minimalSkin({ tokens })))
    expect(warnings.some((w) => w.path === 'tokens.radius.card')).toBe(true)
  })

  it('avvisa se dichiara un tema chiaro e non lo definisce', () => {
    const warnings = checkSkin(
      parsed(minimalSkin({ capabilities: { light: true, mobile: false, dynamicAccent: true } }))
    )
    expect(warnings.some((w) => w.path === 'themes.light')).toBe(true)
  })
})

describe('compilatore — sicurezza dell\'output', () => {
  const dangerous = [
    ['url(', 'nessuna richiesta di rete può partire da una skin'],
    ['@import', 'nessun foglio esterno'],
    ['expression(', 'nessuna espressione'],
    ['javascript:', 'nessuno schema di script'],
    ['<', 'nessun markup'],
    ['}', 'nessuna chiusura di blocco dentro un valore'],
    [';', 'nessuna dichiarazione in più dentro un valore']
  ] as const

  /** Solo i VALORI delle dichiarazioni: è lì che un'iniezione dovrebbe finire. */
  function declarationValues(css: string): string[] {
    return (css.match(/^ {2}--[a-z0-9-]+:.*$/gm) ?? []).map((line) =>
      line.slice(line.indexOf(':') + 1, -1).trim()
    )
  }

  it.each(dangerous)('nessun valore compilato contiene %s (%s)', (needle) => {
    // Property test sull'output, non sull'intenzione: si prova con una skin che
    // tenta di infilare quelle stringhe in ogni campo che accetta testo.
    const attempt = minimalSkin({
      meta: {
        name: 'url(https://evil.example/x) @import "y" <script>',
        author: '}\n:root { color: red } /*',
        version: '1.0.0',
        description: 'javascript:alert(1) expression(alert(1))'
      },
      patterns: {
        grid: { effect: 'hairlineGrid', color: 'rgba(0,240,255,0.07)', cell: '36px' },
        cut: { effect: 'chamfer', size: '14px' }
      }
    })
    for (const value of declarationValues(compiled(attempt))) {
      expect(value).not.toContain(needle)
    }
  })

  it('i metadati non finiscono in nessuna dichiarazione', () => {
    // Il nome e l'autore compaiono solo nel commento di testa, che non è un
    // contesto eseguibile — e i caratteri che chiuderebbero il commento non
    // sopravvivono alla validazione del nome.
    const css = compiled()
    for (const value of declarationValues(css)) {
      expect(value).not.toContain('Nocturne')
      expect(value).not.toContain('Federico')
    }
  })

  it('un valore di colore non può chiudere la dichiarazione', () => {
    // Se fosse copiato tale e quale, `#000; } :root { display: none` spegnerebbe
    // l'interfaccia. Viene rifiutato in validazione, quindi non arriva mai qui.
    const result = parseSkin(
      minimalSkin({
        tokens: {
          ...(minimalSkin().tokens as object),
          'color.accent': '#000; } :root { display: none'
        }
      })
    )
    expect(result.ok).toBe(false)
  })

  it('produce un output deterministico', () => {
    // L'ordine segue il registro, non l'ordine di scrittura nel documento: è ciò
    // che rende utili gli snapshot e la cache dell'anteprima dal vivo.
    const shuffled = minimalSkin({
      tokens: Object.fromEntries(
        Object.entries(minimalSkin().tokens as Record<string, unknown>).reverse()
      )
    })
    expect(compiled(shuffled)).toBe(compiled())
  })
})

describe('compilatore — token', () => {
  it('scopa tutto sotto il selettore della skin', () => {
    const css = compiled()
    expect(css).toContain(":root[data-skin='nocturne'] {")
    // Nessuna regola fuori dallo scope: una skin non può toccare altre skin.
    const selectors = css.match(/^[^\s@/].*\{$/gm) ?? []
    for (const selector of selectors) {
      expect(selector).toContain("data-skin='nocturne'")
    }
  })

  it('usa i nomi CSS del legacy, non nomi nuovi', () => {
    // I componenti leggono `var(--color-surface-0)`: cambiare il nome
    // significherebbe riscrivere i componenti e rompere le skin esistenti.
    const css = compiled()
    for (const property of [
      '--color-surface-0',
      '--color-text-1',
      '--accent',
      '--radius-panel',
      '--shadow-2',
      '--dur-1',
      '--ease-out-expo',
      '--hairline'
    ]) {
      expect(css).toContain(`${property}:`)
    }
  })

  it('deriva la tripla rgb dal colore, così non può divergere', () => {
    // Nel legacy `--accent` e `--accent-rgb` erano due dichiarazioni da tenere
    // allineate a mano, e `--cyber-fog-rgb` aveva il commento «DEVE combaciare
    // con surface-0».
    const css = compiled()
    expect(css).toContain('--accent: rgb(139 124 246)')
    expect(css).toContain('--accent-rgb: 139 124 246')
  })

  it('emette i token calcolati e non li fa sovrascrivere', () => {
    // --player-clearance è l'altezza del player più due volte il margine: una
    // conseguenza, non una scelta. Nel legacy stava in :root con tutto il resto,
    // quindi una skin poteva contraddirla e sfasare la shell.
    const css = compiled(
      minimalSkin({
        tokens: {
          ...(minimalSkin().tokens as object),
          'layout.playerHeight': '92px',
          'layout.playerGap': '14px'
        }
      })
    )
    expect(css).toContain('--player-clearance: calc(var(--player-h) + var(--player-gap) * 2)')
  })

  it('un riferimento fra token diventa var(), col nome preso dal registro', () => {
    const css = compiled(
      minimalSkin({
        tokens: { ...(minimalSkin().tokens as object), 'color.accent.like': { $token: 'color.danger' } }
      })
    )
    expect(css).toContain('--accent-like: var(--danger)')
  })

  it('rifiuta un riferimento a un token inesistente', () => {
    const result = parseSkin(
      minimalSkin({
        tokens: { ...(minimalSkin().tokens as object), 'color.accent.like': { $token: 'color.inventato' } }
      })
    )
    expect(result.ok).toBe(false)
  })

  it('un\'ombra a zero livelli diventa none — così le skin piatte spengono', () => {
    const css = compiled(
      minimalSkin({ tokens: { ...(minimalSkin().tokens as object), 'shadow.2': { layers: [] } } })
    )
    expect(css).toContain('--shadow-2: none')
  })

  it('compila un\'ombra a più livelli, inset compreso', () => {
    const css = compiled(
      minimalSkin({
        tokens: {
          ...(minimalSkin().tokens as object),
          'shadow.player': {
            layers: [
              { inset: true, x: '0px', y: '1px', blur: '0px', color: 'rgba(255,255,255,0.06)' },
              { x: '0px', y: '8px', blur: '40px', color: 'rgba(0,0,0,0.5)' }
            ]
          }
        }
      })
    )
    expect(css).toContain(
      '--shadow-player: inset 0px 1px 0px rgb(255 255 255 / 0.06), 0px 8px 40px rgb(0 0 0 / 0.5)'
    )
  })

  it('compila il tema chiaro e le sovrascritture mobile su selettori distinti', () => {
    const css = compiled(
      minimalSkin({
        capabilities: { light: true, mobile: true, dynamicAccent: true },
        themes: { light: { 'color.surface.0': '#eef1f4' } },
        platforms: { mobile: { 'color.text.3': 'rgba(255,255,255,0.5)' } }
      })
    )
    expect(css).toContain(":root[data-skin='nocturne'][data-theme='light'] {")
    expect(css).toContain(":root[data-skin='nocturne'][data-mobile] {")
    // La correzione scoperta sul dispositivo diventa una sovrascrittura dichiarata.
    expect(css).toContain('--color-text-3: rgb(255 255 255 / 0.5)')
  })

  it('segnala quali token seguono la copertina', () => {
    const result = compileSkin(
      parsed(
        minimalSkin({
          tokens: {
            ...(minimalSkin().tokens as object),
            'color.accent': { $source: 'albumArt.vibrant' },
            'color.accent.soft': { $source: 'albumArt.vibrant', alpha: 0.16 }
          }
        })
      )
    )
    expect(result.ok).toBe(true)
    if (result.ok) {
      // Sostituisce il booleano supportsDynamicAccent, che accendeva o spegneva il
      // meccanismo in blocco: ora si sa QUALI token seguono la copertina.
      expect(result.value.dynamicTokens).toContain('color.accent')
      expect(result.value.dynamicTokens).toContain('color.accent.soft')
      expect(result.value.css).toContain('--accent-soft: rgb(var(--accent-rgb) / 0.16)')
    }
  })
})

describe('effetti', () => {
  function effect(input: Record<string, unknown>): Effect {
    const result = effectSchema.safeParse(input)
    if (!result.success) throw new Error(result.error.message)
    return result.data
  }

  it('la griglia sottile scrive una volta ciò che il legacy scriveva due', () => {
    // --cyber-grid era due linear-gradient con lo stesso colore e lo stesso passo,
    // copiati a mano — e la variante chiara aveva perfino un passo diverso per
    // svista (36px 36px invece di 100% 36px).
    const css = compileEffect(
      effect({ effect: 'hairlineGrid', color: 'rgba(0,240,255,0.07)', cell: '36px' })
    )
    expect(css).toContain('linear-gradient(rgb(0 240 255 / 0.07) 1px, transparent 1px) 0 0 / 100% 36px')
    expect(css).toContain('linear-gradient(90deg, rgb(0 240 255 / 0.07) 1px, transparent 1px) 0 0 / 36px 100%')
  })

  it('le scanline riproducono il repeating-linear-gradient del legacy', () => {
    const css = compileEffect(
      effect({ effect: 'scanlines', color: 'rgba(0,240,255,0.05)', line: '1px', gap: '3px' })
    )
    expect(css).toBe(
      'repeating-linear-gradient(0deg, rgb(0 240 255 / 0.05) 0 1px, transparent 1px 3px)'
    )
  })

  it('le strisce hazard calcolano il secondo stop invece di ripeterlo', () => {
    const css = compileEffect(
      effect({
        effect: 'stripes',
        angle: -45,
        color: 'rgba(252,238,10,0.85)',
        background: 'rgba(5,5,5,0.9)',
        width: '6px'
      })
    )
    expect(css).toContain('-45deg')
    expect(css).toContain('0 6px')
    expect(css).toContain('6px 12px')
  })

  it('il chamfer produce il poligono, e la misura non può divergere dalla forma', () => {
    // Nel legacy erano due token: --cyber-cut per la misura e --cyber-chamfer per
    // il poligono, da tenere coerenti a mano.
    const css = compileEffect({ effect: 'chamfer', size: { value: 14, unit: 'px' }, corners: ['topRight', 'bottomLeft'] })
    expect(css).toBe('polygon(0 0, calc(100% - 14px) 0, 100% 14px, 100% 100%, 14px 100%, 0 calc(100% - 14px))')
  })

  it('ogni effetto dichiara il suo costo', () => {
    expect(stackCost([effect({ effect: 'solid', color: '#000' })])).toBe(1)
    expect(stackCost([effect({ effect: 'blurBehind', radius: '20px' })])).toBe(10)
  })

  it('quattro superfici con blur sforano il budget — la nota di global.css, verificabile', () => {
    // «al massimo ~4 superfici con backdrop-filter composte insieme» era un
    // commento scritto a mano. Ora è un numero che si può controllare.
    const blur = effect({ effect: 'blurBehind', radius: '20px' })
    expect(exceedsBudget([blur])).toBe(false)
    expect(exceedsBudget([blur, blur])).toBe(true)
  })

  it('rifiuta un effetto che non esiste', () => {
    const result = effectSchema.safeParse({ effect: 'parallasseOlografica', color: '#000' })
    expect(result.success).toBe(false)
  })

  it('i motivi diventano proprietà con prefisso, e il suffisso dice dove usarli', () => {
    const css = compiled(
      minimalSkin({
        patterns: {
          grid: { effect: 'hairlineGrid', color: 'rgba(0,240,255,0.07)', cell: '36px' },
          chamfer: { effect: 'chamfer', size: '14px' }
        }
      })
    )
    expect(css).toContain('--skin-grid:')
    // Un clip-path e uno sfondo non si scambiano, e il nome lo rende evidente.
    expect(css).toContain('--skin-chamfer-clip:')
  })
})

describe('motion', () => {
  it('l\'intensità diventa un token, non una sostituzione delle media query', () => {
    // Si compone con prefers-reduced-motion: la media query del sistema viene dopo
    // e vince. I blocchi reduced-motion esistenti restano il pavimento.
    const css = compiled(minimalSkin({ motion: { intensity: 'essential' } }))
    expect(css).toContain('--motion-intensity: essential')
    expect(css).toContain('--motion-scale: 0.5')
  })

  it('le transizioni di rotta animano solo transform e opacity', () => {
    const css = compiled(
      minimalSkin({
        motion: {
          intensity: 'full',
          routeTransition: { out: { opacity: 0, scale: 0.992 }, in: { opacity: 0, translateY: 8 } }
        }
      })
    )
    expect(css).toContain('::view-transition-old(root)')
    expect(css).toContain('@keyframes skin-nocturne-out')
    expect(css).toContain('transform: scale(0.992)')
    expect(css).toContain('transform: translateY(8px)')
    // Nessuna proprietà che costringa a un layout: è il vincolo del compilatore.
    expect(css).not.toContain('width:')
    expect(css).not.toContain('height:')
    expect(css).not.toContain('margin')
  })

  it('rifiuta una traslazione fuori scala', () => {
    const result = parseSkin(
      minimalSkin({ motion: { intensity: 'full', routeTransition: { in: { translateY: 5000 } } } })
    )
    expect(result.ok).toBe(false)
  })
})
