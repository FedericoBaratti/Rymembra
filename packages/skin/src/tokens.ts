/**
 * Il registro dei token: il contratto fra le skin e i componenti.
 *
 * Questo contratto **esiste già**. `global.css` dichiara una sessantina di
 * proprietà personalizzate, i componenti le leggono, e le tre skin le
 * sovrascrivono sotto `:root[data-skin='<id>']`. Il problema non è che manchi: è
 * che non è scritto da nessuna parte. Per sapere quali token esistono si devono
 * leggere 3.341 righe di CSS in diciannove file, e per sapere quali sono
 * OBBLIGATORI non c'è modo — un token dimenticato non dà errore, dà una skin
 * visivamente rotta in un punto che può volerci un mese per notare.
 *
 * Qui il contratto è dati. Da qui si derivano: la validazione di una skin, i
 * pannelli dello Studio, il controllo di contrasto, e il CSS.
 *
 * Due cose che il registro fa e i file CSS non facevano.
 *
 * **I token derivati.** `--accent` e `--accent-rgb` sono lo stesso colore in due
 * forme, perché i canvas del visualizer leggono la tripla per costruire `rgba()`
 * a runtime. Nel legacy erano due dichiarazioni da tenere allineate a mano, e
 * `--cyber-fog-rgb` aveva perfino un commento in maiuscolo: «DEVE combaciare con
 * surface-0». Qui la tripla si deriva dal colore e non può divergere.
 *
 * **I token calcolati.** `--shell-left`, `--player-clearance` e
 * `--transition-fast` non sono scelte di stile, sono conseguenze: la prima è la
 * larghezza del rail, la seconda l'altezza del player più due volte il suo
 * margine, la terza una durata più una curva. Una skin non deve poterle
 * contraddire, quindi non sono token della skin — le emette il compilatore.
 */

import { z } from 'zod'
import {
  colorSchema,
  durationSchema,
  easingSchema,
  fontStackSchema,
  lengthSchema,
  lengthValueSchema,
  unitlessSchema
} from './values'

/** A cosa serve il token. Determina i pannelli dello Studio e i controlli. */
export type TokenGroup =
  | 'typography'
  | 'surface'
  | 'text'
  | 'accent'
  | 'status'
  | 'chrome'
  | 'layout'
  | 'geometry'
  | 'elevation'
  | 'motion'
  | 'canvas'

export type TokenKind = 'color' | 'length' | 'duration' | 'easing' | 'number' | 'fontStack' | 'shadow'

export interface TokenDef {
  /** Il nome della proprietà CSS. Viene dal legacy e NON si cambia a piacere. */
  readonly css: string
  readonly kind: TokenKind
  readonly group: TokenGroup
  /**
   * Il token è indispensabile: una skin che non lo definisce eredita quello di
   * base. Serve a distinguere "non l'ho scelto" da "l'ho scelto uguale".
   */
  readonly required: boolean
  /**
   * Emette anche la tripla `r g b` sotto questo nome. Solo per i colori che i
   * canvas leggono a runtime.
   */
  readonly rgbTriple?: string
  readonly description: string
}

/**
 * Ogni token, con il suo nome CSS legacy.
 *
 * L'ordine è quello dei pannelli dello Studio, non alfabetico: si sceglie un
 * colore di superficie prima di scegliere un'ombra.
 */
export const TOKENS = {
  // ── Tipografia ────────────────────────────────────────────────────────────
  'font.sans': {
    css: '--font-sans',
    kind: 'fontStack',
    group: 'typography',
    required: true,
    description: 'Carattere del corpo del testo.'
  },
  'font.mono': {
    css: '--font-mono',
    kind: 'fontStack',
    group: 'typography',
    required: false,
    description: 'Carattere a spaziatura fissa: durate, etichette tecniche.'
  },
  'font.display': {
    // Si chiama --font-dot per ragioni storiche: è nato nella skin Nothing per il
    // suo carattere a matrice di punti. I componenti lo leggono con questo nome,
    // quindi resta questo.
    css: '--font-dot',
    kind: 'fontStack',
    group: 'typography',
    required: false,
    description: 'Carattere da display: titoli grandi, numeri, eyebrow.'
  },

  // ── Superfici ─────────────────────────────────────────────────────────────
  'color.surface.0': {
    css: '--color-surface-0',
    kind: 'color',
    group: 'surface',
    required: true,
    description: 'Il fondo dell\'applicazione. È anche il colore dell\'avvio a freddo.'
  },
  'color.surface.1': {
    css: '--color-surface-1',
    kind: 'color',
    group: 'surface',
    required: true,
    description: 'Pannelli e barre.'
  },
  'color.surface.2': {
    css: '--color-surface-2',
    kind: 'color',
    group: 'surface',
    required: true,
    description: 'Schede e righe.'
  },
  'color.surface.3': {
    css: '--color-surface-3',
    kind: 'color',
    group: 'surface',
    required: true,
    description: 'Elementi sollevati, stati attivi.'
  },

  // ── Testo ─────────────────────────────────────────────────────────────────
  'color.text.1': {
    css: '--color-text-1',
    kind: 'color',
    group: 'text',
    required: true,
    description: 'Testo primario.'
  },
  'color.text.2': {
    css: '--color-text-2',
    kind: 'color',
    group: 'text',
    required: true,
    description: 'Testo secondario. Il controllo di contrasto guarda soprattutto questo.'
  },
  'color.text.3': {
    css: '--color-text-3',
    kind: 'color',
    group: 'text',
    required: true,
    description: 'Testo terziario, al limite della leggibilità: va verificato.'
  },

  // ── Accento ───────────────────────────────────────────────────────────────
  'color.accent': {
    css: '--accent',
    kind: 'color',
    group: 'accent',
    required: true,
    rgbTriple: '--accent-rgb',
    description: 'Accento primario. Può seguire la copertina in riproduzione.'
  },
  'color.accent.soft': {
    css: '--accent-soft',
    kind: 'color',
    group: 'accent',
    required: false,
    description: 'Accento a bassa opacità: sfondi di stato attivo.'
  },
  'color.accent.glow': {
    css: '--accent-glow',
    kind: 'color',
    group: 'accent',
    required: false,
    description: 'Accento per gli aloni.'
  },
  'color.accent.like': {
    css: '--accent-like',
    kind: 'color',
    group: 'accent',
    required: false,
    description: 'Riempimento del cuore. Nothing lo porta al rosso, il suo unico rosso.'
  },
  'color.hero': {
    // Solo tripla: i gradienti degli hero la usano dentro rgba() calcolate a
    // runtime dalla copertina.
    css: '--hero-rgb',
    kind: 'color',
    group: 'accent',
    required: false,
    rgbTriple: '--hero-rgb',
    description: 'Tinta degli hero, di norma derivata dalla copertina.'
  },

  // ── Semantici ─────────────────────────────────────────────────────────────
  'color.danger': {
    css: '--danger',
    kind: 'color',
    group: 'status',
    required: true,
    description: 'Errori e azioni distruttive.'
  },
  'color.danger.soft': {
    css: '--danger-soft',
    kind: 'color',
    group: 'status',
    required: false,
    description: 'Sfondo degli errori.'
  },
  'color.success': {
    css: '--success',
    kind: 'color',
    group: 'status',
    required: true,
    description: 'Conferme e operazioni riuscite.'
  },
  'color.success.soft': {
    css: '--success-soft',
    kind: 'color',
    group: 'status',
    required: false,
    description: 'Sfondo delle conferme.'
  },
  'color.warning': {
    css: '--warning',
    kind: 'color',
    group: 'status',
    required: true,
    description: 'Avvisi che non bloccano l\'operazione.'
  },
  'color.warning.soft': {
    css: '--warning-soft',
    kind: 'color',
    group: 'status',
    required: false,
    description: 'Sfondo degli avvisi.'
  },

  // ── Chrome ────────────────────────────────────────────────────────────────
  'color.sidebar': {
    css: '--sidebar-bg',
    kind: 'color',
    group: 'chrome',
    required: false,
    description: 'Fondo della barra laterale.'
  },
  'color.hairline': {
    css: '--hairline',
    kind: 'color',
    group: 'chrome',
    required: true,
    description: 'Le linee da un pixel che separano le superfici.'
  },
  'color.ambient.1': {
    css: '--ambient-1',
    kind: 'color',
    group: 'chrome',
    required: false,
    description: 'Primo alone dello sfondo ambientale.'
  },
  'color.ambient.2': {
    css: '--ambient-2',
    kind: 'color',
    group: 'chrome',
    required: false,
    description: 'Secondo alone dello sfondo ambientale.'
  },

  // ── Layout ────────────────────────────────────────────────────────────────
  'layout.rail': {
    css: '--rail-w',
    kind: 'length',
    group: 'layout',
    required: false,
    description: 'Larghezza della barra laterale chiusa.'
  },
  'layout.railExpanded': {
    css: '--rail-w-expanded',
    kind: 'length',
    group: 'layout',
    required: false,
    description: 'Larghezza della barra laterale aperta.'
  },
  'layout.playerHeight': {
    css: '--player-h',
    kind: 'length',
    group: 'layout',
    required: false,
    description: 'Altezza della barra del player.'
  },
  'layout.playerGap': {
    css: '--player-gap',
    kind: 'length',
    group: 'layout',
    required: false,
    description: 'Margine attorno al player flottante.'
  },
  'layout.contentX': {
    css: '--content-x',
    kind: 'length',
    group: 'layout',
    required: false,
    description: 'Margine orizzontale del contenuto.'
  },

  // ── Geometria ─────────────────────────────────────────────────────────────
  'radius.panel': {
    css: '--radius-panel',
    kind: 'length',
    group: 'geometry',
    required: true,
    description: 'Raggio dei pannelli. Cyberpunk lo porta a 4px, ed è metà della sua identità.'
  },
  'radius.card': {
    css: '--radius-card',
    kind: 'length',
    group: 'geometry',
    required: true,
    description: 'Raggio delle schede.'
  },

  // ── Elevazione ────────────────────────────────────────────────────────────
  'shadow.1': {
    css: '--shadow-1',
    kind: 'shadow',
    group: 'elevation',
    required: true,
    description: 'Elevazione bassa.'
  },
  'shadow.2': {
    css: '--shadow-2',
    kind: 'shadow',
    group: 'elevation',
    required: true,
    description: 'Elevazione media.'
  },
  'shadow.3': {
    css: '--shadow-3',
    kind: 'shadow',
    group: 'elevation',
    required: true,
    description: 'Elevazione alta: overlay e finestre.'
  },
  'shadow.player': {
    css: '--shadow-player',
    kind: 'shadow',
    group: 'elevation',
    required: false,
    description: 'Elevazione del player flottante, con la sua hairline interna.'
  },
  'glow.accent': {
    css: '--glow-accent',
    kind: 'shadow',
    group: 'elevation',
    required: false,
    description: 'Alone d\'accento riusabile.'
  },

  // ── Motion ────────────────────────────────────────────────────────────────
  'motion.ease.outExpo': {
    css: '--ease-out-expo',
    kind: 'easing',
    group: 'motion',
    required: true,
    description: 'Curva di uscita principale. La usano anche le transizioni di rotta.'
  },
  'motion.ease.spring': {
    css: '--ease-spring',
    kind: 'easing',
    group: 'motion',
    required: false,
    description: 'Curva con rimbalzo.'
  },
  'motion.dur.1': {
    css: '--dur-1',
    kind: 'duration',
    group: 'motion',
    required: true,
    description: 'Durata breve: hover, stati.'
  },
  'motion.dur.2': {
    css: '--dur-2',
    kind: 'duration',
    group: 'motion',
    required: true,
    description: 'Durata media: pannelli, entrate.'
  },
  'motion.dur.3': {
    css: '--dur-3',
    kind: 'duration',
    group: 'motion',
    required: false,
    description: 'Durata lunga: overlay a schermo intero.'
  },

  // ── Canvas ────────────────────────────────────────────────────────────────
  // Questi non sono decorazione: sono il contratto che permette al visualizer e
  // allo scrubber, che disegnano su canvas in JS, di seguire la skin senza una
  // riga di codice skin-aware.
  'canvas.viz.primary': {
    css: '--viz-primary',
    kind: 'color',
    group: 'canvas',
    required: false,
    rgbTriple: '--viz-primary-rgb',
    description: 'Barre dello spettro nel visualizer.'
  },
  'canvas.viz.secondary': {
    css: '--viz-secondary',
    kind: 'color',
    group: 'canvas',
    required: false,
    rgbTriple: '--viz-secondary-rgb',
    description: 'Anello dei bassi nel visualizer.'
  },
  'canvas.viz.glow': {
    css: '--viz-glow',
    kind: 'number',
    group: 'canvas',
    required: false,
    description: 'Raggio dell\'alone del visualizer, in pixel. Numero nudo: lo legge il JS.'
  },
  'canvas.scrubber.glow': {
    css: '--scrubber-glow',
    kind: 'number',
    group: 'canvas',
    required: false,
    description: 'Alone del playhead. Le skin piatte lo azzerano.'
  },
  'canvas.scrubber.rest': {
    css: '--scrubber-rest',
    kind: 'color',
    group: 'canvas',
    required: false,
    description: 'Onda non ancora riprodotta nello scrubber.'
  }
} as const satisfies Record<string, TokenDef>

export type TokenId = keyof typeof TOKENS

export const TOKEN_IDS = Object.keys(TOKENS) as TokenId[]

export const REQUIRED_TOKEN_IDS = TOKEN_IDS.filter((id) => TOKENS[id].required)

export function tokenDef(id: TokenId): TokenDef {
  return TOKENS[id]
}

/** Se una stringa qualunque è un token conosciuto. Usata dai riferimenti. */
export function isTokenId(value: string): value is TokenId {
  return Object.prototype.hasOwnProperty.call(TOKENS, value)
}

/** I token di un gruppo, per costruire i pannelli dello Studio dai dati. */
export function tokensInGroup(group: TokenGroup): TokenId[] {
  return TOKEN_IDS.filter((id) => TOKENS[id].group === group)
}

// ── Riferimenti e sorgenti dinamiche ────────────────────────────────────────

/**
 * Le sorgenti da cui un token può prendere il valore a runtime.
 *
 * Sostituisce il flag `supportsDynamicAccent` che nel legacy era un booleano
 * nell'oggetto della skin: con una sorgente per token si può dire QUALE token
 * segue la copertina, invece di accendere o spegnere il meccanismo in blocco.
 */
export const DYNAMIC_SOURCES = [
  'albumArt.vibrant',
  'albumArt.muted',
  'albumArt.darkVibrant',
  'albumArt.lightVibrant'
] as const
export type DynamicSource = (typeof DYNAMIC_SOURCES)[number]

/** Riferimento a un altro token: `--accent-like: var(--accent)` nel legacy. */
export const tokenRefSchema = z.object({
  $token: z.string().refine(isTokenId, {
    message: 'riferimento a un token che non esiste nel registro'
  })
})

export const dynamicSourceSchema = z.object({
  $source: z.enum(DYNAMIC_SOURCES),
  /** Opacità da applicare alla tinta estratta. Serve ai token `*-soft`/`*-glow`. */
  alpha: z.number().min(0).max(1).optional()
})

/**
 * Un colore: letterale, riferimento a un altro token, o legato alla copertina.
 *
 * Scritto come dispatch a mano e non come `z.union([...])` per una ragione
 * concreta: con l'unione, zod riporta «Invalid input» e il messaggio preciso
 * («colore non valido: blu. Ammessi #rgb, …») viene inghiottito. Ma il messaggio
 * preciso è tutto il valore di questo strato — chi crea una skin deve sapere COSA
 * rifiutare. Riconoscendo prima la forma e validando poi, ogni ramo tiene il suo
 * messaggio.
 */
export const colorValueSchema = z.unknown().transform((raw, ctx) => {
  if (typeof raw === 'object' && raw !== null && '$token' in raw) {
    const parsed = tokenRefSchema.safeParse(raw)
    if (!parsed.success) {
      ctx.addIssue({
        code: 'custom',
        message: parsed.error.issues[0]?.message ?? 'riferimento a token non valido'
      })
      return z.NEVER
    }
    return parsed.data
  }

  if (typeof raw === 'object' && raw !== null && '$source' in raw) {
    const parsed = dynamicSourceSchema.safeParse(raw)
    if (!parsed.success) {
      ctx.addIssue({
        code: 'custom',
        message: parsed.error.issues[0]?.message ?? 'sorgente dinamica non valida'
      })
      return z.NEVER
    }
    return parsed.data
  }

  if (typeof raw === 'string') {
    const parsed = colorSchema.safeParse(raw)
    if (!parsed.success) {
      ctx.addIssue({
        code: 'custom',
        message: parsed.error.issues[0]?.message ?? `colore non valido: ${raw}`
      })
      return z.NEVER
    }
    return parsed.data
  }

  ctx.addIssue({
    code: 'custom',
    message:
      'un colore va scritto come stringa (#rrggbb, rgba(...)), come { "$token": … } o come { "$source": … }'
  })
  return z.NEVER
})

export type ColorValue =
  | z.infer<typeof tokenRefSchema>
  | z.infer<typeof dynamicSourceSchema>
  | z.infer<typeof colorSchema>

// ── Ombre ───────────────────────────────────────────────────────────────────

/**
 * Un'ombra come lista di livelli, non come stringa.
 *
 * Le ombre reali del progetto hanno fino a tre livelli e mescolano `inset` con
 * ombre esterne: `--shadow-player` di plain è `inset 0 1px 0 rgba(...), 0 8px
 * 40px rgba(...)` — una hairline interna in alto più una caduta profonda. Con la
 * struttura, lo Studio può mostrare un editor per livello invece di un campo di
 * testo in cui si può scrivere qualunque cosa.
 */
export const shadowLayerSchema = z.object({
  inset: z.boolean().optional(),
  x: lengthSchema,
  y: lengthSchema,
  blur: lengthSchema,
  spread: lengthSchema.optional(),
  color: colorValueSchema
})

export const shadowValueSchema = z.object({
  // Zero livelli è legittimo e significa `none`: è così che le skin piatte
  // spengono un'ombra senza doverne inventare una trasparente.
  layers: z.array(shadowLayerSchema).max(6)
})

export type ShadowValue = z.infer<typeof shadowValueSchema>

// ── Lo schema di un valore, per token ───────────────────────────────────────

/**
 * Lo schema del valore che un token accetta, dedotto dal suo `kind`.
 *
 * È il pezzo che rende il registro utile: la validazione di una skin non è una
 * lista scritta a mano di sessanta campi, è generata da qui. Aggiungere un token
 * al registro lo rende automaticamente valido, editabile e compilabile.
 */
export function schemaForKind(kind: TokenKind): z.ZodTypeAny {
  switch (kind) {
    case 'color':
      return colorValueSchema
    case 'length':
      // lengthValue e non length: i token di layout possono essere adattivi, ed è
      // emerso convertendo `plain`, dove --content-x è un clamp().
      return lengthValueSchema
    case 'duration':
      return durationSchema
    case 'easing':
      return easingSchema
    case 'number':
      return unitlessSchema
    case 'fontStack':
      return fontStackSchema
    case 'shadow':
      return shadowValueSchema
  }
}

/**
 * Lo schema dell'intero blocco `tokens`, con ogni chiave opzionale.
 *
 * Il tipo di uscita è `Record<string, unknown>` e non la forma esatta: lo shape è
 * costruito a runtime dal registro, quindi TypeScript non può dedurlo. Non è una
 * perdita — il compilatore ricava il tipo di ogni valore dal `kind` del token, che
 * è la stessa fonte da cui è stato validato.
 */
export function buildTokensSchema(): z.ZodType<Record<string, unknown>> {
  const shape: Record<string, z.ZodTypeAny> = {}
  for (const id of TOKEN_IDS) {
    shape[id] = schemaForKind(TOKENS[id].kind).optional()
  }
  // strict: una chiave sconosciuta è un errore, non silenzio. Un token scritto
  // male è il modo più facile per ottenere una skin rotta in un punto solo.
  return z.object(shape).strict() as unknown as z.ZodType<Record<string, unknown>>
}
