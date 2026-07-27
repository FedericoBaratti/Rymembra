/**
 * Capacità della piattaforma.
 *
 * È la regola strutturale del core riscritto: **si dichiarano capacità, non
 * piattaforme.** Dentro `packages/core` non esiste nessun `if (isMobile)`. Chi ha
 * bisogno di sapere se può cercare con FTS5 chiede `caps.fts5`; chi deve lanciare
 * yt-dlp chiede `caps.spawn`.
 *
 * Perché non basta un flag `platform`. Nel legacy la stessa domanda veniva posta
 * in modi diversi in punti diversi — `isMobile`, `process.platform`, la presenza
 * di un modulo, un try/catch attorno a un require — e i due alberi hanno finito
 * per divergere su 73 file. Peggio: alcune differenze non sono la piattaforma ma
 * il DEPLOY. Un desktop senza yt-dlp installato ha `spawn: true` e
 * `youtubeDownload: false`; una vecchia build Android senza il plugin
 * fotocamera ha `qrScan: false` pur essendo lo stesso Android. Con una capacità
 * per fatto, il codice risponde alla domanda giusta.
 *
 * Ogni voce qui sotto corrisponde a una divergenza REALE trovata fra i due alberi
 * legacy, non a una previsione.
 */

/** Come si cerca nel testo. */
export interface SearchCapabilities {
  /**
   * FTS5 disponibile. Falso sul backend mobile: sql.js è compilato senza FTS5, e
   * infatti il legacy salta la creazione di `tracks_fts` sul dispositivo e cade
   * su un LIKE con la funzione SQL `afold()`. Chi cerca deve saperlo, perché le
   * due strade hanno sintassi di query diverse (`MATCH` contro `LIKE`).
   */
  readonly fts5: boolean
  /** La funzione scalare `afold()` è registrata: ripiego senza diacritici. */
  readonly foldFunction: boolean
}

/** Cosa si può fare col disco e coi processi. */
export interface SystemCapabilities {
  /**
   * `child_process.spawn`. Falso su nodejs-mobile: il legacy risolve con un
   * reverse-RPC verso il lato Java (`ytdlp-shim.ts`), non con un fallback.
   */
  readonly spawn: boolean
  /** Percorsi arbitrari, contro accesso mediato (SAF su Android). */
  readonly arbitraryPaths: boolean
  /** Storage Access Framework: import/export di file scelti dall'utente. */
  readonly documentPicker: boolean
  /** Sorveglianza della cartella libreria (chokidar). Assente su Android. */
  readonly watchFilesystem: boolean
  /** Ridimensionamento immagini nativo (sharp). Sul mobile è uno shim. */
  readonly imageResize: boolean
  /**
   * Il processo può essere riavviato dopo un guasto fatale. Falso su
   * nodejs-mobile, che non si riavvia in-process: lì uscire significa app morta,
   * ed è il motivo per cui il supervisor non esce dal processo per default.
   */
  readonly restartAfterFatal: boolean
}

/** Rete locale e dispositivi accoppiati. */
export interface NetworkCapabilities {
  /** Annuncio e scoperta mDNS (bonjour-service sul desktop, NsdManager su Android). */
  readonly mdns: boolean
  /** Serve l'API LAN ai client thin (oggi: solo il desktop). */
  readonly lanServer: boolean
  /** Serve il protocollo di trasferimento su porta effimera (oggi: solo il telefono). */
  readonly transferServer: boolean
  /** Genera codici QR di accoppiamento (la libreria `qrcode` sta sul desktop). */
  readonly qrGenerate: boolean
  /** Scansiona codici QR con la fotocamera (mlkit, solo mobile). */
  readonly qrScan: boolean
}

/** Riproduzione. */
export interface PlaybackCapabilities {
  /** Motore audio nativo (ExoPlayer) invece di Web Audio nel renderer. */
  readonly nativeAudio: boolean
  /** Equalizzatore nel motore nativo. */
  readonly nativeEqualizer: boolean
  /** Crossfade gestito dal motore nativo. */
  readonly nativeCrossfade: boolean
  /** Sessione media di sistema (notifica, lockscreen, Android Auto). */
  readonly mediaSession: boolean
  /** Lavoro in background con servizio in foreground e wakelock. */
  readonly backgroundService: boolean
  /** Stato termico del dispositivo, per ridurre il carico. */
  readonly thermalStatus: boolean
}

/** Aspetto e skin. */
export interface AppearanceCapabilities {
  /**
   * L'editor di skin è disponibile. Falso sul telefono per decisione di
   * progetto: sul mobile si importa, si applica e si elimina, non si crea.
   */
  readonly skinStudio: boolean
  /** Le skin si possono importare da un pacchetto. Vero su entrambi. */
  readonly skinImport: boolean
  /** View Transitions API disponibile nel motore di rendering. */
  readonly viewTransitions: boolean
  /** `backdrop-filter` composita senza costi proibitivi. */
  readonly backdropFilter: boolean
}

export interface Capabilities {
  /**
   * Etichetta per log e diagnostica: 'desktop-win32', 'android-14', …
   *
   * NON va usata per decidere: se serve una decisione, serve una capacità. È qui
   * perché un log che non dice su cosa girava è inutile.
   */
  readonly label: string
  readonly search: SearchCapabilities
  readonly system: SystemCapabilities
  readonly network: NetworkCapabilities
  readonly playback: PlaybackCapabilities
  readonly appearance: AppearanceCapabilities
}

/**
 * Tutto spento. È la base da cui gli adapter partono, e il default sicuro: una
 * capacità che nessuno dichiara è assente, non presente. Il verso opposto
 * farebbe fallire il codice sul dispositivo, che è esattamente il modo in cui i
 * bug del porting legacy arrivavano in produzione.
 */
export const NO_CAPABILITIES: Capabilities = {
  label: 'unknown',
  search: { fts5: false, foldFunction: false },
  system: {
    spawn: false,
    arbitraryPaths: false,
    documentPicker: false,
    watchFilesystem: false,
    imageResize: false,
    restartAfterFatal: false
  },
  network: {
    mdns: false,
    lanServer: false,
    transferServer: false,
    qrGenerate: false,
    qrScan: false
  },
  playback: {
    nativeAudio: false,
    nativeEqualizer: false,
    nativeCrossfade: false,
    mediaSession: false,
    backgroundService: false,
    thermalStatus: false
  },
  appearance: {
    skinStudio: false,
    skinImport: false,
    viewTransitions: false,
    backdropFilter: false
  }
}

/** Sovrascrittura per gruppi, così un adapter dichiara solo ciò che sa. */
export type CapabilityOverrides = {
  readonly label?: string
} & {
  readonly [K in Exclude<keyof Capabilities, 'label'>]?: Partial<Capabilities[K]>
}

export function defineCapabilities(overrides: CapabilityOverrides): Capabilities {
  return {
    label: overrides.label ?? NO_CAPABILITIES.label,
    search: { ...NO_CAPABILITIES.search, ...overrides.search },
    system: { ...NO_CAPABILITIES.system, ...overrides.system },
    network: { ...NO_CAPABILITIES.network, ...overrides.network },
    playback: { ...NO_CAPABILITIES.playback, ...overrides.playback },
    appearance: { ...NO_CAPABILITIES.appearance, ...overrides.appearance }
  }
}
