/**
 * `cyberpunk`, convertita nel formato dichiarativo.
 *
 * È il caso peggiore, e per questo la prova decisiva: 1.879 righe di CSS in nove
 * file, 90 agganci di parte, 17 keyframes propri, e un vocabolario di effetti che
 * nessun'altra skin usa — griglia al neon, scanline CRT, strisce hazard, angoli
 * tagliati, microtesto, e un pavimento prospettico con nebbia.
 *
 * Convertirla ha fatto emergere le due cose che mancavano al formato, entrambe
 * aggiunte in questo passaggio:
 *
 * **La tavolozza locale.** `--cyber-teal` compare in decine di dichiarazioni:
 * hairline, griglia, scanline, microtesto, tutte e quattro le ombre, i due token
 * del visualizer, lo scrubber. Senza un nome andrebbe ripetuto letteralmente in
 * ognuna — e cambiare il teal della skin diventerebbe trovare e correggere venti
 * valori identici, con la certezza di sbagliarne uno.
 *
 * **La nebbia derivata.** Il legacy dichiara `--cyber-fog-rgb: 6 6 8` con accanto
 * un commento in maiuscolo: «Nebbia del pavimento prospettico: DEVE combaciare
 * con surface-0». Un'invariante affidata a un commento si rompe: basta ritoccare
 * la superficie e dimenticare la nebbia, e il pavimento sfuma verso un colore che
 * non è il fondo. Ora il compilatore emette `--surface-0-rgb` da `surface.0`, e il
 * commento non serve più.
 *
 * Nota sul chamfer: nel legacy erano DUE token da tenere coerenti a mano,
 * `--cyber-cut` per la misura e `--cyber-chamfer` per il poligono che la usa. Qui
 * è un effetto con un parametro.
 */

export const CYBERPUNK_SKIN_SOURCE = {
  format: 1,
  id: 'cyberpunk',
  meta: {
    name: 'Cyberpunk',
    author: 'Aether',
    version: '1.0.0',
    description: 'HUD al neon: giallo elettrico e teal, angoli tagliati, CRT.',
    license: 'MIT'
  },
  capabilities: {
    light: true,
    mobile: false,
    dynamicAccent: false
  },
  /** I colori secondari, nominati una volta. */
  palette: {
    teal: '#00f0ff',
    red: '#ff003c',
    'neon-green': '#0aff9d'
  },
  tokens: {
    // Orbitron da display, Rajdhani per il corpo tecnico, Space Mono per le
    // etichette in maiuscole.
    'font.sans': ['Rajdhani', 'Space Grotesk Variable', 'Inter Variable'],
    'font.mono': ['Space Mono'],
    'font.display': ['Orbitron Variable', 'Doto Variable', 'Space Mono'],

    // Nero HUD, freddo.
    'color.surface.0': '#060608',
    'color.surface.1': '#0b0b12',
    'color.surface.2': '#12121c',
    'color.surface.3': '#1b1b28',

    // Bianco caldo, poi teal-white: la gerarchia del gioco.
    'color.text.1': 'rgba(240, 240, 224, 0.95)',
    'color.text.2': 'rgba(190, 235, 245, 0.64)',
    'color.text.3': 'rgba(190, 235, 245, 0.42)',

    'color.sidebar': '#060608',
    // La hairline è teal al neon, non un grigio.
    'color.hairline': { $palette: 'teal', alpha: 0.18 },

    // Accento = giallo elettrico.
    'color.accent': '#fcee0a',
    'color.accent.soft': 'rgba(252, 238, 10, 0.14)',
    'color.accent.glow': 'rgba(252, 238, 10, 0.55)',
    // Cuore ed errori usano il rosso glitch.
    'color.accent.like': { $palette: 'red' },

    'color.danger': { $palette: 'red' },
    'color.danger.soft': { $palette: 'red', alpha: 0.14 },
    'color.success': { $palette: 'neon-green' },
    'color.success.soft': { $palette: 'neon-green', alpha: 0.14 },
    // Il warning È l'accento: il giallo hazard è la firma della skin.
    'color.warning': { $token: 'color.accent' },
    'color.warning.soft': 'rgba(252, 238, 10, 0.14)',

    'color.ambient.1': 'rgba(252, 238, 10, 0.06)',
    'color.ambient.2': { $palette: 'teal', alpha: 0.07 },
    'color.hero': { $token: 'color.accent' },

    // Elevazione = alone al neon più hairline teal, non ombra morbida.
    'shadow.1': {
      layers: [
        { x: '0px', y: '0px', blur: '0px', spread: '1px', color: { $palette: 'teal', alpha: 0.1 } },
        { x: '0px', y: '2px', blur: '14px', color: 'rgba(0, 0, 0, 0.6)' }
      ]
    },
    'shadow.2': {
      layers: [
        { x: '0px', y: '0px', blur: '0px', spread: '1px', color: { $palette: 'teal', alpha: 0.14 } },
        { x: '0px', y: '10px', blur: '30px', color: 'rgba(0, 0, 0, 0.7)' },
        { x: '0px', y: '0px', blur: '22px', color: { $palette: 'teal', alpha: 0.08 } }
      ]
    },
    'shadow.3': {
      layers: [
        { x: '0px', y: '0px', blur: '0px', spread: '1px', color: { $palette: 'teal', alpha: 0.16 } },
        { x: '0px', y: '18px', blur: '50px', color: 'rgba(0, 0, 0, 0.75)' },
        { x: '0px', y: '0px', blur: '30px', color: { $palette: 'teal', alpha: 0.1 } }
      ]
    },
    'shadow.player': {
      layers: [
        { x: '0px', y: '0px', blur: '0px', spread: '1px', color: { $palette: 'teal', alpha: 0.28 } },
        { x: '0px', y: '0px', blur: '34px', color: { $palette: 'teal', alpha: 0.18 } },
        { x: '0px', y: '14px', blur: '44px', color: 'rgba(0, 0, 0, 0.75)' }
      ]
    },
    'glow.accent': {
      layers: [{ x: '0px', y: '0px', blur: '22px', color: { $token: 'color.accent.glow' } }]
    },

    // Geometria angolare: è metà dell'identità della skin.
    'radius.panel': '4px',
    'radius.card': '3px',

    // Motion secca e punchy.
    'motion.ease.outExpo': { kind: 'cubicBezier', points: [0.16, 1, 0.3, 1] },
    'motion.ease.spring': { kind: 'cubicBezier', points: [0.2, 0.9, 0.2, 1] },
    'motion.dur.1': '120ms',
    'motion.dur.2': '220ms',
    'motion.dur.3': '380ms',

    // Il contratto visualizer: barre teal, anello dei bassi giallo, alone ampio.
    // Arriva dentro i canvas senza una riga di JS che conosca la skin.
    'canvas.viz.primary': { $palette: 'teal' },
    'canvas.viz.secondary': { $token: 'color.accent' },
    'canvas.viz.glow': 26,
    'canvas.scrubber.glow': 12,
    'canvas.scrubber.rest': { $palette: 'teal', alpha: 0.16 }
  },
  themes: {
    /*
     * «Daytime HUD»: fondo chiaro, neon attenuati. Il canone la legittima — il
     * menu degli upgrade nel gioco è beige.
     *
     * Il tema chiaro ridichiara anche la tavolozza attraverso i token che la
     * usano: i colori locali restano quelli del tema scuro, ma i token che ne
     * derivano vengono sovrascritti con i valori leggibili su chiaro.
     */
    light: {
      'color.surface.0': '#eef1f4',
      'color.surface.1': '#e6eaef',
      'color.surface.2': '#f7f9fb',
      'color.surface.3': '#dce2e9',
      'color.text.1': 'rgba(10, 12, 16, 0.92)',
      'color.text.2': 'rgba(8, 44, 54, 0.64)',
      'color.text.3': 'rgba(8, 44, 54, 0.44)',
      'color.sidebar': '#eef1f4',
      'color.hairline': 'rgba(0, 150, 170, 0.3)',
      // Giallo leggibile su chiaro: il #fcee0a non lo è.
      'color.accent': '#b39700',
      'color.accent.soft': 'rgba(179, 151, 0, 0.14)',
      'color.accent.glow': 'rgba(179, 151, 0, 0.3)',
      'color.danger.soft': 'rgba(209, 0, 47, 0.12)',
      'color.success': '#00915f',
      'color.success.soft': 'rgba(0, 145, 95, 0.12)',
      'color.warning.soft': 'rgba(179, 151, 0, 0.14)',
      'canvas.viz.glow': 14,
      'canvas.scrubber.rest': 'rgba(0, 139, 163, 0.22)',
      'shadow.player': {
        layers: [
          { x: '0px', y: '0px', blur: '0px', spread: '1px', color: 'rgba(0, 139, 163, 0.3)' },
          { x: '0px', y: '8px', blur: '26px', color: 'rgba(0, 0, 0, 0.14)' }
        ]
      },
      'glow.accent': {
        layers: [{ x: '0px', y: '0px', blur: '16px', color: { $token: 'color.accent.glow' } }]
      }
    }
  },
  motion: {
    intensity: 'full',
    routeTransition: {
      out: { opacity: 0, scale: 0.994 },
      in: { opacity: 0, translateY: 10 }
    }
  },
  layout: {
    player: 'floating',
    sidebar: 'rail',
    density: 'comfortable'
  },
  /*
   * I motivi. Ognuno era un token scritto a mano nel legacy, e i primi due erano
   * ripetuti quasi identici nella variante chiara — con una svista: là il passo
   * della griglia è `36px 36px` invece di `100% 36px`.
   */
  patterns: {
    grid: { effect: 'hairlineGrid', color: { $palette: 'teal', alpha: 0.07 }, cell: '36px' },
    scanline: {
      effect: 'scanlines',
      color: { $palette: 'teal', alpha: 0.05 },
      line: '1px',
      gap: '3px'
    },
    hazard: {
      effect: 'stripes',
      angle: -45,
      color: 'rgba(252, 238, 10, 0.85)',
      background: 'rgba(5, 5, 5, 0.9)',
      width: '6px'
    },
    'hazard-red': {
      effect: 'stripes',
      angle: -45,
      color: { $palette: 'red', alpha: 0.8 },
      background: 'rgba(5, 5, 5, 0.9)',
      width: '6px'
    },
    // Un parametro, non due token da tenere coerenti.
    chamfer: { effect: 'chamfer', size: '14px' },
    'chamfer-sm': { effect: 'chamfer', size: '7px' }
  },
  parts: {
    /*
     * Il fondo: griglia al neon sopra il nero HUD, più le scanline CRT come
     * livello sovrapposto. Nel legacy erano `.ambient-backdrop` più un
     * `body::after` — cioè due elementi diversi, uno dei quali fuori dal
     * controllo della skin.
     */
    'ambient-backdrop': {
      background: [
        { effect: 'solid', color: { $token: 'color.surface.0' } },
        { effect: 'hairlineGrid', color: { $palette: 'teal', alpha: 0.07 }, cell: '36px' }
      ],
      layer: {
        background: [
          { effect: 'scanlines', color: { $palette: 'teal', alpha: 0.05 }, line: '1px', gap: '3px' }
        ],
        opacity: 0.6
      }
    },
    // Gli angoli tagliati sulle superfici: la firma geometrica.
    'section-card': {
      background: [{ effect: 'solid', color: { $token: 'color.surface.1' } }],
      borderColor: { $palette: 'teal', alpha: 0.14 },
      borderWidth: '1px',
      clip: { effect: 'chamfer', size: '14px' }
    },
    'player-shell': {
      background: [{ effect: 'solid', color: { $token: 'color.surface.0' } }],
      borderColor: { $palette: 'teal', alpha: 0.28 },
      borderWidth: '1px',
      clip: { effect: 'chamfer', size: '14px' }
    },
    'glass-modal': {
      background: [{ effect: 'solid', color: { $token: 'color.surface.1' } }],
      borderColor: { $palette: 'teal', alpha: 0.16 },
      borderWidth: '1px',
      clip: { effect: 'chamfer', size: '14px' }
    },
    // Il microtesto diegetico: seriali HUD negli angoli, in maiuscole spaziate.
    'hero-eyebrow': {
      textColor: { $palette: 'teal', alpha: 0.48 },
      textTransform: 'uppercase',
      letterSpacing: '0.22em'
    },
    'page-title': {
      textTransform: 'uppercase',
      letterSpacing: '0.04em',
      fontWeight: 700
    },
    'section-heading': {
      textColor: { $palette: 'teal' },
      textTransform: 'uppercase',
      letterSpacing: '0.12em'
    },
    'nav-pill': {
      clip: { effect: 'chamfer', size: '7px' },
      states: {
        hover: { background: [{ effect: 'solid', color: { $palette: 'teal', alpha: 0.12 } }] },
        active: {
          background: [{ effect: 'solid', color: { $token: 'color.accent' } }],
          textColor: { $token: 'color.surface.0' }
        }
      }
    },
    'icon-btn': {
      clip: { effect: 'chamfer', size: '7px' },
      states: {
        hover: { background: [{ effect: 'solid', color: { $palette: 'teal', alpha: 0.12 } }] },
        focus: { borderColor: { $token: 'color.accent' }, borderWidth: '1px' }
      }
    },
    // Le strisce hazard dove serve avvertire: è il loro uso nel canone.
    'toast-card': {
      background: [{ effect: 'solid', color: { $token: 'color.surface.2' } }],
      borderColor: { $palette: 'teal', alpha: 0.16 },
      borderWidth: '1px',
      clip: { effect: 'chamfer', size: '7px' },
      layer: {
        background: [
          {
            effect: 'stripes',
            angle: -45,
            color: 'rgba(252, 238, 10, 0.85)',
            background: 'rgba(5, 5, 5, 0.9)',
            width: '6px'
          }
        ],
        opacity: 0.12
      }
    },
    'play-btn-primary': {
      background: [{ effect: 'solid', color: { $token: 'color.accent' } }],
      textColor: { $token: 'color.surface.0' },
      clip: { effect: 'chamfer', size: '7px' }
    }
  }
} as const satisfies Record<string, unknown>
