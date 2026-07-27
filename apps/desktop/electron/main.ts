/**
 * L'avvio del desktop.
 *
 * Questo file è deliberatamente sottile, e l'ordine delle operazioni è il
 * contenuto: è la correzione di due bug del legacy che stavano proprio qui.
 *
 * **Primo.** Il legacy non aveva NIENTE come rete di sicurezza di processo: zero
 * `uncaughtException`, zero `unhandledRejection`, nessun crashReporter. Un throw
 * uccideva l'app e perdeva impostazioni, coda, segreti e store di accoppiamento —
 * tutti su debounce. Qui il supervisor si installa PRIMA di qualunque altra cosa
 * possa lanciare.
 *
 * **Secondo.** `library.ipc.ts:65` chiamava `getDb()` come prima istruzione della
 * registrazione degli handler. Un database guasto significava zero handler
 * registrati, e un renderer che chiama e non riceve mai risposta. Qui gli handler
 * si registrano SEMPRE, e l'apertura del database è un valore che ognuno di loro
 * può leggere.
 *
 * E `will-quit`: il legacy concatenava quattro flush non guardati, quindi il primo
 * throw saltava gli altri tre e perdeva dati anche in una chiusura pulita. Ora è
 * il supervisor a eseguirli, uno per uno, con un nome.
 */

import { BrowserWindow, app, ipcMain, shell } from 'electron'
import { join } from 'node:path'
import Database from 'better-sqlite3'
import {
  AETHER_CHAIN,
  addLogSink,
  assertHandlersComplete,
  channelNames,
  configureLogger,
  createConsoleSink,
  createDbHandle,
  createSupervisor,
  defineHandlers,
  errorEnvelope,
  flushLogs,
  logger,
  toEnvelope,
  type BoundHandler
} from '@aether/core'
import {
  backupBeforeMigration,
  createFileSink,
  desktopCapabilities,
  desktopPaths,
  detectFts5,
  ensureDirectories,
  migrationFiles,
  openBetterSqlite,
  quarantineDatabase
} from '@aether/core/adapters/desktop'
import { CONTRACT } from './contract'
import { createHandlers, loadSkinDocument } from './handlers'
import { createSettingsStore } from './settingsStore'
import { BUILTIN_SKIN_SOURCES, type SkinDocument } from '@aether/skin'

const log = logger('boot')

const isDevelopment = process.env['NODE_ENV'] === 'development' || !app.isPackaged

function main(): void {
  // Una sola istanza.
  //
  // Non è una comodità: due processi sullo STESSO file SQLite e sullo stesso
  // settings.json si pestano i piedi in modi che si manifestano come guasti
  // apparentemente casuali — SQLITE_BUSY sotto carico, impostazioni che tornano
  // indietro perché l'ultimo a scrivere vince con dati vecchi. Nel legacy non
  // c'era, e i due file sono esattamente quelli che l'app tiene su debounce.
  if (!app.requestSingleInstanceLock()) {
    app.quit()
    return
  }
  app.on('second-instance', () => {
    // Chi ha provato ad aprire una seconda copia voleva la finestra: gliela si
    // porta davanti invece di non fare niente.
    const [existing] = BrowserWindow.getAllWindows()
    if (existing !== undefined) {
      if (existing.isMinimized()) existing.restore()
      existing.focus()
    }
  })

  const paths = desktopPaths(app.getPath('userData'))

  // 1. I log per primi: senza, tutto ciò che segue è invisibile.
  configureLogger({
    minLevel: isDevelopment ? 'debug' : 'info',
    sinks: [createConsoleSink()]
  })

  const directoryError = ensureDirectories(paths)
  if (directoryError !== null) {
    // Non si può nemmeno creare la cartella dei dati: si continua comunque, ma
    // con la console come unico sink. Meglio un'app degradata che una che non parte.
    log.error('creazione delle cartelle dati non riuscita', directoryError)
  } else {
    addLogSink(createFileSink({ directory: paths.logs }))
  }

  log.info('avvio', { paths: paths.userData, version: app.getVersion() })

  const settings = createSettingsStore(paths.userData)

  // 2. Il database, che può fallire senza fermare niente.
  const db = createDbHandle({
    path: paths.database,
    openDriver: (path) =>
      openBetterSqlite(path, {
        createDatabase: (file) => new Database(file) as never
      }),
    chain: AETHER_CHAIN,
    // Questo file è stato scritto dall'app desktop: la sua storia è quella.
    history: 'desktop',
    files: migrationFiles(paths),
    supportsFts5: true,
    detectFts5,
    backup: (version) => backupBeforeMigration(paths, version),
    quarantine: (path) => quarantineDatabase(paths, path)
  })

  const capabilities = desktopCapabilities({
    label: `desktop-${process.platform}-${process.versions.electron ?? '?'}`
  })

  const skins = loadBuiltinSkins()

  // 3. Il supervisor, prima di aprire finestre.
  const supervisor = createSupervisor({
    flushes: [
      { name: 'logs', run: () => void flushLogs() },
      { name: 'database', run: () => db.close() }
    ],
    notify: (event) => {
      for (const window of BrowserWindow.getAllWindows()) {
        window.webContents.send('aether:fatal', event.error.toPayload())
      }
    },
    // Il desktop PUÒ rilanciarsi: `restartAfterFatal` è vero fra le sue capacità,
    // ed è la differenza con il mobile, dove uscire significa app morta.
    exitOnFatal: false
  })
  const uninstallSupervisor = supervisor.installProcessHandlers()

  // 4. Gli handler, SEMPRE, database o no.
  const bound = defineHandlers(
    CONTRACT,
    createHandlers({ db, capabilities, settings, dbPath: paths.database, skins })
  )
  registerIpc(bound)

  const complete = assertHandlersComplete(CONTRACT, bound)
  if (!complete.ok) {
    // I tipi lo garantiscono a compile time; questo coglie un contratto composto a
    // runtime. Un buco qui è un errore immediato e nominato, non un reject quando
    // l'utente clicca.
    log.fatal('contratto IPC incompleto', complete.error)
  }

  // 5. L'apertura del database si tenta ora, e il suo esito è solo un log: se
  //    fallisce, gli handler ci sono già e la diagnostica funziona.
  const opened = db.open()
  if (!opened.ok) {
    log.error('database non disponibile: l\'app parte in modalità ridotta', opened.error)
  }

  void createWindow()

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) void createWindow()
  })

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit()
  })

  app.on('will-quit', () => {
    // Uno per uno, con un nome: il legacy li concatenava e il primo throw saltava
    // gli altri tre.
    const outcomes = supervisor.runFlushes('quit')
    for (const outcome of outcomes) {
      if (!outcome.ok) log.error(`flush ${outcome.name} non riuscito`, outcome.error)
    }
    uninstallSupervisor()
  })
}

/**
 * Registra i canali sul trasporto.
 *
 * Gli handler NON lanciano: risolvono sempre con una busta, e l'errore viaggia
 * come dato. È ciò che sostituisce `ipc/handle.ts:14`, che rilanciava
 * `new Error(err.message)` e distruggeva dieci classi di errore ricche.
 */
function registerIpc(bound: Record<string, BoundHandler>): void {
  for (const name of channelNames(CONTRACT)) {
    const handler = bound[name]
    if (handler === undefined) continue
    ipcMain.handle(name, async (event, input: unknown) => {
      try {
        const result = await handler(input, {
          // Questa chiamata viene dalla finestra locale. Un client LAN userà
          // origin 'lan', e gli handler potranno distinguerli.
          origin: 'renderer',
          deviceId: String(event.sender.id)
        })
        return toEnvelope(result)
      } catch (cause) {
        // Non dovrebbe accadere — defineHandlers cattura già tutto — ma se accade
        // il renderer riceve una busta e non resta appeso a una promise.
        return errorEnvelope(cause)
      }
    })
  }
}

/**
 * Le skin di serie.
 *
 * Passano dallo STESSO percorso di validazione di un pacchetto importato: se una
 * built-in è malformata lo si scopre in sviluppo, con il messaggio che vedrebbe
 * l'utente. Nel legacy le skin erano CSS caricato dal bundle, quindi un errore non
 * veniva scoperto affatto.
 */
function loadBuiltinSkins(): Map<string, { document: SkinDocument; builtin: boolean }> {
  const skins = new Map<string, { document: SkinDocument; builtin: boolean }>()
  for (const source of BUILTIN_SKIN_SOURCES) {
    const parsed = loadSkinDocument(source)
    if (!parsed.ok) {
      log.error('skin di serie non valida', parsed.error)
      continue
    }
    skins.set(parsed.value.id, { document: parsed.value, builtin: true })
  }
  log.info('skin di serie caricate', { count: skins.size, ids: [...skins.keys()] })
  return skins
}

async function createWindow(): Promise<void> {
  const window = new BrowserWindow({
    width: 1280,
    height: 820,
    minWidth: 940,
    minHeight: 600,
    show: false,
    // Il colore di fondo prima del primo fotogramma. Nel legacy era cablato a
    // #09090d — il surface-0 della skin plain — quindi con un'altra skin l'avvio
    // a freddo lampeggiava del colore sbagliato. Qui verrà dalla skin attiva.
    backgroundColor: '#09090d',
    autoHideMenuBar: true,
    webPreferences: {
      // `.cjs` e non `.js`: il preload è costruito in formato CommonJS, perché
      // Electron lo carica in un contesto che non accetta moduli ES quando il
      // contesto è isolato.
      preload: join(import.meta.dirname, '../preload/preload.cjs'),
      // Le tre righe che tengono il renderer isolato. Non si toccano.
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    }
  })

  window.once('ready-to-show', () => window.show())

  // Un link esterno apre il browser, non una finestra Electron senza barra
  // degli indirizzi — che sarebbe un ottimo posto per una pagina di phishing.
  window.webContents.setWindowOpenHandler(({ url }) => {
    void shell.openExternal(url)
    return { action: 'deny' }
  })

  const devServer = process.env['ELECTRON_RENDERER_URL']
  if (devServer !== undefined && devServer.length > 0) {
    await window.loadURL(devServer)
  } else {
    await window.loadFile(join(import.meta.dirname, '../renderer/index.html'))
  }
}

app.whenReady().then(main).catch((cause: unknown) => {
  // Anche l'avvio può fallire, e senza questo catch fallirebbe in silenzio con una
  // finestra che non appare mai.
  logger('boot').fatal('avvio non riuscito', cause)
})
