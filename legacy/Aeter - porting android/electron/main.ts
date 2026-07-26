import { app, BrowserWindow, protocol, globalShortcut, net } from 'electron'
import { registerMediaKeys } from './modules/mediaKeys'
import { join, extname } from 'node:path'
import { createReadStream, statSync } from 'node:fs'
import { Readable } from 'node:stream'
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
import { registerSpotifyMigrationIpc } from './ipc/spotifyMigration.ipc'
import { registerDiscoveryIpc } from './ipc/discovery.ipc'
import { registerPodcastIpc } from './ipc/podcasts.ipc'
import { registerSyncIpc } from './ipc/sync.ipc'
import { startSyncService, stopSyncService } from './modules/sync/syncService'

const isDev = !!process.env['ELECTRON_RENDERER_URL']

protocol.registerSchemesAsPrivileged([
  {
    scheme: 'aether',
    privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true }
  }
])

const MIME: Record<string, string> = {
  '.mp3': 'audio/mpeg',
  '.flac': 'audio/flac',
  '.m4a': 'audio/mp4',
  '.aac': 'audio/aac',
  '.ogg': 'audio/ogg',
  '.opus': 'audio/ogg',
  '.wav': 'audio/wav',
  '.aiff': 'audio/aiff',
  '.aif': 'audio/aiff',
  '.wma': 'audio/x-ms-wma'
}

function streamFile(
  path: string,
  rangeHeader: string | null,
  opts?: { mime?: string; cacheControl?: string }
): Response {
  const size = statSync(path).size
  const mime = opts?.mime ?? MIME[extname(path).toLowerCase()] ?? 'application/octet-stream'
  const common: Record<string, string> = {
    'Accept-Ranges': 'bytes',
    'Content-Type': mime,
    'Access-Control-Allow-Origin': '*'
  }
  if (opts?.cacheControl) common['Cache-Control'] = opts.cacheControl

  if (rangeHeader) {
    const m = /bytes=(\d*)-(\d*)/.exec(rangeHeader)
    if (m) {
      const start = m[1] ? parseInt(m[1], 10) : 0
      const end = m[2] ? Math.min(parseInt(m[2], 10), size - 1) : size - 1
      if (start <= end && start < size) {
        const stream = Readable.toWeb(
          createReadStream(path, { start, end })
        ) as unknown as ReadableStream
        return new Response(stream, {
          status: 206,
          headers: {
            ...common,
            'Content-Range': `bytes ${start}-${end}/${size}`,
            'Content-Length': String(end - start + 1)
          }
        })
      }
      return new Response(null, { status: 416, headers: { 'Content-Range': `bytes */${size}` } })
    }
  }

  const stream = Readable.toWeb(createReadStream(path)) as unknown as ReadableStream
  return new Response(stream, {
    status: 200,
    headers: { ...common, 'Content-Length': String(size) }
  })
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
        return net.fetch(target)
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
    registerSpotifyMigrationIpc()
    registerDiscoveryIpc()
    registerPodcastIpc()
    registerSyncIpc()
    registerMediaKeys()
    createWindow()

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
    flushSettingsSync()
    flushSecretsSync()
    flushQueueStateSync()
  })
}
