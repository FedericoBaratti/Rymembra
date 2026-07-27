/**
 * Il compilatore: da `skin.json` a CSS.
 *
 * È l'**unico** autore di CSS in tutto il sistema. Non esiste un percorso per cui
 * una stringa scritta da chi crea la skin arrivi nell'output: i colori entrano
 * come canali, le lunghezze come numero più unità, i nomi delle proprietà vengono
 * dal registro dei token e non dal documento. Quella proprietà è ciò che rende
 * l'intera classe di attacchi via CSS — `url()` che chiama a casa, `@import`,
 * selettori che esfiltrano attributi — non applicabile, e i test la verificano
 * come proprietà dell'output, non come intenzione.
 *
 * Gira nel renderer, non nel backend: nessun vincolo Node 12, e il foglio si
 * sostituisce con `adoptedStyleSheets` — che è ciò che rende l'anteprima dal vivo
 * dello Studio istantanea invece di un ricaricamento.
 */

import { AppError } from '@aether/core'
import { err, ok, type Result } from '@aether/core'
import { EFFECT_TARGET, stackCost, type Effect } from './effects'
import { compilePart, type PartName, type PartStyle } from './parts'
import { TOKENS, tokenDef, type ColorValue, type ShadowValue, type TokenId } from './tokens'
import {
  formatColor,
  formatDuration,
  formatEasing,
  formatFontStack,
  formatLength,
  formatLengthValue,
  formatRgbTriple,
  type Length,
  type LengthValue,
  type Rgba
} from './values'
import type { SkinDocument, SkinTokens } from './schema'

export interface CompiledSkin {
  readonly id: string
  readonly css: string
  /** Somma dei costi dei motivi, per il budget prestazionale. */
  readonly cost: number
  /** Token che seguono la copertina: il runtime deve aggiornarli. */
  readonly dynamicTokens: readonly TokenId[]
}

/** Prefisso delle proprietà dei motivi. Evita collisioni fra skin e col registro. */
const PATTERN_PREFIX = '--skin-'

function selectorFor(id: string, variant: 'base' | 'light' | 'mobile'): string {
  const root = `:root[data-skin='${id}']`
  switch (variant) {
    case 'base':
      return root
    case 'light':
      return `${root}[data-theme='light']`
    case 'mobile':
      return `${root}[data-mobile]`
  }
}

/**
 * Risolve un valore di colore in CSS.
 *
 * I tre casi hanno tre significati diversi: un letterale è un colore, un
 * riferimento è `var()` verso un altro token del registro — e il nome della
 * variabile viene dal registro, quindi non può essere testo arbitrario — e una
 * sorgente dinamica è un legame che il runtime aggiorna dalla copertina.
 */
function resolveColor(value: ColorValue): string {
  if (typeof value === 'object' && '$token' in value) {
    // Il nome della variabile viene dal registro, non dal documento: è la ragione
    // per cui un riferimento non può diventare un canale di iniezione.
    return `var(${tokenDef(value.$token as TokenId).css})`
  }
  if (typeof value === 'object' && '$source' in value) {
    // Il legame con la copertina passa per i token che il runtime già aggiorna:
    // è il meccanismo esistente, non uno nuovo.
    const base = value.$source === 'albumArt.vibrant' ? '--accent' : '--accent'
    if (value.alpha === undefined) return `var(${base})`
    // Con un'opacità serve la tripla, perché `rgb(var(--x) / a)` funziona solo se
    // --x è una tripla di canali. È il motivo per cui i token `*-rgb` esistono.
    return `rgb(var(--accent-rgb) / ${Number(value.alpha.toFixed(3))})`
  }
  return formatColor(value as Rgba)
}

function isLiteralColor(value: ColorValue): value is Rgba {
  return typeof value === 'object' && !('$token' in value) && !('$source' in value)
}

function resolveShadow(value: ShadowValue): string {
  if (value.layers.length === 0) return 'none'
  return value.layers
    .map((layer) => {
      const parts: string[] = []
      if (layer.inset === true) parts.push('inset')
      parts.push(formatLength(layer.x), formatLength(layer.y), formatLength(layer.blur))
      if (layer.spread !== undefined) parts.push(formatLength(layer.spread))
      parts.push(resolveColor(layer.color))
      return parts.join(' ')
    })
    .join(', ')
}

function stopList(
  stops: readonly { color: ColorValue; at?: Length | null }[]
): string {
  return stops
    .map((stop) => {
      const color = resolveColor(stop.color)
      return stop.at === undefined || stop.at === null ? color : `${color} ${formatLength(stop.at)}`
    })
    .join(', ')
}

/** Un effetto in un valore CSS. Il `target` dice in quale proprietà va. */
export function compileEffect(effect: Effect): string {
  switch (effect.effect) {
    case 'solid':
      return resolveColor(effect.color)

    case 'linearGradient':
      return `linear-gradient(${num(effect.angle)}deg, ${stopList(effect.stops)})`

    case 'radialGradient': {
      const position = effect.at === undefined ? '' : ` at ${formatLength(effect.at[0])} ${formatLength(effect.at[1])}`
      const size = effect.size === undefined ? '' : ` ${formatLength(effect.size)}`
      return `radial-gradient(${effect.shape}${size}${position}, ${stopList(effect.stops)})`
    }

    case 'conicGradient': {
      const position = effect.at === undefined ? '' : ` at ${formatLength(effect.at[0])} ${formatLength(effect.at[1])}`
      return `conic-gradient(from ${num(effect.from)}deg${position}, ${stopList(effect.stops)})`
    }

    case 'hairlineGrid': {
      // Le due righe che nel legacy erano copiate a mano, con il passo ripetuto.
      const color = resolveColor(effect.color)
      const thickness = formatLength(effect.thickness ?? { value: 1, unit: 'px' })
      const cellX = formatLength(effect.cell)
      const cellY = formatLength(effect.cellY ?? effect.cell)
      return [
        `linear-gradient(${color} ${thickness}, transparent ${thickness}) 0 0 / 100% ${cellY}`,
        `linear-gradient(90deg, ${color} ${thickness}, transparent ${thickness}) 0 0 / ${cellX} 100%`
      ].join(', ')
    }

    case 'scanlines': {
      const color = resolveColor(effect.color)
      const line = formatLength(effect.line)
      const gap = formatLength(effect.gap)
      return `repeating-linear-gradient(0deg, ${color} 0 ${line}, transparent ${line} ${gap})`
    }

    case 'stripes': {
      const color = resolveColor(effect.color)
      const background = resolveColor(effect.background)
      const width = formatLength(effect.width)
      const twice = formatLength({ value: effect.width.value * 2, unit: effect.width.unit })
      return `repeating-linear-gradient(${num(effect.angle)}deg, ${color} 0 ${width}, ${background} ${width} ${twice})`
    }

    case 'dotGrid': {
      const color = resolveColor(effect.color)
      const dot = formatLength(effect.dot)
      const spacing = formatLength(effect.spacing)
      return `radial-gradient(circle at center, ${color} 0 ${dot}, transparent ${dot}) 0 0 / ${spacing} ${spacing}`
    }

    case 'vignette': {
      const color = resolveColor(effect.color)
      return `radial-gradient(ellipse at center, transparent ${num(effect.start)}%, ${color} 100%)`
    }

    case 'chamfer': {
      // Il poligono che nel legacy erano due token da tenere coerenti a mano: la
      // misura in `--cyber-cut` e la forma in `--cyber-chamfer`.
      const size = formatLength(effect.size)
      const corners = new Set(effect.corners)
      const points: string[] = []
      points.push(corners.has('topLeft') ? `0 ${size}` : '0 0')
      if (corners.has('topLeft')) points.push(`${size} 0`)
      points.push(corners.has('topRight') ? `calc(100% - ${size}) 0` : '100% 0')
      if (corners.has('topRight')) points.push(`100% ${size}`)
      points.push(
        corners.has('bottomRight') ? `100% calc(100% - ${size})` : '100% 100%'
      )
      if (corners.has('bottomRight')) points.push(`calc(100% - ${size}) 100%`)
      points.push(corners.has('bottomLeft') ? `${size} 100%` : '0 100%')
      if (corners.has('bottomLeft')) points.push(`0 calc(100% - ${size})`)
      return `polygon(${points.join(', ')})`
    }

    case 'blurBehind': {
      const radius = formatLength(effect.radius)
      const saturate = effect.saturate === undefined ? '' : ` saturate(${num(effect.saturate)}%)`
      return `blur(${radius})${saturate}`
    }
  }
}

function num(value: number): number {
  return Number(value.toFixed(4))
}

interface Declaration {
  readonly property: string
  readonly value: string
}

/** Le dichiarazioni prodotte da un blocco di token. */
function compileTokens(tokens: SkinTokens): {
  declarations: Declaration[]
  dynamic: TokenId[]
} {
  const declarations: Declaration[] = []
  const dynamic: TokenId[] = []
  const entries = tokens as Record<string, unknown>

  // L'ordine è quello del registro, non quello di scrittura nel documento: un
  // output deterministico è ciò che rende utili gli snapshot e la cache.
  for (const id of Object.keys(TOKENS) as TokenId[]) {
    const value = entries[id]
    if (value === undefined) continue
    const def = tokenDef(id)

    switch (def.kind) {
      case 'color': {
        const color = value as ColorValue
        if (typeof color === 'object' && '$source' in color) dynamic.push(id)
        // `color.hero` esiste solo come tripla: emettere `--hero-rgb: rgb(...)`
        // romperebbe le rgba() che lo usano.
        if (def.css === def.rgbTriple) {
          if (isLiteralColor(color)) {
            declarations.push({ property: def.css, value: formatRgbTriple(color) })
          } else {
            declarations.push({ property: def.css, value: 'var(--accent-rgb)' })
          }
          break
        }
        declarations.push({ property: def.css, value: resolveColor(color) })
        if (def.rgbTriple !== undefined) {
          // La tripla derivata: non può divergere dal colore, perché è lo stesso
          // dato. È la correzione del commento «DEVE combaciare con surface-0».
          declarations.push({
            property: def.rgbTriple,
            value: isLiteralColor(color) ? formatRgbTriple(color) : 'var(--accent-rgb)'
          })
        }
        break
      }
      case 'length':
        declarations.push({ property: def.css, value: formatLengthValue(value as LengthValue) })
        break
      case 'duration':
        declarations.push({
          property: def.css,
          value: formatDuration(value as { ms: number })
        })
        break
      case 'easing':
        declarations.push({
          property: def.css,
          value: formatEasing(value as Parameters<typeof formatEasing>[0])
        })
        break
      case 'number':
        declarations.push({ property: def.css, value: String(num(value as number)) })
        break
      case 'fontStack': {
        const kind = id === 'font.mono' ? 'mono' : id === 'font.display' ? 'display' : 'sans'
        declarations.push({
          property: def.css,
          value: formatFontStack(value as string[], kind)
        })
        break
      }
      case 'shadow':
        declarations.push({ property: def.css, value: resolveShadow(value as ShadowValue) })
        break
    }
  }

  return { declarations, dynamic }
}

/**
 * I token CALCOLATI.
 *
 * Non sono scelte di stile, sono conseguenze, e una skin non deve poterle
 * contraddire: `--shell-left` è la larghezza del rail, `--player-clearance`
 * l'altezza del player più due volte il suo margine, le due `--transition-*` una
 * durata più una curva. Nel legacy stavano in `:root` insieme a tutto il resto,
 * quindi una skin poteva sovrascriverle con valori incoerenti e la shell si
 * sfasava.
 */
function computedDeclarations(tokens: SkinTokens): Declaration[] {
  const entries = tokens as Record<string, unknown>
  const declarations: Declaration[] = []

  if (entries['layout.rail'] !== undefined) {
    declarations.push({ property: '--shell-left', value: 'var(--rail-w)' })
  }
  if (entries['layout.playerHeight'] !== undefined || entries['layout.playerGap'] !== undefined) {
    declarations.push({
      property: '--player-clearance',
      value: 'calc(var(--player-h) + var(--player-gap) * 2)'
    })
  }
  if (entries['motion.dur.1'] !== undefined) {
    declarations.push({
      property: '--transition-fast',
      value: 'var(--dur-1) var(--ease-out-expo)'
    })
  }
  if (entries['motion.dur.2'] !== undefined) {
    declarations.push({
      property: '--transition-med',
      value: 'var(--dur-2) var(--ease-out-expo)'
    })
  }

  return declarations
}

function compilePatterns(patterns: Record<string, Effect>): {
  declarations: Declaration[]
  cost: number
} {
  const declarations: Declaration[] = []
  const effects: Effect[] = []

  for (const name of Object.keys(patterns).sort()) {
    const effect = patterns[name]
    if (effect === undefined) continue
    effects.push(effect)
    const target = EFFECT_TARGET[effect.effect]
    // Il suffisso dice in quale proprietà va usato: un clip-path e uno sfondo non
    // si scambiano, e il nome lo rende evidente a chi scrive le parti.
    const suffix = target === 'clipPath' ? '-clip' : target === 'filter' ? '-filter' : ''
    declarations.push({
      property: `${PATTERN_PREFIX}${name}${suffix}`,
      value: compileEffect(effect)
    })
  }

  return { declarations, cost: stackCost(effects) }
}

/**
 * Le parti ridisegnate.
 *
 * L'ordine è quello del registro e non quello di scrittura, per la stessa ragione
 * dei token: un output deterministico è ciò che rende utili gli snapshot e la
 * cache dell'anteprima dal vivo. Ma qui c'è un motivo in più — nel CSS l'ordine
 * decide la cascata a parità di specificità, quindi un ordine che dipende da come
 * è stato scritto il JSON renderebbe il risultato imprevedibile.
 */
function compileParts(
  skinId: string,
  parts: Partial<Record<string, PartStyle>> | undefined
): { css: string; cost: number } {
  if (parts === undefined) return { css: '', cost: 0 }

  let css = ''
  const effects: Effect[] = []

  for (const name of Object.keys(parts).sort()) {
    const style = parts[name]
    if (style === undefined) continue

    for (const effect of style.background ?? []) effects.push(effect)
    for (const effect of style.layer?.background ?? []) effects.push(effect)
    if (style.clip !== undefined) effects.push(style.clip)

    const rules = compilePart(
      skinId,
      name as PartName,
      style,
      compileEffect,
      (color) => resolveColor(color as ColorValue),
      (length) => formatLength(length as Length)
    )

    for (const rule of rules) {
      css += block(
        rule.selector,
        rule.declarations.map((declaration) => ({
          property: declaration.property,
          value: declaration.value
        }))
      )
    }
  }

  return { css, cost: stackCost(effects) }
}

function block(selector: string, declarations: readonly Declaration[]): string {
  if (declarations.length === 0) return ''
  const body = declarations
    .map((declaration) => `  ${declaration.property}: ${declaration.value};`)
    .join('\n')
  return `${selector} {\n${body}\n}\n`
}

/**
 * Il motion come dati.
 *
 * L'intensità si compone con `prefers-reduced-motion` e non lo sovrascrive verso
 * l'alto: la media query del sistema viene DOPO, quindi vince. I tre blocchi
 * reduced-motion esistenti restano il pavimento, come previsto.
 */
function compileMotion(id: string, motion: NonNullable<SkinDocument['motion']>): string {
  const declarations: Declaration[] = [
    { property: '--motion-intensity', value: motion.intensity }
  ]

  const scale =
    motion.intensity === 'none'
      ? 0
      : motion.intensity === 'essential'
        ? 0.5
        : motion.intensity === 'maximum'
          ? 1.25
          : 1
  declarations.push({ property: '--motion-scale', value: String(scale) })

  for (const [name, easing] of Object.entries(motion.easings ?? {})) {
    declarations.push({
      property: `${PATTERN_PREFIX}ease-${name}`,
      value: formatEasing(easing)
    })
  }

  let css = block(selectorFor(id, 'base'), declarations)

  const transition = motion.routeTransition
  if (transition !== undefined) {
    // Le animazioni delle transizioni di rotta: solo transform e opacity, che è
    // il vincolo che il compilatore impone e non una raccomandazione.
    if (transition.out !== undefined) {
      css += `:root[data-skin='${id}']::view-transition-old(root) {\n  animation: skin-${id}-out var(--dur-2) var(--ease-out-expo) both;\n}\n`
      css += keyframes(`skin-${id}-out`, 'to', transition.out)
    }
    if (transition.in !== undefined) {
      css += `:root[data-skin='${id}']::view-transition-new(root) {\n  animation: skin-${id}-in var(--dur-2) var(--ease-out-expo) both;\n}\n`
      css += keyframes(`skin-${id}-in`, 'from', transition.in)
    }
  }

  return css
}

function keyframes(
  name: string,
  at: 'from' | 'to',
  frame: { opacity?: number; scale?: number; translateY?: number }
): string {
  const declarations: string[] = []
  if (frame.opacity !== undefined) declarations.push(`    opacity: ${num(frame.opacity)};`)
  const transforms: string[] = []
  if (frame.translateY !== undefined) transforms.push(`translateY(${num(frame.translateY)}px)`)
  if (frame.scale !== undefined) transforms.push(`scale(${num(frame.scale)})`)
  if (transforms.length > 0) declarations.push(`    transform: ${transforms.join(' ')};`)
  if (declarations.length === 0) return ''
  return `@keyframes ${name} {\n  ${at} {\n${declarations.join('\n')}\n  }\n}\n`
}

/**
 * Compila una skin già validata.
 *
 * Restituisce un Result: un documento validato non dovrebbe più poter fallire,
 * ma «non dovrebbe» non è una garanzia, e un compilatore che lancia dentro
 * l'anteprima dal vivo dello Studio rende inutilizzabile l'editor invece di
 * segnalare un problema.
 */
export function compileSkin(skin: SkinDocument): Result<CompiledSkin, AppError> {
  try {
    const base = compileTokens(skin.tokens)
    const computed = computedDeclarations(skin.tokens)
    const patterns = compilePatterns(skin.patterns ?? {})

    let css = `/* ${skin.meta.name} ${skin.meta.version} — generato, non modificare a mano */\n`
    css += block(selectorFor(skin.id, 'base'), [
      ...base.declarations,
      ...computed,
      ...patterns.declarations,
      // `color-scheme` non è un token: è ciò che dice al motore di rendering come
      // disegnare le barre di scorrimento e i controlli nativi. Nel legacy stava
      // scritto a mano in ogni skin, e dimenticarlo dava scrollbar chiare su fondo
      // nero.
      { property: 'color-scheme', value: 'dark' }
    ])

    const light = skin.themes?.light
    if (light !== undefined) {
      const compiled = compileTokens(light)
      css += block(selectorFor(skin.id, 'light'), [
        ...compiled.declarations,
        ...computedDeclarations(light),
        { property: 'color-scheme', value: 'light' }
      ])
    }

    const mobile = skin.platforms?.mobile
    if (mobile !== undefined) {
      const compiled = compileTokens(mobile)
      css += block(selectorFor(skin.id, 'mobile'), [
        ...compiled.declarations,
        ...computedDeclarations(mobile)
      ])
    }

    if (skin.motion !== undefined) css += compileMotion(skin.id, skin.motion)

    const compiledParts = compileParts(skin.id, skin.parts)
    css += compiledParts.css

    return ok({
      id: skin.id,
      css,
      // Il costo somma motivi e parti: è la cifra che il budget della Fase 7
      // confronta, e sommarne solo una metà la renderebbe inutile.
      cost: patterns.cost + compiledParts.cost,
      dynamicTokens: base.dynamic
    })
  } catch (cause) {
    // Un documento validato non dovrebbe più poter fallire qui: se accade è un
    // buco fra schema e compilatore, cioè un bug nostro, e va detto come tale.
    return err(
      AppError.of(
        'internal.invariantViolated',
        { what: `la skin ${skin.id} ha superato la validazione ma non compila` },
        { cause }
      )
    )
  }
}
