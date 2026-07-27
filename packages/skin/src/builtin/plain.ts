/**
 * `plain`, la skin di base, convertita nel formato dichiarativo.
 *
 * Non è una skin nuova: è il blocco `:root` di `global.css` riga per riga. Ogni
 * valore qui sotto è quello che l'app usa oggi, e il test di fedeltà confronta il
 * CSS compilato con quelle dichiarazioni.
 *
 * Convertirla ha fatto emergere tre cose che il formato non esprimeva, ed è
 * esattamente il motivo per cui la conversione viene prima dello Studio:
 *
 *   1. `--content-x: clamp(16px, 3cqw, 48px)` — una lunghezza adattiva. Aggiunta
 *      come struttura, non come stringa.
 *   2. `color-scheme: dark` — non un token, ma indispensabile: senza, le barre di
 *      scorrimento native restano chiare su fondo nero. Ora la emette il
 *      compilatore.
 *   3. `--accent-like: var(--accent)` e `--viz-primary: var(--accent)` —
 *      riferimenti fra token, non colori duplicati.
 */

import type { SkinDocument } from '../schema'

export const PLAIN_SKIN_SOURCE = {
  format: 1,
  id: 'plain',
  meta: {
    name: 'Plain',
    author: 'Aether',
    version: '1.0.0',
    description: 'Scuro, sobrio, senza effetti. Il riferimento per tutte le altre.',
    license: 'MIT'
  },
  capabilities: {
    light: true,
    mobile: false,
    // L'accento segue la copertina: è il comportamento storico di questa skin.
    dynamicAccent: true
  },
  tokens: {
    'font.sans': ['Inter Variable'],

    'color.surface.0': '#09090d',
    'color.surface.1': '#0e0e14',
    'color.surface.2': '#16161f',
    'color.surface.3': '#1e1e2a',

    'color.text.1': 'rgba(255, 255, 255, 0.92)',
    'color.text.2': 'rgba(255, 255, 255, 0.6)',
    'color.text.3': 'rgba(255, 255, 255, 0.38)',

    'color.accent': '#8b7cf6',
    'color.accent.soft': 'rgba(139, 124, 246, 0.16)',
    'color.accent.glow': 'rgba(139, 124, 246, 0.35)',
    // Nel legacy: `--accent-like: var(--accent)`. Nothing lo porta al rosso.
    'color.accent.like': { $token: 'color.accent' },
    'color.hero': { $token: 'color.accent' },

    'color.danger': '#e5484d',
    'color.danger.soft': 'rgba(229, 72, 77, 0.14)',
    'color.success': '#34d399',
    'color.success.soft': 'rgba(52, 211, 153, 0.14)',
    'color.warning': '#facc15',
    'color.warning.soft': 'rgba(250, 204, 21, 0.14)',

    'color.ambient.1': 'rgba(139, 124, 246, 0.1)',
    'color.ambient.2': 'rgba(76, 60, 180, 0.06)',
    'color.sidebar': 'rgba(255, 255, 255, 0.04)',
    'color.hairline': 'rgba(255, 255, 255, 0.07)',

    'layout.rail': '68px',
    'layout.railExpanded': '240px',
    'layout.playerHeight': '92px',
    'layout.playerGap': '14px',
    // La lunghezza adattiva che ha richiesto l'aggiunta al formato.
    'layout.contentX': { min: '16px', preferred: '3cqw', max: '48px' },

    'radius.panel': '20px',
    'radius.card': '14px',

    'shadow.1': {
      layers: [{ x: '0px', y: '2px', blur: '12px', color: 'rgba(0, 0, 0, 0.3)' }]
    },
    'shadow.2': {
      layers: [{ x: '0px', y: '8px', blur: '28px', color: 'rgba(0, 0, 0, 0.45)' }]
    },
    'shadow.3': {
      layers: [{ x: '0px', y: '16px', blur: '56px', color: 'rgba(0, 0, 0, 0.55)' }]
    },
    // Hairline interna in alto più caduta profonda: due livelli, uno inset.
    'shadow.player': {
      layers: [
        { inset: true, x: '0px', y: '1px', blur: '0px', color: 'rgba(255, 255, 255, 0.06)' },
        { x: '0px', y: '8px', blur: '40px', color: 'rgba(0, 0, 0, 0.5)' }
      ]
    },
    'glow.accent': {
      layers: [{ x: '0px', y: '0px', blur: '24px', color: { $token: 'color.accent.glow' } }]
    },

    'motion.ease.outExpo': { kind: 'cubicBezier', points: [0.16, 1, 0.3, 1] },
    // y oltre 1 è il rimbalzo: è il motivo per cui lo schema lo ammette.
    'motion.ease.spring': { kind: 'cubicBezier', points: [0.34, 1.56, 0.64, 1] },
    'motion.dur.1': '150ms',
    'motion.dur.2': '280ms',
    'motion.dur.3': '450ms',

    'canvas.viz.primary': { $token: 'color.accent' },
    'canvas.viz.secondary': { $token: 'color.accent' },
    'canvas.viz.glow': 20,
    'canvas.scrubber.glow': 6,
    'canvas.scrubber.rest': 'rgba(255, 255, 255, 0.18)'
  },
  themes: {
    // La variante chiara di `global.css`: cambia poco, e proprio per questo è un
    // buon collaudo del blocco `themes` — sovrascrive solo ciò che serve.
    light: {
      'color.sidebar': 'rgba(0, 0, 0, 0.04)',
      'color.hairline': 'rgba(0, 0, 0, 0.08)',
      'color.danger': '#d1242b',
      'color.danger.soft': 'rgba(209, 36, 43, 0.12)',
      'color.success': '#0f8a4d',
      'color.success.soft': 'rgba(15, 138, 77, 0.12)',
      'color.warning': '#9a7b00',
      'color.warning.soft': 'rgba(154, 123, 0, 0.14)'
    }
  },
  motion: {
    intensity: 'full',
    // Le transizioni di rotta esistenti: vt-out scala a 0.992 e sfuma, vt-in
    // entra da 8px sotto. Erano due @keyframes scritti a mano in global.css.
    routeTransition: {
      out: { opacity: 0, scale: 0.992 },
      in: { opacity: 0, translateY: 8 }
    }
  },
  layout: {
    player: 'floating',
    sidebar: 'rail',
    density: 'comfortable'
  }
} as const satisfies Record<string, unknown>

/** Il tipo lo dà la validazione, non questa costante: `parseSkin` è la fonte. */
export type PlainSkinSource = typeof PLAIN_SKIN_SOURCE
export type { SkinDocument }
