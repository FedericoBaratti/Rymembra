/**
 * I valori che una skin può scrivere.
 *
 * Questo file è la sicurezza dell'intero formato, e vale spiegare perché non è
 * un dettaglio di validazione.
 *
 * La decisione di progetto è: **zero CSS arbitrario**. Una skin non contiene
 * stringhe CSS, contiene dati; il compilatore è l'unico autore di CSS. La
 * conseguenza è che tutta la classe di attacchi via CSS — `url()` che chiama a
 * casa, `@import` che carica un foglio remoto, selettori che esfiltrano il
 * contenuto degli attributi — non si applica, perché non esiste un canale in cui
 * infilarli. Ma questo vale solo se OGNI valore è tipizzato: basta un campo che
 * accetti una stringa e la copi nell'output, e la garanzia salta tutta insieme.
 *
 * Quindi: un colore viene parsato nei suoi canali, una lunghezza è un numero più
 * un'unità da lista chiusa, un easing è una curva con quattro numeri. Niente di
 * ciò che entra viene mai copiato tale e quale.
 */

import { z } from 'zod'

// ── Colori ──────────────────────────────────────────────────────────────────

/**
 * Un colore, sempre nei suoi canali.
 *
 * Perché scomposto e non una stringa: la skin dichiara `--accent`, ma il
 * contratto token esistente richiede ANCHE `--accent-rgb` come tripla ("252 238
 * 10") perché i canvas del visualizer e dello scrubber la leggono per costruire
 * `rgba()` a runtime. Nel legacy erano due token da tenere allineati a mano — e
 * lo stesso valeva per `--cyber-fog-rgb`, che aveva un commento in maiuscolo
 * «DEVE combaciare con surface-0». Con i canali, la tripla si deriva e non può
 * divergere.
 */
export interface Rgba {
  readonly r: number
  readonly g: number
  readonly b: number
  readonly a: number
}

const HEX_RE = /^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i
const RGB_FN_RE =
  /^rgba?\(\s*(\d{1,3})\s*[, ]\s*(\d{1,3})\s*[, ]\s*(\d{1,3})\s*(?:[,/]\s*([0-9.]+)\s*)?\)$/i

function clamp(value: number, min: number, max: number): number {
  return value < min ? min : value > max ? max : value
}

function expandHex(hex: string): string {
  if (hex.length === 4 || hex.length === 5) {
    // #abc → #aabbcc, #abcd → #aabbccdd
    return `#${hex.slice(1).split('').map((c) => c + c).join('')}`
  }
  return hex
}

/** Parsa un colore. Restituisce null invece di lanciare: chi chiama fa un errore tipizzato. */
export function parseColor(input: string): Rgba | null {
  const text = input.trim()

  if (HEX_RE.test(text)) {
    const hex = expandHex(text).slice(1)
    const r = Number.parseInt(hex.slice(0, 2), 16)
    const g = Number.parseInt(hex.slice(2, 4), 16)
    const b = Number.parseInt(hex.slice(4, 6), 16)
    const a = hex.length === 8 ? Number.parseInt(hex.slice(6, 8), 16) / 255 : 1
    return { r, g, b, a }
  }

  const fn = RGB_FN_RE.exec(text)
  if (fn !== null) {
    const [, rRaw, gRaw, bRaw, aRaw] = fn
    if (rRaw === undefined || gRaw === undefined || bRaw === undefined) return null
    const r = clamp(Number(rRaw), 0, 255)
    const g = clamp(Number(gRaw), 0, 255)
    const b = clamp(Number(bRaw), 0, 255)
    const a = aRaw === undefined ? 1 : clamp(Number(aRaw), 0, 1)
    if (!Number.isFinite(a)) return null
    return { r, g, b, a }
  }

  // Nessun nome di colore CSS, nessun `currentColor`, nessun `color-mix()`. La
  // lista chiusa è il punto: se non si sa cos'è, non entra.
  return null
}

/** `rgb(r g b / a)`, la forma che il compilatore emette. */
export function formatColor(color: Rgba): string {
  const { r, g, b, a } = color
  const round = (n: number): number => Math.round(clamp(n, 0, 255))
  if (a >= 1) return `rgb(${round(r)} ${round(g)} ${round(b)})`
  const alpha = Number(clamp(a, 0, 1).toFixed(3))
  return `rgb(${round(r)} ${round(g)} ${round(b)} / ${alpha})`
}

/** La tripla senza alpha, per i token `*-rgb` che i canvas leggono. */
export function formatRgbTriple(color: Rgba): string {
  return `${Math.round(color.r)} ${Math.round(color.g)} ${Math.round(color.b)}`
}

/** Lo stesso colore con un'altra opacità. Usata dai token `*-soft` e `*-glow`. */
export function withAlpha(color: Rgba, alpha: number): Rgba {
  return { ...color, a: clamp(alpha, 0, 1) }
}

/**
 * Luminanza relativa secondo WCAG 2.1.
 *
 * Sta qui perché serve al controllo di contrasto dello Studio, e perché è la
 * ragione per cui i colori vanno tenuti nei canali: sull'albero mobile qualcuno
 * ha dovuto alzare a mano `--color-text-2/3` da 0.38 a 0.5 per renderli leggibili
 * su un telefono, scoprendolo sul dispositivo. Con i canali il controllo si fa
 * prima.
 */
export function relativeLuminance(color: Rgba): number {
  const channel = (raw: number): number => {
    const c = clamp(raw, 0, 255) / 255
    return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4)
  }
  return 0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

/**
 * Rapporto di contrasto fra due colori, appiattendo il primo sul secondo quando
 * è semitrasparente — che è il caso normale qui, dato che i token del testo sono
 * `rgba(255 255 255 / 0.6)` sopra una superficie.
 */
export function contrastRatio(foreground: Rgba, background: Rgba): number {
  const flattened =
    foreground.a >= 1
      ? foreground
      : {
          r: foreground.r * foreground.a + background.r * (1 - foreground.a),
          g: foreground.g * foreground.a + background.g * (1 - foreground.a),
          b: foreground.b * foreground.a + background.b * (1 - foreground.a),
          a: 1
        }
  const lighter = Math.max(relativeLuminance(flattened), relativeLuminance(background))
  const darker = Math.min(relativeLuminance(flattened), relativeLuminance(background))
  return (lighter + 0.05) / (darker + 0.05)
}

// ── Lunghezze e numeri ──────────────────────────────────────────────────────

/**
 * Unità ammesse. Lista chiusa, e corta di proposito.
 *
 * Fuori restano `calc()` e le stringhe libere: una lunghezza è un numero più
 * un'unità, e il compilatore compone lui i `calc()` che servono (per esempio
 * `--player-clearance`, che nel contratto esistente è
 * `calc(var(--player-h) + var(--player-gap) * 2)`).
 */
export const LENGTH_UNITS = ['px', 'rem', 'em', '%', 'vh', 'vw', 'cqw', 'cqh', 'ch'] as const
export type LengthUnit = (typeof LENGTH_UNITS)[number]

export interface Length {
  readonly value: number
  readonly unit: LengthUnit
}

const LENGTH_RE = /^(-?(?:\d+\.?\d*|\.\d+))([a-z%]+)$/i

export function parseLength(input: string): Length | null {
  const match = LENGTH_RE.exec(input.trim())
  if (match === null) return null
  const [, rawValue, rawUnit] = match
  if (rawValue === undefined || rawUnit === undefined) return null
  const value = Number(rawValue)
  if (!Number.isFinite(value)) return null
  const unit = LENGTH_UNITS.find((candidate) => candidate === rawUnit.toLowerCase())
  if (unit === undefined) return null
  return { value, unit }
}

export function formatLength(length: Length): string {
  return `${Number(length.value.toFixed(4))}${length.unit}`
}

/** Zero è l'unico numero senza unità che CSS accetta come lunghezza. */
export const ZERO_LENGTH: Length = { value: 0, unit: 'px' }

/**
 * Una lunghezza che si adatta: minimo, preferito, massimo.
 *
 * È emersa convertendo la skin `plain`: `--content-x` nel legacy è
 * `clamp(16px, 3cqw, 48px)`, e una lunghezza semplice non lo esprime. La risposta
 * NON è ammettere `clamp()` come stringa — sarebbe il primo campo che copia testo
 * nell'output, e da lì la garanzia del formato salta tutta. La risposta è la
 * struttura: tre lunghezze, e il compilatore compone lui la funzione.
 */
export interface ClampLength {
  readonly min: Length
  readonly preferred: Length
  readonly max: Length
}

export type LengthValue = Length | ClampLength

export function isClampLength(value: LengthValue): value is ClampLength {
  return 'preferred' in value
}

export function formatLengthValue(value: LengthValue): string {
  if (isClampLength(value)) {
    return `clamp(${formatLength(value.min)}, ${formatLength(value.preferred)}, ${formatLength(value.max)})`
  }
  return formatLength(value)
}

// ── Durate ──────────────────────────────────────────────────────────────────

export interface Duration {
  readonly ms: number
}

const DURATION_RE = /^(\d+\.?\d*|\.\d+)(ms|s)$/i

/** Tetto sulle durate: un'animazione di dieci secondi è un blocco, non uno stile. */
export const MAX_DURATION_MS = 10_000

export function parseDuration(input: string): Duration | null {
  const match = DURATION_RE.exec(input.trim())
  if (match === null) return null
  const [, rawValue, rawUnit] = match
  if (rawValue === undefined || rawUnit === undefined) return null
  const value = Number(rawValue)
  if (!Number.isFinite(value) || value < 0) return null
  const ms = rawUnit.toLowerCase() === 's' ? value * 1000 : value
  if (ms > MAX_DURATION_MS) return null
  return { ms }
}

export function formatDuration(duration: Duration): string {
  return `${Number(duration.ms.toFixed(2))}ms`
}

// ── Easing ──────────────────────────────────────────────────────────────────

export const EASING_KEYWORDS = ['linear', 'ease', 'ease-in', 'ease-out', 'ease-in-out'] as const
export type EasingKeyword = (typeof EASING_KEYWORDS)[number]

export type Easing =
  | { readonly kind: 'keyword'; readonly keyword: EasingKeyword }
  /** I due punti di controllo. x deve stare in [0,1]; y può uscirne (rimbalzo). */
  | { readonly kind: 'cubicBezier'; readonly points: readonly [number, number, number, number] }
  | { readonly kind: 'steps'; readonly count: number; readonly position: 'start' | 'end' }

export function formatEasing(easing: Easing): string {
  switch (easing.kind) {
    case 'keyword':
      return easing.keyword
    case 'cubicBezier': {
      const [x1, y1, x2, y2] = easing.points
      return `cubic-bezier(${trim(x1)}, ${trim(y1)}, ${trim(x2)}, ${trim(y2)})`
    }
    case 'steps':
      return `steps(${Math.round(easing.count)}, ${easing.position})`
  }
}

function trim(value: number): number {
  return Number(value.toFixed(4))
}

// ── Schemi zod ──────────────────────────────────────────────────────────────

/**
 * Perché gli schemi trasformano invece di limitarsi a controllare: il valore che
 * esce dalla validazione è già la forma interna (canali, numero più unità, curva),
 * quindi il compilatore non vede mai il testo originale. È l'invariante che
 * rende vera la promessa «il compilatore è l'unico autore di CSS».
 */

export const colorSchema = z
  .string()
  .transform((raw, ctx) => {
    const parsed = parseColor(raw)
    if (parsed === null) {
      ctx.addIssue({
        code: 'custom',
        message: `colore non valido: ${raw}. Ammessi #rgb, #rrggbb, #rrggbbaa, rgb() e rgba()`
      })
      return z.NEVER
    }
    return parsed
  })

export const lengthSchema = z.string().transform((raw, ctx) => {
  const parsed = parseLength(raw)
  if (parsed === null) {
    ctx.addIssue({
      code: 'custom',
      message: `lunghezza non valida: ${raw}. Un numero più un'unità fra ${LENGTH_UNITS.join(', ')}`
    })
    return z.NEVER
  }
  return parsed
})

export const clampLengthSchema = z.object({
  min: lengthSchema,
  preferred: lengthSchema,
  max: lengthSchema
})

/**
 * Una lunghezza semplice o adattiva.
 *
 * Dispatch a mano e non `z.union`, per lo stesso motivo del colore: con l'unione
 * zod riporta «Invalid input» e il messaggio che nomina l'unità sbagliata — l'unica
 * cosa utile a chi sta scrivendo la skin — viene inghiottito.
 */
export const lengthValueSchema = z.unknown().transform((raw, ctx) => {
  if (typeof raw === 'object' && raw !== null) {
    const parsed = clampLengthSchema.safeParse(raw)
    if (!parsed.success) {
      ctx.addIssue({
        code: 'custom',
        message:
          parsed.error.issues[0]?.message ??
          'una lunghezza adattiva va scritta come { "min": …, "preferred": …, "max": … }'
      })
      return z.NEVER
    }
    return parsed.data
  }

  if (typeof raw === 'string') {
    const parsed = lengthSchema.safeParse(raw)
    if (!parsed.success) {
      ctx.addIssue({
        code: 'custom',
        message: parsed.error.issues[0]?.message ?? `lunghezza non valida: ${raw}`
      })
      return z.NEVER
    }
    return parsed.data
  }

  ctx.addIssue({
    code: 'custom',
    message: 'una lunghezza va scritta come "14px" o come { min, preferred, max }'
  })
  return z.NEVER
})

export const durationSchema = z.string().transform((raw, ctx) => {
  const parsed = parseDuration(raw)
  if (parsed === null) {
    ctx.addIssue({
      code: 'custom',
      message: `durata non valida: ${raw}. Un numero in ms o s, al massimo ${MAX_DURATION_MS}ms`
    })
    return z.NEVER
  }
  return parsed
})

export const easingSchema: z.ZodType<Easing> = z.union([
  z.object({
    kind: z.literal('keyword'),
    keyword: z.enum(EASING_KEYWORDS)
  }),
  z.object({
    kind: z.literal('cubicBezier'),
    points: z.tuple([
      z.number().min(0).max(1),
      // y fuori da [0,1] è legittimo: è come si ottiene il rimbalzo di
      // --ease-spring, cubic-bezier(0.34, 1.56, 0.64, 1).
      z.number().min(-5).max(5),
      z.number().min(0).max(1),
      z.number().min(-5).max(5)
    ])
  }),
  z.object({
    kind: z.literal('steps'),
    count: z.number().int().min(1).max(60),
    position: z.enum(['start', 'end'])
  })
])

/**
 * Un numero nudo, per i token che il JS legge come numero: `--viz-glow`,
 * `--scrubber-glow`. Non sono lunghezze CSS, sono parametri letti dai canvas.
 */
export const unitlessSchema = z.number().finite()

/**
 * Una famiglia di caratteri.
 *
 * Solo il nome, e con una forma vincolata: il compilatore lo mette lui fra
 * apici e aggiunge lui i fallback di sistema. Un nome con virgolette o punti e
 * virgola potrebbe chiudere la dichiarazione e aprirne un'altra, ed è
 * esattamente il tipo di fuga che il formato deve rendere impossibile.
 */
export const fontFamilySchema = z
  .string()
  .min(1)
  .max(64)
  .regex(/^[A-Za-z0-9 _-]+$/, 'nome del carattere non valido: ammessi lettere, cifre, spazi, - e _')

export const fontStackSchema = z.array(fontFamilySchema).min(1).max(8)

/** Fallback di sistema, aggiunti dal compilatore in coda a ogni stack. */
export const SYSTEM_FALLBACKS: Readonly<Record<'sans' | 'mono' | 'display', string>> = {
  sans: 'system-ui, -apple-system, sans-serif',
  mono: 'ui-monospace, SFMono-Regular, monospace',
  display: 'system-ui, sans-serif'
}

export function formatFontStack(families: readonly string[], kind: 'sans' | 'mono' | 'display'): string {
  const quoted = families.map((family) => `'${family}'`).join(', ')
  return `${quoted}, ${SYSTEM_FALLBACKS[kind]}`
}
