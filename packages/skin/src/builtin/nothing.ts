/**
 * `nothing`, convertita nel formato dichiarativo.
 *
 * La skin è monocroma per scelta, non per povertà: nero OLED piatto, accento
 * bianco, **glow azzerato**, e un solo rosso — `#d71921` — riservato a like ed
 * errori. Il legacy lo dice in un commento: «Rosso "interrupt": riservato a like /
 * errori / stato attivo puntuale». Convertirla mette alla prova il formato in una
 * direzione opposta a `plain`: qui la maggior parte delle scelte è una NEGAZIONE.
 *
 * Le tre negazioni, e come si esprimono:
 *
 *   - `--accent-glow: transparent` → un colore con alpha zero;
 *   - `--glow-accent: none` → un'ombra a ZERO livelli. Il compilatore la emette
 *     come `none`, ed è il motivo per cui `layers: []` è ammesso invece di essere
 *     un errore;
 *   - `--shadow-1/2/3: 0 0 0 1px var(--hairline)` → l'elevazione non è un'ombra ma
 *     un bordo. Tutte e tre uguali, e uguali anche a `--shadow-player`: nella
 *     skin non esiste il concetto di "più in alto".
 *
 * `capabilities.dynamicAccent` è **falso**, e non è un dettaglio: se l'accento
 * seguisse la copertina, la skin diventerebbe colorata e smetterebbe di essere
 * questa skin. Nel legacy la stessa cosa era un booleano nell'oggetto `SKINS`.
 */

const HAIRLINE_BORDER = {
  layers: [{ x: '0px', y: '0px', blur: '0px', spread: '1px', color: { $token: 'color.hairline' } }]
} as const

export const NOTHING_SKIN_SOURCE = {
  format: 1,
  id: 'nothing',
  meta: {
    name: 'Nothing',
    author: 'Aether',
    version: '1.0.0',
    description: 'Monocroma, piatta, tecnica. Nero OLED, un solo rosso, niente aloni.',
    license: 'MIT'
  },
  capabilities: {
    light: true,
    mobile: false,
    // La palette è voluta: seguire la copertina la annullerebbe.
    dynamicAccent: false
  },
  tokens: {
    'font.sans': ['Space Grotesk Variable', 'Inter Variable'],
    'font.mono': ['Space Mono'],
    // Il carattere a matrice di punti da cui viene il nome `--font-dot`.
    'font.display': ['Doto Variable', 'Space Mono'],

    'color.surface.0': '#000000',
    'color.surface.1': '#0a0a0a',
    'color.surface.2': '#141414',
    'color.surface.3': '#1f1f1f',

    'color.text.1': 'rgba(255, 255, 255, 0.92)',
    'color.text.2': 'rgba(255, 255, 255, 0.62)',
    'color.text.3': 'rgba(255, 255, 255, 0.42)',

    // Accento bianco, alone inesistente.
    'color.accent': '#ffffff',
    'color.accent.soft': 'rgba(255, 255, 255, 0.1)',
    'color.accent.glow': 'rgba(255, 255, 255, 0)',
    // Il rosso interrupt. Nel legacy era il token skin-locale `--nothing-red`,
    // riferito da `--accent-like` e da `--danger`.
    'color.accent.like': '#d71921',
    'color.danger': '#d71921',
    'color.danger.soft': 'rgba(215, 25, 33, 0.14)',
    'color.success': '#34d399',
    'color.success.soft': 'rgba(52, 211, 153, 0.14)',
    'color.warning': '#facc15',
    'color.warning.soft': 'rgba(250, 204, 21, 0.14)',

    'color.sidebar': '#000000',
    'color.hairline': 'rgba(255, 255, 255, 0.16)',
    // L'ambient backdrop non è un alone: è il campo di punti, disegnato dalla
    // parte `ambient-backdrop`. I due token si spengono.
    'color.ambient.1': 'rgba(0, 0, 0, 0)',
    'color.ambient.2': 'rgba(0, 0, 0, 0)',
    'color.hero': '#ffffff',

    'radius.panel': '12px',
    'radius.card': '4px',

    // L'elevazione è un bordo, non un'ombra. Tutte e tre identiche.
    'shadow.1': HAIRLINE_BORDER,
    'shadow.2': HAIRLINE_BORDER,
    'shadow.3': HAIRLINE_BORDER,
    'shadow.player': HAIRLINE_BORDER,
    // Zero livelli: il compilatore emette `none`.
    'glow.accent': { layers: [] },

    // Motion meccanica: la curva neutralizza ogni sovra-oscillazione, quindi
    // `ease.spring` e `ease.outExpo` sono LA STESSA curva. Nel legacy erano due
    // dichiarazioni identiche, e qui restano due token perché i componenti li
    // leggono separatamente.
    'motion.ease.outExpo': { kind: 'cubicBezier', points: [0.2, 0, 0, 1] },
    'motion.ease.spring': { kind: 'cubicBezier', points: [0.2, 0, 0, 1] },
    'motion.dur.1': '90ms',
    'motion.dur.2': '160ms',
    'motion.dur.3': '260ms',

    'canvas.viz.primary': { $token: 'color.accent' },
    'canvas.viz.secondary': { $token: 'color.accent' },
    'canvas.viz.glow': 0,
    // Il playhead non brilla: qui il glow è vietato.
    'canvas.scrubber.glow': 0,
    'canvas.scrubber.rest': 'rgba(255, 255, 255, 0.18)'
  },
  themes: {
    // Chiaro "warm off-white", accento nero: l'inversione completa, non un
    // semplice schiarimento.
    light: {
      'color.surface.0': '#f3f1ec',
      'color.surface.1': '#ebe9e3',
      'color.surface.2': '#ffffff',
      'color.surface.3': '#e4e1da',
      'color.text.1': 'rgba(0, 0, 0, 0.9)',
      'color.text.2': 'rgba(0, 0, 0, 0.6)',
      'color.text.3': 'rgba(0, 0, 0, 0.42)',
      'color.sidebar': '#f3f1ec',
      'color.hairline': 'rgba(0, 0, 0, 0.18)',
      'color.accent': '#000000',
      'color.accent.soft': 'rgba(0, 0, 0, 0.08)',
      'color.danger.soft': 'rgba(215, 25, 33, 0.1)',
      'color.success': '#12855a',
      'color.success.soft': 'rgba(18, 133, 90, 0.12)',
      'color.warning': '#9a7b00',
      'color.warning.soft': 'rgba(154, 123, 0, 0.14)'
    }
  },
  motion: {
    intensity: 'essential',
    // Le transizioni non scalano e non rimbalzano: entrano e escono, secche.
    routeTransition: {
      out: { opacity: 0 },
      in: { opacity: 0, translateY: 4 }
    }
  },
  layout: {
    player: 'bottom-bar',
    sidebar: 'rail',
    density: 'compact'
  },
  patterns: {
    // La texture dot-matrix. Nel legacy: `radial-gradient(currentColor 1px,
    // transparent 1.5px) 0 0 / 12px 12px` — con `currentColor`, che il formato non
    // ammette perché dipende da dove viene usato. Qui il colore è esplicito.
    'dot-grid': {
      effect: 'dotGrid',
      color: 'rgba(255, 255, 255, 0.16)',
      spacing: '12px',
      dot: '1px'
    }
  },
  parts: {
    // Il campo di punti dietro al contenuto: è la firma visiva della skin.
    'ambient-backdrop': {
      background: [
        { effect: 'solid', color: { $token: 'color.surface.0' } },
        {
          effect: 'dotGrid',
          color: 'rgba(255, 255, 255, 0.16)',
          spacing: '12px',
          dot: '1px'
        }
      ]
    },
    // L'elevazione come bordo, non come ombra: vale anche per le parti.
    'section-card': {
      borderColor: { $token: 'color.hairline' },
      borderWidth: '1px',
      radius: '4px',
      background: [{ effect: 'solid', color: { $token: 'color.surface.1' } }]
    },
    'player-shell': {
      borderColor: { $token: 'color.hairline' },
      borderWidth: '1px',
      radius: '12px',
      background: [{ effect: 'solid', color: { $token: 'color.surface.0' } }]
    },
    // Le maiuscole spaziate sono l'altra metà dell'identità tipografica.
    'hero-eyebrow': {
      textTransform: 'uppercase',
      letterSpacing: '0.14em',
      textColor: { $token: 'color.text.2' }
    },
    'section-heading': {
      textTransform: 'uppercase',
      letterSpacing: '0.1em',
      fontWeight: 500
    },
    'nav-pill': {
      radius: '4px',
      states: {
        hover: { background: [{ effect: 'solid', color: { $token: 'color.accent.soft' } }] },
        active: {
          background: [{ effect: 'solid', color: { $token: 'color.accent' } }],
          textColor: { $token: 'color.surface.0' }
        }
      }
    },
    'icon-btn': {
      radius: '4px',
      states: {
        hover: { background: [{ effect: 'solid', color: { $token: 'color.accent.soft' } }] },
        focus: { borderColor: { $token: 'color.accent' }, borderWidth: '1px' }
      }
    }
  }
} as const satisfies Record<string, unknown>
