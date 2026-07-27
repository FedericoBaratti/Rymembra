import { describe, expect, it } from 'vitest'
import { compileSkin } from '../compile'
import { checkSkin, parseSkin } from '../parse'
import { CYBERPUNK_SKIN_SOURCE } from './cyberpunk'

function compiled(): { css: string; cost: number } {
  const parsed = parseSkin(CYBERPUNK_SKIN_SOURCE)
  if (!parsed.ok) throw new Error(String(parsed.error.params['detail']))
  const result = compileSkin(parsed.value)
  if (!result.ok) throw result.error
  return { css: result.value.css, cost: result.value.cost }
}

describe('cyberpunk — tavolozza locale', () => {
  const { css } = compiled()

  it('emette ogni colore locale in entrambe le forme', () => {
    // Senza la tripla, `rgb(var(--x) / 0.14)` non funziona — ed è la forma con cui
    // la skin costruisce tutte le sue varianti a bassa opacità del teal.
    expect(css).toContain('--skin-color-teal: rgb(0 240 255);')
    expect(css).toContain('--skin-color-teal-rgb: 0 240 255;')
    expect(css).toContain('--skin-color-red: rgb(255 0 60);')
    expect(css).toContain('--skin-color-red-rgb: 255 0 60;')
  })

  it('un riferimento con opacità usa la tripla, uno senza usa il colore', () => {
    // hairline: teal al 18%. Nel legacy era rgba(0, 240, 255, 0.18) scritto per
    // esteso, uno dei venti punti in cui lo stesso teal era ripetuto a mano.
    expect(css).toContain('--hairline: rgb(var(--skin-color-teal-rgb) / 0.18);')
    expect(css).toContain('--accent-like: var(--skin-color-red);')
  })

  it('il warning È l\'accento, per riferimento e non per copia', () => {
    // Il giallo hazard è la firma della skin: duplicarlo permetterebbe ai due di
    // divergere.
    expect(css).toContain('--warning: var(--accent);')
  })

  it('rifiuta un nome di colore non valido nella tavolozza', () => {
    const result = parseSkin({
      ...CYBERPUNK_SKIN_SOURCE,
      palette: { 'Teal Neon': '#00f0ff' }
    })
    expect(result.ok).toBe(false)
  })
})

describe('cyberpunk — la nebbia derivata', () => {
  it('la tripla di surface-0 è emessa dal compilatore', () => {
    // Nel legacy: `--cyber-fog-rgb: 6 6 8` con accanto «DEVE combaciare con
    // surface-0». Un'invariante affidata a un commento si rompe.
    const { css } = compiled()
    expect(css).toContain('--color-surface-0: rgb(6 6 8);')
    expect(css).toContain('--surface-0-rgb: 6 6 8;')
  })

  it('cambiare surface-0 cambia la nebbia, senza toccare altro', () => {
    // È la prova che la derivazione funziona: la coerenza non dipende più da chi
    // ricorda di aggiornare due valori.
    const parsed = parseSkin({
      ...CYBERPUNK_SKIN_SOURCE,
      tokens: { ...(CYBERPUNK_SKIN_SOURCE.tokens as object), 'color.surface.0': '#101018' }
    })
    if (!parsed.ok) throw new Error(String(parsed.error.params['detail']))
    const result = compileSkin(parsed.value)
    if (!result.ok) throw result.error
    expect(result.value.css).toContain('--surface-0-rgb: 16 16 24;')
  })
})

describe('cyberpunk — fedeltà dei token', () => {
  const { css } = compiled()

  it.each([
    ['--color-surface-0', 'rgb(6 6 8)'],
    ['--color-surface-1', 'rgb(11 11 18)'],
    ['--color-surface-2', 'rgb(18 18 28)'],
    ['--color-surface-3', 'rgb(27 27 40)'],
    ['--color-text-1', 'rgb(240 240 224 / 0.95)'],
    ['--color-text-2', 'rgb(190 235 245 / 0.64)'],
    ['--accent', 'rgb(252 238 10)'],
    ['--accent-rgb', '252 238 10'],
    ['--radius-panel', '4px'],
    ['--radius-card', '3px'],
    ['--dur-1', '120ms'],
    ['--dur-2', '220ms'],
    ['--dur-3', '380ms'],
    ['--viz-glow', '26'],
    ['--scrubber-glow', '12']
  ])('%s vale %s', (property, value) => {
    expect(css).toContain(`${property}: ${value};`)
  })

  it('l\'elevazione è alone al neon più hairline, non ombra morbida', () => {
    // Tre livelli su shadow-2, di cui uno è un alone teal: è così che la skin
    // esprime la profondità senza usare il nero.
    expect(css).toContain(
      '--shadow-2: 0px 0px 0px 1px rgb(var(--skin-color-teal-rgb) / 0.14), 0px 10px 30px rgb(0 0 0 / 0.7), 0px 0px 22px rgb(var(--skin-color-teal-rgb) / 0.08);'
    )
  })

  it('il contratto visualizer porta l\'identità dentro i canvas', () => {
    // Barre teal, anello dei bassi giallo. Nessuna riga di JS conosce la skin.
    expect(css).toContain('--viz-primary: var(--skin-color-teal);')
    expect(css).toContain('--viz-secondary: var(--accent);')
  })

  it('il tema chiaro attenua i neon invece di invertirli', () => {
    const light = css.slice(css.indexOf("[data-theme='light']"))
    expect(light).toContain('--color-surface-0: rgb(238 241 244);')
    // Il #fcee0a non è leggibile su chiaro: diventa un giallo più scuro.
    expect(light).toContain('--accent: rgb(179 151 0);')
    expect(light).toContain('--viz-glow: 14;')
    // E la nebbia segue la superficie chiara, automaticamente.
    expect(light).toContain('--surface-0-rgb: 238 241 244;')
  })

  it('non lascia avvisi', () => {
    const parsed = parseSkin(CYBERPUNK_SKIN_SOURCE)
    if (!parsed.ok) throw parsed.error
    expect(checkSkin(parsed.value)).toEqual([])
  })
})

describe('cyberpunk — gli effetti, il caso peggiore', () => {
  const { css, cost } = compiled()

  it('la griglia al neon: due gradienti da un parametro', () => {
    // Nel legacy erano due righe copiate a mano, e la variante chiara aveva un
    // passo diverso per svista (36px 36px invece di 100% 36px).
    expect(css).toContain('--skin-grid:')
    expect(css).toContain('0 0 / 100% 36px')
    expect(css).toContain('0 0 / 36px 100%')
  })

  it('le scanline CRT sono un livello sovrapposto, non un body::after', () => {
    // Nel legacy il secondo livello era su `body`, cioè fuori dal controllo della
    // skin: nessuna skin poteva spegnerlo senza sapere che esisteva.
    expect(css).toContain(":root[data-skin='cyberpunk'] .ambient-backdrop::after {")
    expect(css).toContain('repeating-linear-gradient(0deg,')
    // Il compilatore mette lui content, position e inset: la skin non può
    // spostare lo pseudo-elemento.
    expect(css).toContain('content: "";')
    expect(css).toContain('pointer-events: none;')
  })

  it('il chamfer è un parametro, non due token da tenere coerenti', () => {
    // --cyber-cut per la misura e --cyber-chamfer per il poligono che la usa.
    const rule = css.slice(css.indexOf('.section-card {'))
    expect(rule).toContain(
      'clip-path: polygon(0 0, calc(100% - 14px) 0, 100% 14px, 100% 100%, 14px 100%, 0 calc(100% - 14px));'
    )
    // E la variante piccola usa lo stesso effetto con un'altra misura.
    expect(css).toContain('calc(100% - 7px)')
  })

  it('le strisce hazard restano dove il canone le mette: gli avvisi', () => {
    const rule = css.slice(css.indexOf('.toast-card::after'))
    expect(rule).toContain('repeating-linear-gradient(-45deg,')
    expect(rule).toContain('opacity: 0.12;')
  })

  it('il microtesto è maiuscole spaziate in teal, dichiarate', () => {
    const rule = css.slice(css.indexOf('.hero-eyebrow {'))
    expect(rule).toContain('text-transform: uppercase;')
    expect(rule).toContain('letter-spacing: 0.22em;')
    expect(rule).toContain('rgb(var(--skin-color-teal-rgb) / 0.48)')
  })

  it('resta dentro il budget nonostante sia la skin più carica', () => {
    // Tutti motivi di tipo `paint`: nessun backdrop-filter, che è la ragione per
    // cui una skin così densa può stare nel budget.
    expect(cost).toBeGreaterThan(0)
    expect(css).not.toContain('backdrop-filter')
    expect(css).not.toContain('blur(')
  })

  it('tutto resta scopato sotto data-skin=cyberpunk', () => {
    for (const selector of css.match(/^[^\s@/].*\{$/gm) ?? []) {
      expect(selector).toContain("data-skin='cyberpunk'")
    }
  })
})

describe('le tre skin insieme', () => {
  it('non producono nessun CSS pericoloso', () => {
    // La proprietà vale per l'insieme, non per una skin alla volta: è l'invariante
    // del formato, e le tre skin reali sono il suo collaudo più severo.
    for (const source of [CYBERPUNK_SKIN_SOURCE]) {
      const parsed = parseSkin(source)
      if (!parsed.ok) throw new Error(String(parsed.error.params['detail']))
      const result = compileSkin(parsed.value)
      if (!result.ok) throw result.error

      const values = (result.value.css.match(/^ {2}[a-z-]+:.*$/gm) ?? []).map((line) =>
        line.slice(line.indexOf(':') + 1, -1).trim()
      )
      for (const value of values) {
        expect(value).not.toContain('url(')
        expect(value).not.toContain('@import')
        expect(value).not.toContain('expression(')
        expect(value).not.toContain('javascript:')
        expect(value).not.toContain('}')
      }
    }
  })
})
