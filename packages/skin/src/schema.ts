/**
 * Lo schema di `skin.json`.
 *
 * Tutto è `strict()`: una chiave che non conosciamo è un errore, mai silenzio. È
 * la scelta opposta a quella comoda, e il motivo è che nel legacy una skin era
 * CSS — un token scritto male non dava errore, dava una skin visivamente rotta in
 * un punto solo, e trovarlo richiedeva di accorgersene guardando.
 *
 * La forma del documento segue le decisioni prese: token, temi, piattaforme,
 * motion, layout, motivi. Nessun campo accetta CSS.
 */

import { z } from 'zod'
import { effectSchema } from './effects'
import { skinPartsSchema } from './parts'
import { DYNAMIC_SOURCES, buildTokensSchema, skinPaletteSchema } from './tokens'
import { easingSchema } from './values'

/** La versione del formato. Un numero, non semver: cambia solo se rompiamo. */
export const SKIN_FORMAT_VERSION = 1

/**
 * L'identificatore di una skin.
 *
 * Vincolato perché finisce in un selettore CSS (`:root[data-skin='<id>']`), in un
 * nome di file e in un URL della LAN. Un id con una virgoletta o una barra
 * romperebbe uno dei tre, e proprio quello è il tipo di fuga che il formato deve
 * rendere impossibile.
 */
export const skinIdSchema = z
  .string()
  .min(2)
  .max(48)
  .regex(/^[a-z][a-z0-9-]*$/, 'l\'id ammette minuscole, cifre e trattini, e inizia con una lettera')

export const skinMetaSchema = z
  .object({
    name: z.string().min(1).max(64),
    author: z.string().min(1).max(64),
    /** Versione della skin, semver-like. Serve all'allineamento fra PC e telefono. */
    version: z
      .string()
      .regex(/^\d+\.\d+\.\d+$/, 'la versione va scritta come 1.0.0'),
    description: z.string().max(280).optional(),
    license: z.string().max(64).optional(),
    /** Da quale skin è stata derivata, quando è un fork. */
    basedOn: skinIdSchema.optional()
  })
  .strict()

/**
 * Cosa la skin dichiara di saper fare.
 *
 * Sostituisce `supportsDynamicAccent`, che nel legacy era un booleano
 * nell'oggetto `SKINS` di `src/lib/skins.ts` — accendeva o spegneva in blocco il
 * legame con la copertina. Qui il legame si dichiara per token (`$source`), e
 * questo blocco resta solo per ciò che riguarda la skin nel suo complesso.
 */
export const skinCapabilitiesSchema = z
  .object({
    /** Ha una variante chiara. Se falsa, l'interruttore tema non si mostra. */
    light: z.boolean().default(false),
    /** Ha sovrascritture pensate per lo schermo di un telefono. */
    mobile: z.boolean().default(false),
    /**
     * Va bene che i colori seguano la copertina. Una skin con una palette
     * fissa e voluta — Nothing è bianco e nero per scelta — dice falso.
     */
    dynamicAccent: z.boolean().default(true)
  })
  .strict()

/**
 * L'intensità del movimento.
 *
 * Si COMPONE con `prefers-reduced-motion`, non lo sovrascrive: il sistema
 * operativo ha sempre l'ultima parola verso il basso. I tre blocchi CSS
 * reduced-motion esistenti restano il pavimento.
 */
export const MOTION_INTENSITIES = ['none', 'essential', 'full', 'maximum'] as const
export type MotionIntensity = (typeof MOTION_INTENSITIES)[number]

export const routeTransitionSchema = z
  .object({
    /** Come esce la vista uscente. */
    out: z
      .object({
        opacity: z.number().min(0).max(1).optional(),
        scale: z.number().min(0.5).max(1.5).optional(),
        translateY: z.number().min(-100).max(100).optional()
      })
      .strict()
      .optional(),
    in: z
      .object({
        opacity: z.number().min(0).max(1).optional(),
        scale: z.number().min(0.5).max(1.5).optional(),
        translateY: z.number().min(-100).max(100).optional()
      })
      .strict()
      .optional()
  })
  .strict()

export const skinMotionSchema = z
  .object({
    intensity: z.enum(MOTION_INTENSITIES).default('full'),
    /** Curve aggiuntive della skin, oltre a quelle del registro token. */
    easings: z.record(z.string().regex(/^[a-z][a-zA-Z0-9]*$/), easingSchema).optional(),
    routeTransition: routeTransitionSchema.optional()
  })
  .strict()

export const skinLayoutSchema = z
  .object({
    player: z.enum(['bottom-bar', 'floating', 'compact']).default('floating'),
    sidebar: z.enum(['rail', 'expanded', 'hidden']).default('rail'),
    density: z.enum(['compact', 'comfortable', 'spacious']).default('comfortable')
  })
  .strict()

/**
 * I motivi: effetti nominati dalla skin, riusabili.
 *
 * È il posto dei token skin-locali del legacy (`--cyber-grid`,
 * `--cyber-scanline`, `--cyber-hazard`, `--cyber-chamfer`). Il compilatore li
 * emette come proprietà personalizzate con un prefisso, quindi non possono
 * collidere né fra skin né coi token del registro.
 */
export const patternNameSchema = z
  .string()
  .min(1)
  .max(40)
  .regex(/^[a-z][a-z0-9-]*$/, 'il nome di un motivo ammette minuscole, cifre e trattini')

export const skinPatternsSchema = z.record(patternNameSchema, effectSchema)

/** Sovrascritture per un tema o una piattaforma: solo token, niente struttura. */
const tokenOverridesSchema = buildTokensSchema()

export const skinDocumentSchema = z
  .object({
    format: z.literal(SKIN_FORMAT_VERSION),
    id: skinIdSchema,
    meta: skinMetaSchema,
    capabilities: skinCapabilitiesSchema.default({
      light: false,
      mobile: false,
      dynamicAccent: true
    }),
    /**
     * I colori locali della skin, nominati.
     *
     * Sono i token skin-locali del legacy: `--cyber-teal`, `--cyber-red`,
     * `--nothing-red`. Non fanno parte del contratto con i componenti — nessun
     * componente li legge — ma servono alla skin per non ripetere lo stesso valore
     * in venti dichiarazioni.
     */
    palette: skinPaletteSchema.optional(),
    tokens: tokenOverridesSchema,
    /** Sovrascritture per `[data-theme='light']`. */
    themes: z
      .object({ light: tokenOverridesSchema.optional() })
      .strict()
      .optional(),
    /** Sovrascritture per `[data-mobile]`. */
    platforms: z
      .object({ mobile: tokenOverridesSchema.optional() })
      .strict()
      .optional(),
    motion: skinMotionSchema.optional(),
    layout: skinLayoutSchema.optional(),
    patterns: skinPatternsSchema.optional(),
    /**
     * Le superfici ridisegnate, per nome di parte.
     *
     * È il livello che nel legacy occupava la quasi totalità del CSS di una skin:
     * 1.084 righe su 1.165 per `nothing`, 1.709 su 1.879 per `cyberpunk`. Un nome
     * di parte inesistente è un errore di validazione, dove prima era un selettore
     * che non combaciava con niente e non lo diceva.
     */
    parts: skinPartsSchema.optional()
  })
  .strict()

export type SkinDocument = z.infer<typeof skinDocumentSchema>
export type SkinTokens = SkinDocument['tokens']

export { DYNAMIC_SOURCES }
