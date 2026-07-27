/**
 * L'adapter desktop.
 *
 * Dichiara cosa il desktop sa fare e fornisce le implementazioni concrete. È
 * l'unico posto del core dove compaiono `node:fs`, i percorsi di Electron e
 * better-sqlite3 — e la regola che lo tiene tale è quella delle capacità: nessun
 * modulo di dominio chiede «sono sul desktop?», chiede «posso lanciare un
 * processo?».
 */

import { defineCapabilities, type Capabilities } from '../../capabilities'

export {
  adaptBetterSqlite,
  detectFts5,
  isHealthy,
  openBetterSqlite,
  type BetterSqliteLike,
  type BetterSqliteStatement,
  type DesktopSqliteOptions
} from './sqlite'

export { createFileSink, type FileSinkOptions } from './logSink'

export {
  backupBeforeMigration,
  coverPath,
  desktopPaths,
  ensureDirectories,
  migrationFiles,
  quarantineDatabase,
  type DesktopPaths
} from './files'

export interface DesktopCapabilityInput {
  /** Etichetta per i log: `desktop-win32-33.2.1`. */
  readonly label: string
  /**
   * yt-dlp è presente e lanciabile.
   *
   * Distinta da `spawn`: un desktop sa lanciare processi anche quando yt-dlp non
   * è installato. Nel legacy le due cose erano confuse, e il risultato era che il
   * pulsante di download appariva e poi falliva con `BINARY_MISSING`.
   */
  readonly ytdlp?: boolean
  /** Il modulo `sharp` si è caricato: su alcune piattaforme il binario manca. */
  readonly sharp?: boolean
  /** mDNS disponibile: `bonjour-service` si è caricato. */
  readonly mdns?: boolean
}

export function desktopCapabilities(input: DesktopCapabilityInput): Capabilities {
  return defineCapabilities({
    label: input.label,
    search: { fts5: true, foldFunction: true },
    system: {
      spawn: true,
      arbitraryPaths: true,
      documentPicker: true,
      watchFilesystem: true,
      imageResize: input.sharp ?? true,
      // Il desktop può rilanciarsi dopo un guasto fatale: `app.relaunch()`
      // esiste. È la differenza che permette al supervisor di uscire dal
      // processo qui e di non uscirne sul mobile.
      restartAfterFatal: true
    },
    network: {
      mdns: input.mdns ?? true,
      // Il desktop serve l'API LAN ai telefoni accoppiati; il protocollo di
      // trasferimento lo serve il telefono, quindi qui è falso.
      lanServer: true,
      transferServer: false,
      qrGenerate: true,
      // Nessuna fotocamera: il QR lo mostra il desktop e lo scansiona il telefono.
      qrScan: false
    },
    playback: {
      // Web Audio nel renderer, con la sua catena EQ/ReplayGain/visualizer.
      nativeAudio: false,
      nativeEqualizer: false,
      nativeCrossfade: false,
      mediaSession: false,
      backgroundService: false,
      thermalStatus: false
    },
    appearance: {
      // Lo Studio esiste solo qui, per decisione di progetto.
      skinStudio: true,
      skinImport: true,
      viewTransitions: true,
      backdropFilter: true
    }
  })
}
