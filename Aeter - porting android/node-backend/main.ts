/**
 * nodejs-mobile backend entry point (mobile replacement for electron/main.ts).
 *
 * Lifecycle:
 *   1. Native NodeBackend plugin starts node and sends an `init` message with
 *      the Android directories (HostConfig).
 *   2. We set host paths, start the local media server, register the reused
 *      IPC handlers (electron/ipc/*) and run the deferred bootstrap.
 *   3. Renderer `req` messages are dispatched to the matching IPC handler and
 *      answered with a correlated `res`; backend events are pushed as `event`.
 *
 * The heavy logic (library scan, enrichment, downloads, scrobbler, db) is the
 * SAME code as the desktop app — `electron` is aliased to ./electron-shim here.
 */
// MUST be first: installs fetch / AbortController / AbortSignal.timeout for the
// Node 12 nodejs-mobile runtime before any networking module loads (see file).
import './net-polyfill'
import { resolve, join } from 'path'
import { createFatalHandler, installFatalHandlers } from './fatal'
import {
  setHostConfig,
  setEmit,
  emitReply,
  getHandler,
  registerHandler,
  resolveNative,
  callNative,
  type HostConfig
} from './runtime'
import { startMediaServer } from './server'
import { initSqlite } from './sqlite-shim'
import { setSecretKey, sharedWindow } from './electron-shim'

// Reused, unchanged IPC registrations from the desktop app.
import { registerLibraryIpc, startLibraryScan, restartWatcher } from '../electron/ipc/library.ipc'
import { registerDownloadIpc } from '../electron/ipc/download.ipc'
import { registerMetadataIpc } from '../electron/ipc/metadata.ipc'
import { registerScrobbleIpc } from '../electron/ipc/scrobble.ipc'
import { registerSpotifyMigrationIpc } from '../electron/ipc/spotifyMigration.ipc'
import { registerDiscoveryIpc } from '../electron/ipc/discovery.ipc'
import { registerPodcastIpc } from '../electron/ipc/podcasts.ipc'
import { registerSyncIpc } from '../electron/ipc/sync.ipc'
import { registerThermalIpc } from '../electron/ipc/thermal.ipc'
import { startSyncService } from '../electron/modules/sync/syncService'
import { setNetworkProbe, type NetworkType } from '../electron/modules/sync/fetchMissing'
import {
  resumeSpotifyMigration,
  setMigrationLifecycle
} from '../electron/modules/spotifyMigration'
import { getDb } from '../electron/modules/db'
import { flushQueueStateSync } from '../electron/modules/queueState'
import { flushSecretsSync } from '../electron/modules/secrets'
import { flushSettingsSync } from '../electron/modules/settings'
import { setDownloadLifecycle } from '../electron/modules/downloader'
import { repairSplitAlbums } from '../electron/modules/albumRepair'
import { setMainWindow, broadcast, onBroadcast } from '../electron/modules/events'
import {
  enableAutoCatalog,
  scheduleAutoCatalogRefresh
} from '../electron/modules/auto/autoCatalog'
import { setYtdlpRunner } from '../electron/modules/download/ytdlpRunner'
import { androidYtdlpRunner } from './ytdlp-shim'
import { setTagWriteBack } from '../electron/modules/tagIO'
import { safTagWriteBack } from './tag-saf-shim'
import { setPcmDecoder } from '../electron/modules/enrichment/shazam/pcm'
import { androidPcmDecoder } from './pcm-shim'
import { registerTransferIpc, maybeStartTransferServer } from './transfer/ipc'
import { flushPeersSync } from './transfer/peers'

interface BridgeChannel {
  on(event: 'message', cb: (msg: string) => void): void
  // cordova-bridge / rn-bridge: send(...msg) posts a 'message' event.
  send(msg: string): void
}

/** Resolve the nodejs-mobile bridge channel (cordova/RN variants). */
function getChannel(): BridgeChannel {
  const req = eval('require') as (id: string) => unknown
  for (const id of ['cordova-bridge', 'rn-bridge']) {
    try {
      const mod = req(id) as { channel: BridgeChannel }
      if (mod?.channel) return mod.channel
    } catch {
      /* try next */
    }
  }
  // Fallback (e.g. local dev under plain node): no-op channel.
  return { on: () => {}, send: () => {} }
}

const channel = getChannel()
let mediaBase = ''

setEmit((raw) => channel.send(raw))

// Crash safety net (see fatal.ts): an uncaught error must NOT kill the event
// loop (the engine can't restart in-process) and must NOT lose the debounced
// writes — flush everything synchronously and tell the renderer.
installFatalHandlers(
  createFatalHandler({
    log: (message, err) => console.error(message, err),
    flushes: [
      () => (getDb() as unknown as { flushNow?: () => void }).flushNow?.(),
      flushSettingsSync,
      flushQueueStateSync,
      flushSecretsSync,
      flushPeersSync
    ],
    notify: (kind, message) => broadcast('backend:fatal', { kind, message })
  })
)

// Built-in channel so the renderer can discover the media server origin.
registerHandler('__mediaBase', () => mediaBase)

let booted: Promise<void> | null = null

/** Start boot() once (idempotent). The first renderer request triggers it on
 *  the nodejs-mobile-cordova transport, which never sends an `init` message. */
function ensureBooted(host?: HostConfig): Promise<void> {
  if (!booted) booted = boot(host ?? deriveHostConfig()).catch((e) => console.error('boot failed', e))
  return booted
}

/**
 * Derive the Android directories from the runtime environment.
 *
 * Used on the nodejs-mobile-cordova transport, which (unlike the custom
 * NodeBackendPlugin) never sends an `init` message: the plugin just starts the
 * engine. We reconstruct HostConfig ourselves so `boot()` can run.
 *
 * At runtime main.js lives at `<filesDir>/www/nodejs-project/main.js`, so
 * `resolve(__dirname, '../..')` yields the same `filesDir` the native plugin
 * would pass (`context.filesDir`). The cordova plugin sets TMPDIR to cacheDir.
 *
 * `nativeLibraryDir` arrives via the AETHER_NATIVE_LIB_DIR env var, exported by
 * MainActivity.onCreate with Os.setenv (same mechanism the cordova plugin uses
 * for TMPDIR — node runs in the same process, so process.env sees it). It feeds
 * binaries.ts (lib*.so executables such as libfpcalc.so, shipped via jniLibs).
 */
function deriveHostConfig(): HostConfig {
  const filesDir = resolve(__dirname, '../..')
  const cacheDir = process.env['TMPDIR'] || join(filesDir, 'cache')
  return {
    dataDir: filesDir,
    filesDir,
    cacheDir,
    musicDir: join(filesDir, 'Music'),
    nativeLibraryDir: process.env['AETHER_NATIVE_LIB_DIR'] || ''
  }
}

async function boot(host: HostConfig): Promise<void> {
  setHostConfig(host)
  // Wire the broadcast sink so events.ts broadcast() actually emits. Without this
  // `win` stays null and EVERY backend→renderer event (scan:progress,
  // library:changed, track:updated, download:updated, media-key, …) is silently
  // dropped — the scan completes but the UI never refreshes. The desktop app does
  // this in electron/main.ts via setMainWindow(mainWindow); the mobile shim exposes
  // a single sharedWindow whose webContents.send routes 'aether:event' to emitEvent.
  setMainWindow(sharedWindow)
  // Expose the native lib dir to binaries.ts (where ARM lib*.so executables live).
  // Conditional: on the cordova transport the env var is already set by
  // MainActivity (Os.setenv) and host derives from it — never blank it out.
  if (host.nativeLibraryDir) process.env['AETHER_NATIVE_LIB_DIR'] = host.nativeLibraryDir

  // yt-dlp can't run as a standalone binary on bionic; route it through the
  // youtubedl-android library (YtDlp plugin) instead of spawning a binary.
  setYtdlpRunner(androidYtdlpRunner)

  // Keep a foreground service alive during a Spotify migration so Android does
  // not kill this process (and the WebView the yt-dlp reverse-RPC depends on)
  // while the app is backgrounded. Best-effort: failures never break migration.
  setMigrationLifecycle({
    onStart: (total) => void callNative('migrationServiceStart', { total }, 8000).catch(() => {}),
    onProgress: (done, total) =>
      void callNative('migrationServiceProgress', { done, total }, 8000).catch(() => {}),
    onStop: () => void callNative('migrationServiceStop', undefined, 8000).catch(() => {})
  })

  // Aggregate download notification (YouTube + single Spotify): keeps the process
  // alive and shows live percentage while backgrounded. The intra-track percent is
  // pushed natively from YtDlpPlugin's progress callback; these calls carry the
  // title + active/total counts the backend knows.
  setDownloadLifecycle({
    onStart: () => void callNative('downloadServiceStart', { title: '', total: 0 }, 8000).catch(() => {}),
    onProgress: (active, total, percent, title) =>
      void callNative('downloadServiceProgress', { active, total, percent, title }, 8000).catch(() => {}),
    onStop: () => void callNative('downloadServiceStop', undefined, 8000).catch(() => {})
  })

  // Route tag writes through SAF: on Android shared storage the audio path is
  // read-only (READ_MEDIA_AUDIO), so node-taglib-sharp's in-place save() throws
  // EACCES. tagIO edits a private temp copy (under cacheDir) and this seam copies
  // it back over the original via the FileAccess plugin's saveFileViaSaf.
  setTagWriteBack(safTagWriteBack, host.cacheDir)

  // Shazam fingerprint PCM: no ffmpeg binary on Android — decode via the
  // native AudioDecode plugin (MediaCodec) instead of the desktop default.
  setPcmDecoder(androidPcmDecoder)

  // Network gate for the missing-track auto-fetch: read the live Android
  // connection type via ConnectivityManager (reverse-RPC through the WebView) so
  // the headless nodejs-mobile worker can honour the user's "only on Wi-Fi"
  // policy. A failed/timed-out probe returns 'unknown' → treated as allowed.
  setNetworkProbe(async (): Promise<NetworkType> => {
    const res = await callNative('getNetworkType', undefined, 5000)
    const t = (res as { type?: string } | null)?.type
    return t === 'wifi' || t === 'cellular' || t === 'ethernet' || t === 'none'
      ? (t as NetworkType)
      : 'unknown'
  })

  // Fetch the Keystore-backed encryption key so safeStorage can encrypt secrets
  // at rest. Fire-and-forget: NEVER block boot on it. The SecureStore reverse-RPC
  // can be slow or unavailable (plugin missing → the call times out), and making
  // the UI wait on it stalls every cold start. Secrets degrade to plaintext until
  // the key arrives (the SecretCodec seam reports encryption off) — acceptable,
  // and on a fresh install there are no secrets to decrypt yet.
  void callNative('getSecureKey', undefined, 5000)
    .then((res) => {
      const keyB64 = (res as { keyB64?: string })?.keyB64
      if (keyB64) setSecretKey(keyB64)
    })
    .catch((err) => console.error('secure key unavailable; secrets stored unencrypted', err))

  // Bring up the WASM SQLite engine, the DB and the local media server. Wrapped
  // in try/catch so that a failure here can NEVER skip the IPC registration
  // below: if it did, every renderer request would resolve to "unknown channel"
  // and the UI would hang on its loading skeletons forever (the exact symptom we
  // are fixing). getDb() itself no longer throws on a corrupt file — the sqlite
  // shim recovers — but initSqlite()/startMediaServer() still can.
  try {
    await initSqlite()
    getDb()
    const port = await startMediaServer()
    mediaBase = `http://127.0.0.1:${port}`
  } catch (err) {
    console.error('db/media server init failed; registering IPC anyway', err)
  }

  // Register the reused IPC surface. Always reached, regardless of the above.
  registerLibraryIpc()
  registerDownloadIpc()
  registerMetadataIpc()
  registerScrobbleIpc()
  registerSpotifyMigrationIpc()
  registerDiscoveryIpc()
  registerPodcastIpc()
  registerSyncIpc()
  registerThermalIpc()
  registerTransferIpc()

  // Android Auto: keep an app-private browse-catalog snapshot fresh so the native
  // MediaBrowserService can serve the car without the WebView/Node running.
  // Enabled only here (mobile backend) — a no-op on desktop. Rebuilds on any
  // library:changed (covers download/enrichment/metadata/migration) plus setLiked.
  enableAutoCatalog()
  onBroadcast((event) => {
    if (event === 'library:changed') scheduleAutoCatalogRefresh()
  })

  // Deferred startup tasks (mirror electron/main.ts onReady deferral).
  setTimeout(() => {
    try {
      restartWatcher()
      startLibraryScan()
      startSyncService()
      // "Riparazione da PC": resume the LAN transfer server if the user left it on.
      maybeStartTransferServer()
    } catch (err) {
      console.error('startup tasks failed', err)
    }
    // Seed the Auto catalog from the already-persisted DB so the car has content
    // immediately, even before the first scan completes.
    scheduleAutoCatalogRefresh()
    // Resume a Spotify migration that was interrupted by an app/process kill.
    void resumeSpotifyMigration().catch((err) =>
      console.error('spotify migration resume failed', err)
    )
    // One-time-ish heal of albums split by a legacy inconsistent album_artist
    // (idempotent — does nothing once aligned). Deferred so it doesn't race the
    // initial scan's file writes; it heals from the already-persisted DB rows.
    setTimeout(() => {
      void repairSplitAlbums()
        .then((res) => {
          if (res.retagged > 0) broadcast('library:changed', { reason: 'repair' })
        })
        .catch((err) => console.error('repairSplitAlbums boot failed', err))
    }, 8000)
  }, 600)
}

async function dispatch(id: number, channelName: string, args: unknown[]): Promise<void> {
  // Wait for boot() to finish registering the reused IPC handlers. Without this
  // an early renderer request (getSettings/refreshAll fired by useAppBootstrap)
  // arrives before registration and gets rejected as "unknown channel",
  // leaving the Settings/Library skeletons stuck loading forever.
  await ensureBooted()
  const handler = getHandler(channelName)
  if (!handler) {
    emitReply(id, false, `unknown channel: ${channelName}`)
    return
  }
  try {
    const result = await handler({ sender: null }, ...args)
    emitReply(id, true, result ?? null)
  } catch (err) {
    emitReply(id, false, err instanceof Error ? err.message : String(err))
  }
}

channel.on('message', (raw) => {
  let msg: { t: string; [k: string]: unknown }
  try {
    msg = JSON.parse(raw)
  } catch {
    return
  }
  if (msg.t === 'init') {
    // Custom NodeBackend plugin path: boot with the host-provided directories.
    ensureBooted(msg.host as HostConfig)
    return
  }
  if (msg.t === 'req') {
    void dispatch(msg.id as number, msg.channel as string, (msg.args as unknown[]) ?? [])
    return
  }
  if (msg.t === 'nres') {
    // Reply to a callNative() reverse-RPC (native plugin result).
    resolveNative(msg.nid as number, msg.ok as boolean, msg.ok ? msg.result : msg.error)
  }
})

// nodejs-mobile-cordova never sends `init`. Self-bootstrap with a derived
// HostConfig so the backend is ready even if no renderer request arrives first.
// `ensureBooted()` is idempotent, so a native `init` (custom plugin) or an early
// request still wins if it gets there first. Kept short — the dispatch path also
// triggers boot — but a small delay lets a native `init` win on that transport.
setTimeout(() => void ensureBooted(), 300)
