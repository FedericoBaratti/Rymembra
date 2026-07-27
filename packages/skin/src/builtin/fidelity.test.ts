/**
 * Fedeltà: la skin convertita produce le dichiarazioni che l'app usa oggi.
 *
 * Il criterio di accettazione della fase è il confronto visivo pixel a pixel, che
 * richiede l'app in esecuzione. Questo test è il gradino sotto, e coglie la classe
 * di errori più probabile in una conversione a mano: un valore trascritto male, un
 * token dimenticato, un'unità sbagliata.
 *
 * I valori attesi qui sono copiati da `packages/ui/src/styles/global.css`. Se
 * quel file cambia, questo test deve cambiare con lui — ed è voluto: è l'unico
 * posto dove le due descrizioni della stessa skin si incontrano, finché il CSS di
 * base non verrà rimosso a fine Fase 4.
 */

import { describe, expect, it } from 'vitest'
import { compileSkin } from '../compile'
import { checkSkin, parseSkin } from '../parse'
import { REQUIRED_TOKEN_IDS } from '../tokens'
import { BUILTIN_SKIN_SOURCES } from './index'
import { PLAIN_SKIN_SOURCE } from './plain'

function compilePlain(): string {
  const parsed = parseSkin(PLAIN_SKIN_SOURCE)
  if (!parsed.ok) throw parsed.error
  const compiled = compileSkin(parsed.value)
  if (!compiled.ok) throw compiled.error
  return compiled.value.css
}

describe('skin di serie', () => {
  it('ognuna passa la validazione dei pacchetti esterni', () => {
    // Lo stesso percorso di un pacchetto importato: una built-in malformata si
    // scopre qui, non sul computer di un utente.
    for (const source of BUILTIN_SKIN_SOURCES) {
      const result = parseSkin(source)
      if (!result.ok) throw new Error(String(result.error.params['detail']))
      expect(result.ok).toBe(true)
    }
  })

  it('plain non ha avvisi: dichiara tutti i token obbligatori', () => {
    const parsed = parseSkin(PLAIN_SKIN_SOURCE)
    if (!parsed.ok) throw parsed.error
    // È la skin di riferimento: se lei lascia un token ereditato, il valore
    // ereditato è l'unica descrizione di quel token e nessuno lo sa.
    expect(checkSkin(parsed.value)).toEqual([])
    expect(REQUIRED_TOKEN_IDS.every((id) => id in parsed.value.tokens)).toBe(true)
  })
})

describe('fedeltà di plain rispetto a global.css', () => {
  const css = compilePlain()

  it.each([
    ['--color-surface-0', 'rgb(9 9 13)'],
    ['--color-surface-1', 'rgb(14 14 20)'],
    ['--color-surface-2', 'rgb(22 22 31)'],
    ['--color-surface-3', 'rgb(30 30 42)'],
    ['--color-text-1', 'rgb(255 255 255 / 0.92)'],
    ['--color-text-2', 'rgb(255 255 255 / 0.6)'],
    ['--color-text-3', 'rgb(255 255 255 / 0.38)'],
    ['--accent', 'rgb(139 124 246)'],
    ['--accent-rgb', '139 124 246'],
    ['--accent-soft', 'rgb(139 124 246 / 0.16)'],
    ['--accent-glow', 'rgb(139 124 246 / 0.35)'],
    ['--danger', 'rgb(229 72 77)'],
    ['--success', 'rgb(52 211 153)'],
    ['--warning', 'rgb(250 204 21)'],
    ['--sidebar-bg', 'rgb(255 255 255 / 0.04)'],
    ['--hairline', 'rgb(255 255 255 / 0.07)'],
    ['--rail-w', '68px'],
    ['--rail-w-expanded', '240px'],
    ['--player-h', '92px'],
    ['--player-gap', '14px'],
    ['--radius-panel', '20px'],
    ['--radius-card', '14px'],
    ['--dur-1', '150ms'],
    ['--dur-2', '280ms'],
    ['--dur-3', '450ms'],
    ['--viz-glow', '20'],
    ['--scrubber-glow', '6'],
    ['--scrubber-rest', 'rgb(255 255 255 / 0.18)']
  ])('%s vale %s', (property, value) => {
    expect(css).toContain(`${property}: ${value};`)
  })

  it('la lunghezza adattiva torna a essere un clamp', () => {
    // `clamp(16px, 3cqw, 48px)` nel legacy. Espressa come struttura, ricomposta
    // dal compilatore: non è mai passata come stringa.
    expect(css).toContain('--content-x: clamp(16px, 3cqw, 48px);')
  })

  it('le curve di motion sono identiche a quelle esistenti', () => {
    expect(css).toContain('--ease-out-expo: cubic-bezier(0.16, 1, 0.3, 1);')
    // Il rimbalzo: y = 1.56, oltre 1. Lo schema lo ammette proprio per questo.
    expect(css).toContain('--ease-spring: cubic-bezier(0.34, 1.56, 0.64, 1);')
  })

  it('le ombre hanno gli stessi livelli, inset compreso', () => {
    expect(css).toContain('--shadow-1: 0px 2px 12px rgb(0 0 0 / 0.3);')
    expect(css).toContain('--shadow-2: 0px 8px 28px rgb(0 0 0 / 0.45);')
    expect(css).toContain('--shadow-3: 0px 16px 56px rgb(0 0 0 / 0.55);')
    expect(css).toContain(
      '--shadow-player: inset 0px 1px 0px rgb(255 255 255 / 0.06), 0px 8px 40px rgb(0 0 0 / 0.5);'
    )
    expect(css).toContain('--glow-accent: 0px 0px 24px var(--accent-glow);')
  })

  it('i riferimenti fra token restano riferimenti, non colori duplicati', () => {
    // Nel legacy: `--accent-like: var(--accent)` e `--viz-primary: var(--accent)`.
    // Duplicare il valore significherebbe che seguire la copertina smette di
    // funzionare per quei due token.
    expect(css).toContain('--accent-like: var(--accent);')
    expect(css).toContain('--viz-primary: var(--accent);')
    expect(css).toContain('--hero-rgb: var(--accent-rgb);')
  })

  it('emette color-scheme, che nel legacy era scritto a mano in ogni skin', () => {
    // Dimenticarlo dà barre di scorrimento chiare su fondo nero.
    expect(css).toContain('color-scheme: dark;')
    expect(css).toContain('color-scheme: light;')
  })

  it('il tema chiaro sovrascrive solo ciò che serve', () => {
    const lightBlock = css.slice(css.indexOf("[data-theme='light']"))
    expect(lightBlock).toContain('--danger: rgb(209 36 43);')
    expect(lightBlock).toContain('--hairline: rgb(0 0 0 / 0.08);')
    // Le superfici NON cambiano nel tema chiaro di plain: la variante chiara di
    // global.css tocca soltanto chrome e semantici.
    expect(lightBlock).not.toContain('--color-surface-0:')
  })

  it('le transizioni di rotta riproducono vt-out e vt-in', () => {
    expect(css).toContain('transform: scale(0.992);')
    expect(css).toContain('transform: translateY(8px);')
  })

  it('tutto resta scopato sotto data-skin=plain', () => {
    for (const selector of css.match(/^[^\s@/].*\{$/gm) ?? []) {
      expect(selector).toContain("data-skin='plain'")
    }
  })

  it('non costa niente: nessun effetto, nessun budget consumato', () => {
    const parsed = parseSkin(PLAIN_SKIN_SOURCE)
    if (!parsed.ok) throw parsed.error
    const compiled = compileSkin(parsed.value)
    if (!compiled.ok) throw compiled.error
    // Plain è il riferimento anche prestazionale: zero motivi, zero costo.
    expect(compiled.value.cost).toBe(0)
  })
})
