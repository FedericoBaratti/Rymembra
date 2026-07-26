import { app, BrowserWindow, protocol, globalShortcut, net } from 'electron'
import { registerMediaKeys } from './modules/mediaKeys'
import { join } from 'node:path'
import { createReadStream, statSync } from 'node:fs'
import { Readable } from 'node:stream'
import { resolveRange, mimeForPath } from './modules/httpRange'
import { getDb } from './modules/db'
import { runStartupBackup } from './modules/dbBackup'
import { getApiCache } from './modules/net/apiCacheSingleton'
import { getCover } from './modules/coverArt'
import { setMainWindow } from './modules/events'
import { flushSettingsSync, migratePlaintextSecrets, syncNativeTheme } from './modules/settings'
import { flushSecretsSync } from './modules/secrets'
import { flushQueueStateSync } from './modules/queueState'
import { registerLibraryIpc, startLibraryScan, restartWatcher } from './ipc/library.ipc'
import { registerDownloadIpc } from './ipc/download.ipc'
import { registerMetadataIpc } from './ipc/metadata.ipc'
import { registerScrobbleIpc } from './ipc/scrobble.ipc'
import { registerSyncIpc } from './ipc/sync.ipc'
import { registerSpotifyMigrationIpc } from './ipc/spotifyMigration.ipc'
import { registerDiscoveryIpc } from './ipc/discovery.ipc'
import { registerPodcastIpc } from './ipc/podcasts.ipc'
import { registerLanIpc } from './ipc/lan.ipc'
import { registerThermalIpc } from './ipc/thermal.ipc'
import { registerPhoneSyncIpc } from './ipc/phoneSync.ipc'
import { resumeSpotifyMigration } from './modules/spotifyMigration'
import { startSyncService, stopSyncService } from './modules/sync/syncService'
import { stopScrobbler } from './modules/scrobbler/service'
import { startLanServer, stopLanServer } from './modules/lan/server'
import { flushPairingStoreSync } from './modules/lan/pairingStore'

const isDev = !!process.env['ELECTRON_RENDERER_URL']

protocol.registerSchemesAsPrivileged([
  {
    scheme: 'aether',
    privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true }
  }
])

function streamFile(
  path: string,
  rangeHeader: string | null,
  opts?: { mime?: string; cacheControl?: string }
): Response {
  const size = statSync(path).size
  const mime = opts?.mime ?? mimeForPath(path)
  const common: Record<string, string> = {
    'Accept-Ranges': 'bytes',
    'Content-Type': mime,
    'Access-Control-Allow-Origin': '*'
  }
  if (opts?.cacheControl) common['Cache-Control'] = opts.cacheControl

  const range = resolveRange(rangeHeader, size)
  if (range.status === 416) {
    return new Response(null, { status: 416, headers: { ...common, ...range.headers } })
  }
  const stream = Readable.toWeb(
    createReadStream(path, { start: range.start, end: range.end })
  ) as unknown as ReadableStream
  return new Response(stream, { status: range.status, headers: { ...common, ...range.headers } })
}

function registerAetherProtocol(): void {
  protocol.handle('aether', (req) => {
    try {
      const url = new URL(req.url)
      if (url.host === 'media') {
        const trackId = Number(url.pathname.slice(1))
        const row = getDb().prepare('SELECT path FROM tracks WHERE id = ?').get(trackId) as
          | { path: string }
          | undefined
        if (!row) return new Response('Not found', { status: 404 })
        return streamFile(row.path, req.headers.get('range'))
      }
      if (url.host === 'art') {
        const hash = url.pathname.slice(1)
        const thumb = url.searchParams.get('thumb') === '1'
        const cover = getCover(hash, thumb)
        if (!cover) return new Response('Not found', { status: 404 })
        return streamFile(cover.path, req.headers.get('range'), {
          mime: cover.mime,
          cacheControl: 'max-age=31536000, immutable'
        })
      }
      // proxy remote images (download previews) to avoid CSP/CORS issues
      if (url.host === 'remote') {
        const target = url.searchParams.get('url')
        if (!target || !/^https:\/\//.test(target)) {
          return new Response('Bad request', { status: 400 })
        }
        // 30s cap: a stalled CDN must not hold the renderer's request forever.
        return net.fetch(target, { signal: AbortSignal.timeout(30_000) })
      }
      return new Response('Bad request', { status: 400 })
    } catch (err) {
      console.error('aether protocol error', err)
      return new Response('Internal error', { status: 500 })
    }
  })
}

let mainWindow: BrowserWindow | null = null

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 700,
    minHeight: 600,
    show: false,
    backgroundColor: '#09090d',
    titleBarStyle: 'hidden',
    titleBarOverlay: {
      color: '#00000000',
      symbolColor: '#9b9ba8',
      height: 36
    },
    webPreferences: {
      preload: join(import.meta.dirname, '../preload/preload.cjs'),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false
    }
  })

  mainWindow.once('ready-to-show', () => {
    mainWindow?.show()
    // Defer heavy work until the window is interactive
    setTimeout(() => {
      restartWatcher()
      startLibraryScan()
      void runStartupBackup(getDb())
      startSyncService()
      startLanServer()
    }, 600)
  })

  mainWindow.on('closed', () => {
    mainWindow = null
    setMainWindow(null)
  })

  setMainWindow(mainWindow)

  if (isDev) {
    void mainWindow.loadURL(process.env['ELECTRON_RENDERER_URL']!)
  } else {
    void mainWindow.loadFile(join(import.meta.dirname, '../renderer/index.html'))
  }
}

// Dev-only: point userData at a per-instance directory so two copies of the app
// can run side by side against the same Google account (used for the two-device
// sync test). Must run before any getSettings()/getDb() touches the default path.
if (isDev && process.env['AETHER_USER_DATA']) {
  app.setPath('userData', process.env['AETHER_USER_DATA']!)
}

const gotLock = app.requestSingleInstanceLock()
if (!gotLock) {
  app.quit()
} else {
  app.on('second-instance', () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore()
      mainWindow.focus()
    }
  })

  app.whenReady().then(() => {
    // safeStorage is only usable post-ready; must run before any getSettings()
    migratePlaintextSecrets()
    syncNativeTheme()
    // Drop expired API-cache rows once per session (cheap, indexed delete)
    setTimeout(() => {
      try {
        getApiCache().pruneExpired()
      } catch (err) {
        console.error('api cache prune failed', err)
      }
    }, 5_000)
    registerAetherProtocol()
    registerLibraryIpc()
    registerDownloadIpc()
    registerMetadataIpc()
    registerScrobbleIpc()
    registerSyncIpc()
    registerSpotifyMigrationIpc()
    registerDiscoveryIpc()
    registerPodcastIpc()
    registerLanIpc()
    registerThermalIpc()
    registerPhoneSyncIpc()
    registerMediaKeys()
    createWindow()
    // Resume a Spotify migration interrupted by a previous quit (best-effort).
    setTimeout(() => void resumeSpotifyMigration(), 3_000)

    app.on('activate', () => {
      if (BrowserWindow.getAllWindows().length === 0) createWindow()
    })
  })

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit()
  })

  app.on('will-quit', () => {
    globalShortcut.unregisterAll()
    stopSyncService()
    stopLanServer()
    stopScrobbler()
    flushSettingsSync()
    flushSecretsSync()
    flushQueueStateSync()
    flushPairingStoreSync()
  })
}
