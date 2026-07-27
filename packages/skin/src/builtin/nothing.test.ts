import { describe, expect, it } from 'vitest'
import { compileSkin } from '../compile'
import { checkSkin, parseSkin } from '../parse'
import { PART_NAMES } from '../parts'
import { NOTHING_SKIN_SOURCE } from './nothing'

function compiled(): { css: string; cost: number } {
  const parsed = parseSkin(NOTHING_SKIN_SOURCE)
  if (!parsed.ok) throw new Error(String(parsed.error.params['detail']))
  const result = compileSkin(parsed.value)
  if (!result.ok) throw result.error
  return { css: result.value.css, cost: result.value.cost }
}

describe('nothing — fedeltà dei token', () => {
  const { css } = compiled()

  it.each([
    ['--color-surface-0', 'rgb(0 0 0)'],
    ['--color-surface-1', 'rgb(10 10 10)'],
    ['--color-surface-2', 'rgb(20 20 20)'],
    ['--color-surface-3', 'rgb(31 31 31)'],
    ['--color-text-1', 'rgb(255 255 255 / 0.92)'],
    ['--color-text-2', 'rgb(255 255 255 / 0.62)'],
    ['--color-text-3', 'rgb(255 255 255 / 0.42)'],
    ['--accent', 'rgb(255 255 255)'],
    ['--accent-rgb', '255 255 255'],
    ['--accent-soft', 'rgb(255 255 255 / 0.1)'],
    ['--sidebar-bg', 'rgb(0 0 0)'],
    ['--hairline', 'rgb(255 255 255 / 0.16)'],
    ['--radius-panel', '12px'],
    ['--radius-card', '4px'],
    ['--dur-1', '90ms'],
    ['--dur-2', '160ms'],
    ['--dur-3', '260ms']
  ])('%s vale %s', (property, value) => {
    expect(css).toContain(`${property}: ${value};`)
  })

  it('il rosso interrupt è l\'unico colore, e vale sia per like sia per errori', () => {
    // Nel legacy era il token skin-locale --nothing-red, riferito da due token.
    expect(css).toContain('--accent-like: rgb(215 25 33);')
    expect(css).toContain('--danger: rgb(215 25 33);')
  })

  it('l\'alone è azzerato in tutte le sue forme', () => {
    // Tre negazioni diverse, tre modi di esprimerle.
    expect(css).toContain('--accent-glow: rgb(255 255 255 / 0);')
    // Zero livelli diventa `none`: è il motivo per cui `layers: []` è ammesso.
    expect(css).toContain('--glow-accent: none;')
    expect(css).toContain('--viz-glow: 0;')
    expect(css).toContain('--scrubber-glow: 0;')
  })

  it('l\'elevazione è un bordo, e non cambia con la profondità', () => {
    // Nella skin non esiste il concetto di "più in alto": tutte e quattro le
    // ombre sono lo stesso bordo da un pixel.
    const border = '0px 0px 0px 1px var(--hairline)'
    expect(css).toContain(`--shadow-1: ${border};`)
    expect(css).toContain(`--shadow-2: ${border};`)
    expect(css).toContain(`--shadow-3: ${border};`)
    expect(css).toContain(`--shadow-player: ${border};`)
  })

  it('la curva di motion neutralizza ogni sovra-oscillazione', () => {
    // spring e outExpo sono LA STESSA curva: la skin non rimbalza.
    expect(css).toContain('--ease-out-expo: cubic-bezier(0.2, 0, 0, 1);')
    expect(css).toContain('--ease-spring: cubic-bezier(0.2, 0, 0, 1);')
  })

  it('il tema chiaro inverte, non schiarisce', () => {
    const light = css.slice(css.indexOf("[data-theme='light']"))
    expect(light).toContain('--color-surface-0: rgb(243 241 236);')
    // L'accento passa da bianco a nero: è un'inversione completa.
    expect(light).toContain('--accent: rgb(0 0 0);')
    expect(light).toContain('--accent-rgb: 0 0 0;')
  })

  it('non dichiara di seguire la copertina', () => {
    const parsed = parseSkin(NOTHING_SKIN_SOURCE)
    if (!parsed.ok) throw parsed.error
    // Se l'accento seguisse la copertina la skin diventerebbe colorata, cioè
    // smetterebbe di essere questa skin.
    expect(parsed.value.capabilities.dynamicAccent).toBe(false)
    const result = compileSkin(parsed.value)
    if (!result.ok) throw result.error
    expect(result.value.dynamicTokens).toEqual([])
  })

  it('non lascia avvisi', () => {
    const parsed = parseSkin(NOTHING_SKIN_SOURCE)
    if (!parsed.ok) throw parsed.error
    expect(checkSkin(parsed.value)).toEqual([])
  })
})

describe('nothing — le parti', () => {
  const { css, cost } = compiled()

  it('il campo di punti è sul backdrop, con colore esplicito', () => {
    // Nel legacy usava `currentColor`, che il formato non ammette: dipende da dove
    // l'effetto viene usato, quindi lo stesso motivo darebbe risultati diversi in
    // punti diversi senza che il file lo dica.
    expect(css).toContain(":root[data-skin='nothing'] .ambient-backdrop {")
    expect(css).toContain('radial-gradient(circle at center, rgb(255 255 255 / 0.16) 0 1px, transparent 1px) 0 0 / 12px 12px')
  })

  it('la scheda di sezione ha il bordo hairline invece dell\'ombra', () => {
    const rule = css.slice(css.indexOf(".section-card {"))
    expect(rule).toContain('border: 1px solid var(--hairline);')
    expect(rule).toContain('border-radius: 4px;')
  })

  it('le maiuscole spaziate sono dichiarate, non lasciate al CSS', () => {
    const rule = css.slice(css.indexOf('.hero-eyebrow {'))
    expect(rule).toContain('text-transform: uppercase;')
    expect(rule).toContain('letter-spacing: 0.14em;')
  })

  it('gli stati usano :where() per non alzare la specificità', () => {
    // Senza, uno stato dichiarato da una skin vincerebbe su una regola che il
    // componente considera più importante.
    expect(css).toContain(".nav-pill:where(:hover)")
    expect(css).toContain(".nav-pill:where([data-active], .is-active)")
    // focus-visible e non focus: lo stato appare per chi naviga da tastiera.
    expect(css).toContain(".icon-btn:where(:focus-visible)")
  })

  it('tutte le regole delle parti restano scopate sulla skin', () => {
    for (const selector of css.match(/^[^\s@/].*\{$/gm) ?? []) {
      expect(selector).toContain("data-skin='nothing'")
    }
  })

  it('ogni parte usata esiste nel registro', () => {
    const used = Object.keys(
      (NOTHING_SKIN_SOURCE as { parts: Record<string, unknown> }).parts
    )
    for (const name of used) {
      expect(PART_NAMES).toContain(name)
    }
  })

  it('rifiuta una parte inesistente', () => {
    // Nel legacy un selettore che non combacia con niente non lo dice: la skin
    // resta muta in quel punto e nessuno lo scopre.
    const broken = {
      ...NOTHING_SKIN_SOURCE,
      parts: { 'sezione-che-non-esiste': { radius: '4px' } }
    }
    const result = parseSkin(broken)
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(String(result.error.params['detail'])).toContain('parte inesistente')
    }
  })

  it('suggerisce il nome giusto quando è sbagliato per assonanza', () => {
    // Senza un suggerimento, correggere significa aprire il registro e leggerlo
    // tutto. È il tipo di errore che si fa per assonanza, non per ignoranza.
    const result = parseSkin({
      ...NOTHING_SKIN_SOURCE,
      parts: { 'section-cards': { radius: '4px' } }
    })
    expect(result.ok).toBe(false)
    if (!result.ok) {
      expect(String(result.error.params['detail'])).toContain("'section-card'")
    }
  })

  it('rifiuta una proprietà che sposterebbe il layout', () => {
    // Una skin che può spostare le cose può anche sovrapporle o portarle fuori
    // schermo, e il risultato sembrerebbe un bug dell'app, non una skin brutta.
    for (const property of ['width', 'padding', 'position', 'display', 'zIndex']) {
      const broken = {
        ...NOTHING_SKIN_SOURCE,
        parts: { 'section-card': { [property]: '100px' } }
      }
      expect(parseSkin(broken).ok).toBe(false)
    }
  })

  it('resta dentro il budget prestazionale', () => {
    // Nothing è piatta per scelta: nessun blur, nessun effetto composito. Il costo
    // deve riflettere quella scelta.
    expect(cost).toBeLessThanOrEqual(10)
  })
})
