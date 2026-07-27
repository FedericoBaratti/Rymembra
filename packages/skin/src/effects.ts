/**
 * La libreria degli effetti: parametrica, e con un costo dichiarato.
 *
 * Gli effetti non sono inventati. Sono l'inventario di ciò che le tre skin
 * esistenti fanno davvero, riscritto come parametri invece che come CSS. Per
 * esempio, `--cyber-grid` nel legacy è:
 *
 *     linear-gradient(rgba(0,240,255,0.07) 1px, transparent 1px) 0 0 / 100% 36px,
 *     linear-gradient(90deg, rgba(0,240,255,0.07) 1px, transparent 1px) 0 0 / 36px 100%
 *
 * Due gradienti con lo stesso colore, la stessa opacità e lo stesso passo,
 * scritti due volte. Qui è `{ effect: 'hairlineGrid', color, cell: '36px' }`, e
 * la variante chiara della stessa skin — che nel legacy ripete le due righe con
 * un altro colore, e per una svista usa `36px 36px` invece di `100% 36px` —
 * diventa un colore diverso e nient'altro.
 *
 * **La classe di costo.** Ogni effetto dichiara quanto costa disegnarlo, e non è
 * documentazione: il budget della Fase 7 lo usa per rifiutare una skin che non
 * può stare nei 8ms a 120Hz, e lo Studio per avvisare mentre la si costruisce.
 * `global.css` porta già la nota scritta a mano «al massimo ~4 superfici con
 * backdrop-filter composte insieme»: qui quel limite diventa verificabile.
 */

import { z } from 'zod'
import { colorValueSchema } from './tokens'
import { lengthSchema, unitlessSchema } from './values'

/**
 * Quanto costa un effetto, per fotogramma.
 *
 *   cheap       una tinta o un gradiente: il compositore lo assorbe
 *   paint       un motivo ripetuto: ridisegna l'area a ogni cambio
 *   composited  richiede un livello proprio (transform, opacity animate)
 *   gpu         backdrop-filter o blur: costoso, e su WebView molto costoso
 */
export const COST_CLASSES = ['cheap', 'paint', 'composited', 'gpu'] as const
export type CostClass = (typeof COST_CLASSES)[number]

/** Peso relativo, per sommare il costo di una superficie. */
export const COST_WEIGHT: Readonly<Record<CostClass, number>> = {
  cheap: 1,
  paint: 3,
  composited: 4,
  gpu: 10
}

/**
 * Budget di una singola superficie.
 *
 * Dieci è un `backdrop-filter` da solo: è il numero che rende esplicita la nota
 * di `global.css`. Quattro superfici con blur composte insieme fanno 40, cioè
 * quattro volte il budget — che è precisamente il limite che quel commento
 * avvertiva di non superare.
 */
export const SURFACE_COST_BUDGET = 10

const stopSchema = z.object({
  color: colorValueSchema,
  /** Dove si trova la fermata. Assente = distribuita uniformemente. */
  at: z.union([lengthSchema, z.null()]).optional()
})

const stopsSchema = z.array(stopSchema).min(2).max(8)

/**
 * Gli effetti.
 *
 * Ognuno è un oggetto con `effect` come discriminante, così l'unione è chiusa e
 * un nome sconosciuto è un errore di validazione con il codice `skin.unknownEffect`
 * invece di un CSS silenziosamente vuoto.
 */
export const effectSchema = z.discriminatedUnion('effect', [
  z.object({
    effect: z.literal('solid'),
    color: colorValueSchema
  }),
  z.object({
    effect: z.literal('linearGradient'),
    /** Gradi. 180 = dall'alto in basso, come il default CSS. */
    angle: unitlessSchema.default(180),
    stops: stopsSchema
  }),
  z.object({
    effect: z.literal('radialGradient'),
    shape: z.enum(['circle', 'ellipse']).default('ellipse'),
    at: z.tuple([lengthSchema, lengthSchema]).optional(),
    size: lengthSchema.optional(),
    stops: stopsSchema
  }),
  z.object({
    effect: z.literal('conicGradient'),
    from: unitlessSchema.default(0),
    at: z.tuple([lengthSchema, lengthSchema]).optional(),
    stops: stopsSchema
  }),
  /**
   * La griglia a linee sottili sui due assi. Il `--cyber-grid` del legacy, con
   * il passo che diventa un parametro invece di essere ripetuto due volte.
   */
  z.object({
    effect: z.literal('hairlineGrid'),
    color: colorValueSchema,
    cell: lengthSchema,
    /** Passo verticale, se diverso da quello orizzontale. */
    cellY: lengthSchema.optional(),
    thickness: lengthSchema.optional()
  }),
  /** Le scanline CRT: `--cyber-scanline`. */
  z.object({
    effect: z.literal('scanlines'),
    color: colorValueSchema,
    /** Spessore della linea. */
    line: lengthSchema,
    /** Passo fra due linee. */
    gap: lengthSchema
  }),
  /** Le strisce hazard diagonali: `--cyber-hazard`. */
  z.object({
    effect: z.literal('stripes'),
    angle: unitlessSchema.default(-45),
    color: colorValueSchema,
    background: colorValueSchema,
    width: lengthSchema
  }),
  /** La matrice di punti della skin Nothing. */
  z.object({
    effect: z.literal('dotGrid'),
    color: colorValueSchema,
    spacing: lengthSchema,
    dot: lengthSchema
  }),
  /** Oscuramento ai bordi. */
  z.object({
    effect: z.literal('vignette'),
    color: colorValueSchema,
    /** Da dove comincia a scurire, in percentuale del raggio. */
    start: unitlessSchema.default(60)
  }),
  /**
   * Gli angoli tagliati HUD: `--cyber-chamfer`.
   *
   * Produce un `clip-path`, non uno sfondo. Il compilatore lo sa e lo emette
   * nella proprietà giusta — nel legacy erano due token distinti (`--cyber-cut`
   * per la misura e `--cyber-chamfer` per il poligono) da tenere coerenti.
   */
  z.object({
    effect: z.literal('chamfer'),
    size: lengthSchema,
    /** Quali angoli tagliare. Il default è la firma CP2077: alto-destra e basso-sinistra. */
    corners: z
      .array(z.enum(['topLeft', 'topRight', 'bottomRight', 'bottomLeft']))
      .min(1)
      .max(4)
      .default(['topRight', 'bottomLeft'])
  }),
  /** Sfocatura di ciò che sta sotto. L'effetto più costoso che esista qui. */
  z.object({
    effect: z.literal('blurBehind'),
    radius: lengthSchema,
    /** Saturazione, in percentuale. 100 = invariata. */
    saturate: unitlessSchema.optional()
  })
])

export type Effect = z.infer<typeof effectSchema>
export type EffectName = Effect['effect']

/** Il costo di ogni effetto. Esaustivo: un effetto nuovo senza costo non compila. */
export const EFFECT_COST: Readonly<Record<EffectName, CostClass>> = {
  solid: 'cheap',
  linearGradient: 'cheap',
  radialGradient: 'cheap',
  conicGradient: 'paint',
  hairlineGrid: 'paint',
  scanlines: 'paint',
  stripes: 'paint',
  dotGrid: 'paint',
  vignette: 'cheap',
  chamfer: 'composited',
  blurBehind: 'gpu'
}

/**
 * Quale proprietà CSS produce l'effetto.
 *
 * Serve al compilatore per non mettere un `clip-path` dentro un `background`, e
 * allo Studio per sapere quali effetti si possono comporre a livelli — solo
 * quelli che producono uno sfondo.
 */
export const EFFECT_TARGET: Readonly<Record<EffectName, 'background' | 'clipPath' | 'filter'>> = {
  solid: 'background',
  linearGradient: 'background',
  radialGradient: 'background',
  conicGradient: 'background',
  hairlineGrid: 'background',
  scanlines: 'background',
  stripes: 'background',
  dotGrid: 'background',
  vignette: 'background',
  chamfer: 'clipPath',
  filter: 'filter',
  blurBehind: 'filter'
} as Readonly<Record<EffectName, 'background' | 'clipPath' | 'filter'>>

export function effectCost(effect: Effect): CostClass {
  return EFFECT_COST[effect.effect]
}

/** Il costo sommato di una pila di livelli. */
export function stackCost(effects: readonly Effect[]): number {
  return effects.reduce((total, effect) => total + COST_WEIGHT[effectCost(effect)], 0)
}

export function exceedsBudget(effects: readonly Effect[]): boolean {
  return stackCost(effects) > SURFACE_COST_BUDGET
}
