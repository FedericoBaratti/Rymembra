/**
 * Verifica di contrasto, di serie.
 *
 * Il piano la chiede come cittadina di prima classe, e la ragione è un fatto
 * documentato del progetto: sull'albero mobile qualcuno ha dovuto **alzare a mano**
 * `--color-text-2` e `--color-text-3` da 0.38 a 0.5 perché su un telefono non si
 * leggevano. Quella correzione è arrivata dopo aver guardato lo schermo di un
 * dispositivo — cioè nel modo più tardo e più costoso possibile.
 *
 * Con i colori tenuti nei canali, la stessa domanda si può fare mentre si sceglie
 * il colore. Non è un controllo di conformità da spuntare: è il momento in cui una
 * skin diventa inutilizzabile per qualcuno, e chi la sta costruendo deve saperlo
 * subito.
 *
 * Sulle soglie: sono quelle di WCAG 2.1. 4.5 per il testo normale, 3.0 per il
 * testo grande e per i confini dei componenti. Non le abbassiamo perché una skin
 * "è pensata così": un testo che non si legge non si legge.
 */

import {
  contrastRatio,
  type Rgba,
  type SkinDocument,
  type TokenId
} from '@aether/skin'

export const AA_NORMAL = 4.5
export const AA_LARGE = 3
export const AAA_NORMAL = 7

export type ContrastLevel = 'AAA' | 'AA' | 'AA-large' | 'fail'

export interface ContrastPair {
  readonly foreground: TokenId
  readonly background: TokenId
  readonly ratio: number
  readonly level: ContrastLevel
  /** Cosa fa questa coppia nell'interfaccia. Serve a capire se importa. */
  readonly usage: string
}

export function levelFor(ratio: number): ContrastLevel {
  if (ratio >= AAA_NORMAL) return 'AAA'
  if (ratio >= AA_NORMAL) return 'AA'
  if (ratio >= AA_LARGE) return 'AA-large'
  return 'fail'
}

/**
 * Le coppie che contano davvero.
 *
 * Non il prodotto cartesiano di tutti i colori: la maggior parte delle
 * combinazioni non si verifica mai nell'interfaccia, e riportarle sommergerebbe
 * quelle vere. Queste sono le combinazioni che i componenti producono
 * effettivamente.
 */
const PAIRS: readonly { foreground: TokenId; background: TokenId; usage: string }[] = [
  { foreground: 'color.text.1', background: 'color.surface.0', usage: 'testo primario sul fondo' },
  { foreground: 'color.text.1', background: 'color.surface.1', usage: 'testo primario sui pannelli' },
  { foreground: 'color.text.1', background: 'color.surface.2', usage: 'testo primario sulle schede' },
  { foreground: 'color.text.2', background: 'color.surface.0', usage: 'testo secondario sul fondo' },
  { foreground: 'color.text.2', background: 'color.surface.1', usage: 'testo secondario sui pannelli' },
  { foreground: 'color.text.2', background: 'color.surface.2', usage: 'testo secondario sulle schede' },
  { foreground: 'color.text.3', background: 'color.surface.0', usage: 'testo terziario sul fondo' },
  { foreground: 'color.text.3', background: 'color.surface.1', usage: 'testo terziario sui pannelli' },
  { foreground: 'color.accent', background: 'color.surface.0', usage: 'accento sul fondo' },
  { foreground: 'color.accent', background: 'color.surface.1', usage: 'accento sui pannelli' },
  { foreground: 'color.danger', background: 'color.surface.1', usage: 'errori sui pannelli' },
  { foreground: 'color.success', background: 'color.surface.1', usage: 'conferme sui pannelli' },
  { foreground: 'color.warning', background: 'color.surface.1', usage: 'avvisi sui pannelli' }
]

/**
 * Risolve un token colore nei suoi canali.
 *
 * Un riferimento a un altro token si segue; una sorgente dinamica NON si può
 * valutare — dipende dalla copertina del brano in riproduzione — e viene
 * segnalata come non verificabile invece che approssimata con un valore
 * plausibile. Approssimarla darebbe un verde falso.
 */
function resolve(
  document: SkinDocument,
  id: TokenId,
  depth = 0
): Rgba | 'dynamic' | 'inherited' {
  if (depth > 8) return 'inherited'

  const tokens = document.tokens as Record<string, unknown>
  const value = tokens[id]
  if (value === undefined) return 'inherited'

  if (typeof value === 'object' && value !== null) {
    if ('$source' in value) return 'dynamic'
    if ('$palette' in value) {
      const palette = (document.palette ?? {}) as Record<string, Rgba>
      const name = (value as { $palette: string }).$palette
      const color = palette[name]
      return color ?? 'inherited'
    }
    if ('$token' in value) {
      return resolve(document, (value as { $token: TokenId }).$token, depth + 1)
    }
    if ('r' in value && 'g' in value && 'b' in value) return value as Rgba
  }

  return 'inherited'
}

export interface ContrastReport {
  readonly pairs: readonly ContrastPair[]
  /** Coppie sotto la soglia AA per il testo normale. */
  readonly failures: readonly ContrastPair[]
  /**
   * Coppie non verificabili perché un colore segue la copertina, o perché il
   * token non è dichiarato e verrà ereditato.
   */
  readonly unverifiable: readonly { foreground: TokenId; background: TokenId; reason: string }[]
}

/**
 * Verifica un tema di una skin.
 *
 * `variant` conta: la variante chiara ha spesso il contrasto peggiore, perché è
 * quella che si prova meno — ed è precisamente il motivo per cui va verificata
 * dallo stesso codice e non a occhio.
 */
export function auditContrast(
  document: SkinDocument,
  variant: 'base' | 'light' = 'base'
): ContrastReport {
  // Per la variante chiara si valuta un documento con le sovrascritture applicate:
  // il tema chiaro dichiara solo ciò che cambia, quindi va fuso con la base.
  const effective: SkinDocument =
    variant === 'light' && document.themes?.light !== undefined
      ? {
          ...document,
          tokens: {
            ...(document.tokens as Record<string, unknown>),
            ...(document.themes.light as Record<string, unknown>)
          } as SkinDocument['tokens']
        }
      : document

  const pairs: ContrastPair[] = []
  const unverifiable: { foreground: TokenId; background: TokenId; reason: string }[] = []

  for (const pair of PAIRS) {
    const foreground = resolve(effective, pair.foreground)
    const background = resolve(effective, pair.background)

    if (foreground === 'dynamic' || background === 'dynamic') {
      unverifiable.push({
        foreground: pair.foreground,
        background: pair.background,
        reason: 'segue la copertina: il contrasto cambia con il brano'
      })
      continue
    }
    if (foreground === 'inherited' || background === 'inherited') {
      unverifiable.push({
        foreground: pair.foreground,
        background: pair.background,
        reason: 'token non dichiarato: erediterà il valore di base'
      })
      continue
    }

    const ratio = contrastRatio(foreground, background)
    pairs.push({
      foreground: pair.foreground,
      background: pair.background,
      // Arrotondato a due decimali: la terza cifra non cambia nessuna decisione.
      ratio: Number(ratio.toFixed(2)),
      level: levelFor(ratio),
      usage: pair.usage
    })
  }

  return {
    pairs,
    failures: pairs.filter((entry) => entry.level === 'fail' || entry.level === 'AA-large'),
    unverifiable
  }
}

/**
 * L'opacità minima che porterebbe una coppia a passare.
 *
 * È il suggerimento concreto: sul mobile la correzione è stata «alza 0.38 a 0.5»,
 * e trovarla ha richiesto tentativi su un dispositivo. Con i canali si calcola.
 * Restituisce null quando nemmeno l'opacità piena basta — in quel caso il
 * problema è il colore, non la sua trasparenza.
 */
export function minimumAlphaFor(
  foreground: Rgba,
  background: Rgba,
  target = AA_NORMAL
): number | null {
  if (contrastRatio({ ...foreground, a: 1 }, background) < target) return null

  // Ricerca binaria su venti passi: la funzione è monotona nell'alpha, e venti
  // passi danno una precisione di 1e-6, molto oltre il necessario.
  let low = 0
  let high = 1
  for (let step = 0; step < 20; step++) {
    const middle = (low + high) / 2
    if (contrastRatio({ ...foreground, a: middle }, background) >= target) high = middle
    else low = middle
  }
  // Arrotondato PER ECCESSO, non al valore più vicino: `toFixed` potrebbe scendere
  // sotto la soglia, e un suggerimento che non risolve il problema che dichiara di
  // risolvere è peggio di nessun suggerimento. Se l'utente applica 0.5 il contrasto
  // deve passare, non arrivare a 4.49.
  return Math.min(1, Math.ceil(high * 1000) / 1000)
}
