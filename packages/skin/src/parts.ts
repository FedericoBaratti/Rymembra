/**
 * Il parts registry: le superfici che una skin può ridisegnare.
 *
 * Le classi elencate qui sotto **esistono già** e le skin le usano: `nothing` ne
 * aggancia 72, `cyberpunk` 90. Il problema è che la maggior parte di esse **non
 * ha alcuna definizione di base**: esistono solo come agganci, messe nei
 * componenti perché una skin potesse afferrarle, e non sono documentate da
 * nessuna parte. Le ho estratte dai selettori dei diciannove file CSS delle skin.
 *
 * Le conseguenze pratiche di quel silenzio, entrambe reali:
 *
 *   1. rinominare una classe in un componente rompe silenziosamente due skin, e
 *      il tipo di rottura è «un pannello non ha più il bordo giusto», che si nota
 *      settimane dopo;
 *   2. chi crea una skin non ha modo di sapere quali agganci esistono, quindi ne
 *      scopre alcuni leggendo il CSS di quelle già fatte, e altri mai.
 *
 * Con il registro, la prima diventa un errore di compilazione — i componenti
 * passano da `part('section-card')` — e la seconda diventa un elenco che lo
 * Studio può mostrare.
 *
 * Il vocabolario di ciò che una skin può cambiare per parte è deliberatamente
 * corto. Non è CSS: sono sfondo, bordo, ritaglio, colore del testo, spaziatura,
 * più gli stati. Basta a riprodurre le due skin esistenti e non basta a rompere
 * il layout — che è precisamente il confine giusto, perché una skin che può
 * spostare le cose può anche renderle inaccessibili.
 */

import { z } from 'zod'
import { effectSchema, type Effect } from './effects'
import { colorValueSchema } from './tokens'
import { lengthSchema, unitlessSchema } from './values'

export type PartGroup =
  | 'shell'
  | 'nav'
  | 'page'
  | 'controls'
  | 'lists'
  | 'player'
  | 'nowPlaying'
  | 'overlays'

export interface PartDef {
  readonly group: PartGroup
  readonly description: string
  /**
   * La parte ha uno pseudo-elemento disponibile per un livello aggiuntivo.
   *
   * Conta perché è così che le due skin fanno gli effetti sovrapposti — le
   * scanline CRT di cyberpunk sono un `body::after`, la griglia dot-matrix di
   * nothing un `::before` su `.ambient-backdrop`. Dove lo pseudo-elemento è già
   * usato dal componente per altro, questo è falso e il livello viene rifiutato.
   */
  readonly layers: boolean
}

/**
 * Le parti. Il nome è la classe CSS, senza il punto.
 *
 * Non si rinominano: sono nel markup dei componenti e nelle skin già scritte.
 */
export const PARTS = {
  // ── shell ─────────────────────────────────────────────────────────────────
  'app-shell': {
    group: 'shell',
    description: 'Il contenitore di tutta la finestra.',
    layers: false
  },
  'ambient-backdrop': {
    group: 'shell',
    description:
      'Il fondo dietro al contenuto. È qui che vivono griglie, pavimenti prospettici e campi di punti.',
    layers: true
  },
  'player-shell': {
    group: 'player',
    description: 'La barra del player flottante.',
    layers: true
  },
  'bottom-nav': {
    group: 'nav',
    description: 'La navigazione inferiore, su schermi stretti.',
    layers: true
  },
  'nav-pill': {
    group: 'nav',
    description: 'La voce di navigazione, compreso lo stato attivo.',
    layers: false
  },

  // ── pagina ────────────────────────────────────────────────────────────────
  'page-header': { group: 'page', description: 'L\'intestazione di una pagina.', layers: false },
  'page-title': { group: 'page', description: 'Il titolo grande di una pagina.', layers: false },
  'page-subtitle': {
    group: 'page',
    description: 'Il sottotitolo sotto il titolo di pagina.',
    layers: false
  },
  'hero-eyebrow': {
    group: 'page',
    description: 'L\'etichetta piccola sopra un titolo hero, di norma in maiuscole.',
    layers: false
  },
  'hero-art': { group: 'page', description: 'L\'immagine grande di un hero.', layers: true },
  'section-card': {
    group: 'page',
    description: 'La scheda che contiene una sezione. La superficie più riusata dell\'app.',
    layers: true
  },
  'section-heading': {
    group: 'page',
    description: 'Il titolo di una sezione dentro una pagina.',
    layers: false
  },
  'section-icon': {
    group: 'page',
    description: 'L\'icona accanto al titolo di sezione.',
    layers: false
  },
  'stat-number': {
    group: 'page',
    description: 'Un numero grande nelle statistiche.',
    layers: false
  },

  // ── controlli ─────────────────────────────────────────────────────────────
  'icon-btn': { group: 'controls', description: 'Il pulsante con la sola icona.', layers: false },
  'play-btn-primary': {
    group: 'controls',
    description: 'Il pulsante di riproduzione principale.',
    layers: true
  },
  'btn-accent': { group: 'controls', description: 'Il pulsante d\'azione primaria.', layers: false },
  'btn-ghost': { group: 'controls', description: 'Il pulsante secondario, senza fondo.', layers: false },
  switch: { group: 'controls', description: 'L\'interruttore, nel suo insieme.', layers: false },
  'switch-track': { group: 'controls', description: 'La pista dell\'interruttore.', layers: false },
  'field-input': { group: 'controls', description: 'Il campo di testo.', layers: false },
  'range-accent': { group: 'controls', description: 'Il cursore a scorrimento.', layers: false },
  'tooltip-pill': { group: 'controls', description: 'Il suggerimento al passaggio.', layers: false },

  // ── elenchi ───────────────────────────────────────────────────────────────
  'track-grid': { group: 'lists', description: 'La griglia delle tracce o degli album.', layers: false },
  'queue-list': { group: 'lists', description: 'La coda di riproduzione.', layers: false },
  'home-shortcuts': { group: 'lists', description: 'Le scorciatoie della schermata iniziale.', layers: false },
  'empty-state': { group: 'lists', description: 'Il riquadro mostrato quando non c\'è niente.', layers: true },
  'empty-icon': { group: 'lists', description: 'L\'icona dello stato vuoto.', layers: false },
  skeleton: {
    group: 'lists',
    description: 'Il segnaposto durante il caricamento. La sua animazione è parte dell\'identità della skin.',
    layers: true
  },

  // ── player ────────────────────────────────────────────────────────────────
  'player-progress': { group: 'player', description: 'La barra di avanzamento del player.', layers: true },
  'progress-sheen': {
    group: 'player',
    description: 'Il riflesso che scorre sulla barra di avanzamento.',
    layers: false
  },

  // ── in riproduzione ───────────────────────────────────────────────────────
  'np-screen': { group: 'nowPlaying', description: 'La schermata In riproduzione.', layers: true },
  'np-art': { group: 'nowPlaying', description: 'La copertina in grande.', layers: true },
  'np-title': { group: 'nowPlaying', description: 'Il titolo del brano in riproduzione.', layers: false },
  'np-meta': { group: 'nowPlaying', description: 'Artista e album sotto il titolo.', layers: false },
  'np-transport': { group: 'nowPlaying', description: 'I comandi di riproduzione.', layers: false },
  'np-scrim': { group: 'nowPlaying', description: 'Il velo sopra la copertina, per leggere il testo.', layers: false },
  'lyrics-screen': { group: 'nowPlaying', description: 'La schermata del testo.', layers: true },
  'lyric-line': { group: 'nowPlaying', description: 'La riga di testo, attiva e non.', layers: false },
  'viz-screen': { group: 'nowPlaying', description: 'La schermata del visualizer.', layers: true },
  'viz-title': { group: 'nowPlaying', description: 'Il titolo sopra il visualizer.', layers: false },
  'eq-bars': { group: 'nowPlaying', description: 'Le barre dell\'equalizzatore.', layers: false },
  'eq-slider': { group: 'nowPlaying', description: 'Il cursore di una banda dell\'equalizzatore.', layers: false },

  // ── sovrapposizioni ───────────────────────────────────────────────────────
  'glass-modal': { group: 'overlays', description: 'La finestra modale.', layers: true },
  'menu-pop': { group: 'overlays', description: 'Il menu contestuale.', layers: true },
  'toast-card': { group: 'overlays', description: 'La notifica temporanea.', layers: true },
  'toast-progress': { group: 'overlays', description: 'La barra di durata di una notifica.', layers: false },
  'tour-tooltip': { group: 'overlays', description: 'Il fumetto della presentazione guidata.', layers: true }
} as const satisfies Record<string, PartDef>

export type PartName = keyof typeof PARTS

export const PART_NAMES = Object.keys(PARTS) as PartName[]

export function partDef(name: PartName): PartDef {
  return PARTS[name]
}

export function isPartName(value: string): value is PartName {
  return Object.prototype.hasOwnProperty.call(PARTS, value)
}

export function partsInGroup(group: PartGroup): PartName[] {
  return PART_NAMES.filter((name) => PARTS[name].group === group)
}

/**
 * La classe di una parte, per i componenti.
 *
 * `part('section-card')` invece di `"section-card"`: un nome inesistente non
 * compila, e rinominare una parte diventa un errore in tutti i punti d'uso invece
 * di due skin rotte in silenzio.
 */
export function part(name: PartName): string {
  return name
}

/** Più parti insieme, per i componenti che ne combinano. */
export function parts(...names: PartName[]): string {
  return names.join(' ')
}

// ── Cosa una skin può cambiare, per parte ───────────────────────────────────

/**
 * Gli stati.
 *
 * Solo questi quattro, e sono quelli che le due skin usano davvero. `focus` è
 * `focus-visible` e non `focus`: lo stato deve apparire per chi naviga da
 * tastiera e non a ogni clic del mouse — nel legacy le skin lo scrivevano
 * correttamente, e vale conservarlo per costruzione invece che per disciplina.
 */
export const PART_STATES = ['hover', 'active', 'focus', 'disabled'] as const
export type PartState = (typeof PART_STATES)[number]

const STATE_SELECTOR: Readonly<Record<PartState, string>> = {
  hover: ':hover',
  active: '[data-active], .is-active',
  focus: ':focus-visible',
  disabled: ':disabled, [aria-disabled="true"]'
}

const textTransformSchema = z.enum(['none', 'uppercase', 'lowercase', 'capitalize'])

/**
 * Le proprietà che una skin può dichiarare su una parte.
 *
 * Cosa NON c'è, e per scelta: `width`, `height`, `margin`, `padding`, `position`,
 * `display`. Una skin che può spostare le cose può anche sovrapporle, nasconderle
 * o portarle fuori dallo schermo — e il risultato non sarebbe una skin brutta, ma
 * un'app inutilizzabile che sembra un bug dell'app.
 */
const partAppearanceSchema = z
  .object({
    /** Livelli di sfondo, dal più basso. Il costo sommato è soggetto al budget. */
    background: z.array(effectSchema).max(4).optional(),
    textColor: colorValueSchema.optional(),
    borderColor: colorValueSchema.optional(),
    borderWidth: lengthSchema.optional(),
    radius: lengthSchema.optional(),
    /** Un effetto `chamfer` per gli angoli tagliati. */
    clip: effectSchema.optional(),
    shadow: z.array(effectSchema).max(0).optional(),
    opacity: z.number().min(0).max(1).optional(),
    letterSpacing: lengthSchema.optional(),
    textTransform: textTransformSchema.optional(),
    /** Peso del carattere, per le parti che la skin vuole più marcate. */
    fontWeight: unitlessSchema.optional()
  })
  .strict()

export const partStyleSchema = partAppearanceSchema
  .extend({
    /** Un livello aggiuntivo su ::after. Ammesso solo dove `layers` è vero. */
    layer: z
      .object({
        background: z.array(effectSchema).min(1).max(3),
        opacity: z.number().min(0).max(1).optional()
      })
      .strict()
      .optional(),
    /**
     * Gli stati, come oggetto esplicito e non `z.record` su un enum: in zod v4
     * un record con chiavi enumerate le richiede TUTTE, quindi una skin che
     * dichiara solo `hover` verrebbe rifiutata per i tre stati che non le
     * interessano.
     */
    states: z
      .object({
        hover: partAppearanceSchema.optional(),
        active: partAppearanceSchema.optional(),
        focus: partAppearanceSchema.optional(),
        disabled: partAppearanceSchema.optional()
      })
      .strict()
      .optional()
  })
  .strict()

export type PartStyle = z.infer<typeof partStyleSchema>

/**
 * Nomi vicini a quello scritto, per il messaggio d'errore.
 *
 * Chi crea una skin sbaglia un nome di parte per assonanza — `section-cards`,
 * `np-titles`, `player-bar` — e ricevere «parte inesistente» senza un
 * suggerimento significa aprire il registro e leggerlo tutto. Con due o tre
 * candidati, la correzione è immediata.
 */
function nearestParts(unknownName: string): PartName[] {
  const needle = unknownName.toLowerCase()
  const scored = PART_NAMES.map((candidate) => {
    if (candidate.startsWith(needle) || needle.startsWith(candidate)) return { candidate, score: 3 }
    if (candidate.includes(needle) || needle.includes(candidate)) return { candidate, score: 2 }
    const head = needle.split('-')[0] ?? ''
    return { candidate, score: head.length > 2 && candidate.includes(head) ? 1 : 0 }
  })
  return scored
    .filter((entry) => entry.score > 0)
    .sort((a, b) => b.score - a.score)
    .slice(0, 3)
    .map((entry) => entry.candidate)
}

/**
 * Le parti di una skin.
 *
 * `superRefine` sulle chiavi e non `z.record(z.string().refine(...))`: zod
 * scarta il messaggio del refine sulle chiavi di un record e riporta «Invalid key
 * in record», che non dice quale parte né cosa scrivere al suo posto. È lo stesso
 * inghiottimento del messaggio che ha già portato a scrivere a mano il dispatch
 * dei colori.
 */
export const skinPartsSchema = z
  .record(z.string(), partStyleSchema)
  .superRefine((value, ctx) => {
    for (const name of Object.keys(value)) {
      if (isPartName(name)) continue
      const suggestions = nearestParts(name)
      ctx.addIssue({
        code: 'custom',
        path: [name],
        message:
          suggestions.length > 0
            ? `parte inesistente: '${name}'. Forse intendevi ${suggestions.map((s) => `'${s}'`).join(', ')}?`
            : `parte inesistente: '${name}'. I nomi ammessi sono quelli del parts registry.`
      })
    }
  })

// ── Compilazione ────────────────────────────────────────────────────────────

export interface PartDeclaration {
  readonly property: string
  readonly value: string
}

export interface CompiledPart {
  readonly selector: string
  readonly declarations: readonly PartDeclaration[]
}

/**
 * Le regole CSS di una parte.
 *
 * Il selettore include sempre `[data-skin='<id>']`, quindi una parte ridisegnata
 * da una skin non può influenzare un'altra skin: è la stessa proprietà che il
 * compilatore dei token garantisce, estesa alle parti.
 */
export function compilePart(
  skinId: string,
  name: PartName,
  style: PartStyle,
  renderEffect: (effect: Effect) => string,
  renderColor: (color: unknown) => string,
  renderLength: (length: unknown) => string
): CompiledPart[] {
  const base = `:root[data-skin='${skinId}'] .${name}`
  const output: CompiledPart[] = []

  const appearance = (source: Omit<PartStyle, 'layer' | 'states'>): PartDeclaration[] => {
    const declarations: PartDeclaration[] = []

    if (source.background !== undefined && source.background.length > 0) {
      declarations.push({
        property: 'background',
        value: source.background.map(renderEffect).join(', ')
      })
    }
    if (source.textColor !== undefined) {
      declarations.push({ property: 'color', value: renderColor(source.textColor) })
    }
    if (source.borderColor !== undefined || source.borderWidth !== undefined) {
      const width = source.borderWidth === undefined ? '1px' : renderLength(source.borderWidth)
      const color =
        source.borderColor === undefined ? 'var(--hairline)' : renderColor(source.borderColor)
      declarations.push({ property: 'border', value: `${width} solid ${color}` })
    }
    if (source.radius !== undefined) {
      declarations.push({ property: 'border-radius', value: renderLength(source.radius) })
    }
    if (source.clip !== undefined) {
      declarations.push({ property: 'clip-path', value: renderEffect(source.clip) })
    }
    if (source.opacity !== undefined) {
      declarations.push({ property: 'opacity', value: String(source.opacity) })
    }
    if (source.letterSpacing !== undefined) {
      declarations.push({ property: 'letter-spacing', value: renderLength(source.letterSpacing) })
    }
    if (source.textTransform !== undefined) {
      declarations.push({ property: 'text-transform', value: source.textTransform })
    }
    if (source.fontWeight !== undefined) {
      declarations.push({ property: 'font-weight', value: String(Math.round(source.fontWeight)) })
    }

    return declarations
  }

  const { layer, states, ...rest } = style

  const baseDeclarations = appearance(rest)
  if (baseDeclarations.length > 0) output.push({ selector: base, declarations: baseDeclarations })

  if (layer !== undefined) {
    // Lo pseudo-elemento ha bisogno di `content` e di essere posizionato, ma NON
    // di poter essere spostato dalla skin: le due proprietà le mette il
    // compilatore, sempre uguali.
    output.push({
      selector: `${base}::after`,
      declarations: [
        { property: 'content', value: '""' },
        { property: 'position', value: 'absolute' },
        { property: 'inset', value: '0' },
        { property: 'pointer-events', value: 'none' },
        { property: 'background', value: layer.background.map(renderEffect).join(', ') },
        ...(layer.opacity !== undefined
          ? [{ property: 'opacity', value: String(layer.opacity) }]
          : [])
      ]
    })
  }

  for (const state of PART_STATES) {
    const styleForState = states?.[state]
    if (styleForState === undefined) continue
    const declarations = appearance(styleForState)
    if (declarations.length === 0) continue
    // `:where()` mantiene la specificità del selettore di stato uguale a quella
    // della parte: senza, uno stato dichiarato da una skin vincerebbe su una
    // regola che il componente considera più importante.
    output.push({
      selector: `${base}:where(${STATE_SELECTOR[state]})`,
      declarations
    })
  }

  return output
}
