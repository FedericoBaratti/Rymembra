import { registerPlugin } from '@capacitor/core'

/**
 * Renderer-side dispatcher for backend → native reverse-RPC (see the `nrpc`
 * message handling in src/lib/bridge.ts and callNative() in
 * node-backend/runtime.ts).
 *
 * The node-backend cannot reach Android-native code directly; it asks the
 * renderer (which hosts the Capacitor plugins) to invoke a plugin method and
 * return the result. Each `method` here maps to one of the custom Kotlin
 * plugins (FileAccess / ImageResize / SecureStore — see
 * android/app/src/main/java/com/aether/player/*Plugin.kt).
 */

interface FileAccessPlugin {
  /** ACTION_OPEN_DOCUMENT_TREE → persisted real filesystem path (or canceled). */
  pickFolder(): Promise<{ canceled: boolean; path?: string }>
  /** Reveal a file in a file-manager Intent. */
  showInFolder(options: { path: string }): Promise<void>
  /** Open a URL in a Custom Tab / browser Intent. */
  openExternal(options: { url: string }): Promise<void>
  /** Scans the folder using SAF and returns metadata for all files. */
  scanFolder(options: { uri: string }): Promise<{ files: Array<{ name: string, size: number, mimeType: string, uri: string }> }>
  /** Copies a locally-edited temp file back over the original on shared storage via SAF. */
  saveFileViaSaf(options: { originalPath: string; tempPath: string }): Promise<{ ok: boolean }>
  /** Permanently deletes a file on shared storage via SAF (or a direct delete
   *  under All-Files-Access). Rejects when no grant covers the path. */
  deleteFile(options: { path: string }): Promise<{ ok: boolean }>
  /** Creates a NEW document in a granted SAF folder from a temp file (phone
   *  repair commit when the extension changes, e.g. .opus → .mp3). */
  importFileViaSaf(options: {
    folderPath: string
    fileName: string
    tempPath: string
  }): Promise<{ ok: boolean; path: string }>
  /** All Files Access (MANAGE_EXTERNAL_STORAGE) state — needed to create/manage Download/Music. */
  hasAllFilesAccess(): Promise<{ granted: boolean }>
  /** Opens the system "All files access" screen and re-reads the grant on return. */
  requestAllFilesAccess(): Promise<{ granted: boolean }>
  /** Ensures `<external>/Download/Music` exists; returns its real path (always). */
  ensureDownloadMusicFolder(): Promise<{ path: string; existed: boolean }>
  /** Live connection type (ConnectivityManager) so the backend can gate the
   *  missing-track auto-fetch on the user's Wi-Fi-only policy. */
  getNetworkType(): Promise<{ type: 'wifi' | 'cellular' | 'ethernet' | 'none' }>
  /** Full process restart (RestartActivity trampoline) — the only recovery when
   *  the nodejs-mobile engine is dead. Renderer-driven (BackendDownBanner). */
  restartApp(): Promise<void>
}

interface ImageResizePlugin {
  /** Resize image using temp files to avoid base64 bridge overhead. */
  resize(options: {
    srcPath: string
    destPath: string
    width: number
    height: number
    quality: number
  }): Promise<void>
  /** Read image dimensions without a full decode (inJustDecodeBounds). */
  probe(options: { srcPath: string }): Promise<{ width: number; height: number }>
}

interface AudioDecodePlugin {
  /** Decode a segment of an audio file to raw s16le PCM (MediaCodec). */
  decode(options: {
    srcPath: string
    destPath: string
    offsetSec: number
    durationSec: number
  }): Promise<{ sampleRate: number; channels: number }>
}

interface SecureStorePlugin {
  /** Get-or-create a 32-byte AES key in the Android Keystore (base64). */
  getKey(): Promise<{ keyB64: string }>
  /** Generic encrypted key-value read/write, used directly by the renderer
   *  (not reverse-RPC) to persist the LAN pairing (src/lib/lanClient.ts). */
  getValue(options: { key: string }): Promise<{ value: string | null }>
  setValue(options: { key: string; value: string }): Promise<void>
  deleteValue(options: { key: string }): Promise<void>
}

interface YtDlpPlugin {
  /**
   * Run yt-dlp with the given argv via the youtubedl-android library (bundled
   * Python + ffmpeg). One-shot: resolves with the aggregated process output.
   * `processId` lets cancel() target this exact run.
   */
  run(options: {
    args: string[]
    processId?: string
  }): Promise<{ code: number; stdout: string; stderr: string }>
  /** Terminate a running yt-dlp process started with the given processId. */
  cancel(options: { processId: string }): Promise<{ killed: boolean }>
  /** Current yt-dlp version string (empty until the first successful update). */
  version(): Promise<{ version: string }>
  /** Force a yt-dlp update now (bypasses the 24h cache); rejects YTDLP_BUSY
   *  while running downloads hold the package. */
  update(): Promise<{ status: string; version: string }>
}

interface MigrationServicePlugin {
  /** Start a foreground service so Android keeps the process alive during a
   *  long Spotify migration (and thus the WebView the yt-dlp RPC depends on). */
  start(options: { total: number }): Promise<void>
  progress(options: { done: number; total: number }): Promise<void>
  stop(): Promise<void>
}

interface DownloadNotificationPlugin {
  /** Start the aggregate download foreground-service + progress notification. */
  start(options: { title: string; total: number }): Promise<void>
  /** Update title / active-count / total / percent on the running notification. */
  progress(options: { title: string; active: number; total: number; percent: number }): Promise<void>
  stop(): Promise<void>
}

interface TransferServicePlugin {
  /** FGS (dataSync) + WakeLock + high-perf WifiLock for a desktop repair session. */
  start(): Promise<void>
  progress(options: { done: number; total: number }): Promise<void>
  stop(): Promise<void>
  /** Human-readable device name (manufacturer + model) for pairing/mDNS TXT. */
  deviceName(): Promise<{ name: string }>
}

interface LanDiscoveryRegisterPlugin {
  /** Advertise the phone transfer server over NSD (`_aether-transfer._tcp`). */
  registerService(options: {
    port: number
    deviceId: string
    deviceName: string
  }): Promise<void>
  unregisterService(): Promise<void>
}

const FileAccess = registerPlugin<FileAccessPlugin>('FileAccess')
/** Same FileAccess plugin, exported for renderer-driven (non reverse-RPC) calls
 *  such as the Android storage auto-setup (see src/lib/androidStorage.ts). */
export const FileAccessNative = FileAccess
const ImageResize = registerPlugin<ImageResizePlugin>('ImageResize')
const AudioDecode = registerPlugin<AudioDecodePlugin>('AudioDecode')
const SecureStore = registerPlugin<SecureStorePlugin>('SecureStore')
/** Same SecureStore plugin, exported for renderer-driven (non reverse-RPC)
 *  calls — LAN pairing persistence (src/lib/lanClient.ts). */
export const SecureStoreNative = SecureStore
const YtDlp = registerPlugin<YtDlpPlugin>('YtDlp')
/** Same YtDlp plugin, exported for renderer-driven (non reverse-RPC) calls —
 *  the Settings diagnostics row (yt-dlp version + forced update). */
export const YtDlpNative = YtDlp
const MigrationService = registerPlugin<MigrationServicePlugin>('MigrationService')
const DownloadNotification = registerPlugin<DownloadNotificationPlugin>('DownloadNotification')
const TransferService = registerPlugin<TransferServicePlugin>('TransferService')
// Same LanDiscovery plugin lanClient.ts browses with; here only the advertise
// half is needed (reverse-RPC from the phone's transfer server).
const LanDiscoveryRegister = registerPlugin<LanDiscoveryRegisterPlugin>('LanDiscovery')

/**
 * Per-method timeouts (ms). A native plugin that never replies would otherwise
 * leave the backend caller (callNative in node-backend/runtime.ts) hung forever,
 * which on Android has stalled boot/scan in the past (e.g. getSecureKey). A method
 * with no entry here is treated as unbounded — `pickFolder` waits on the user
 * browsing the SAF tree, so it must never time out.
 */
const RPC_TIMEOUT_MS: Record<string, number> = {
  // Just above the backend's 30-min DOWNLOAD_TIMEOUT_MS (ytdlp-shim.ts): a hung
  // native yt-dlp worker (exhausted pool, stuck Python) must not leave the
  // reverse-RPC promise pending forever on the renderer side.
  ytdlpRun: 31 * 60_000,
  showInFolder: 8000,
  openExternal: 8000,
  // Image ops run on a bounded native pool (ImageResizePlugin, 3 threads);
  // values allow for a couple of queued jobs ahead, and stay below the
  // backend's 30s callNative default so this error surfaces first.
  imageResize: 20000,
  imageProbe: 10000,
  // Below the backend's 40s (pcm-shim.ts) for the same reason; covers the
  // decode plus the native codec-init retry backoff (AudioDecodePlugin).
  audioDecode: 30000,
  getSecureKey: 8000,
  scanFolder: 60000,
  saveFileViaSaf: 30000,
  deleteFile: 15000,
  getNetworkType: 5000,
  ytdlpCancel: 8000,
  migrationServiceStart: 8000,
  migrationServiceProgress: 8000,
  migrationServiceStop: 8000,
  downloadServiceStart: 8000,
  downloadServiceProgress: 8000,
  downloadServiceStop: 8000,
  transferServiceStart: 8000,
  transferServiceProgress: 8000,
  transferServiceStop: 8000,
  transferDeviceName: 5000,
  lanRegisterService: 8000,
  lanUnregisterService: 8000,
  importFileViaSaf: 60000
}

function withTimeout<T>(method: string, p: Promise<T>): Promise<T> {
  const ms = RPC_TIMEOUT_MS[method]
  if (!ms) return p
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`native ${method} timed out after ${ms}ms`)),
      ms
    )
    p.then(
      (v) => { clearTimeout(timer); resolve(v) },
      (e) => { clearTimeout(timer); reject(e) }
    )
  })
}

/** Route a reverse-RPC method name to its Capacitor plugin call. */
export async function nativeDispatch(method: string, args: unknown): Promise<unknown> {
  return withTimeout(method, dispatchRaw(method, (args ?? {}) as Record<string, unknown>))
}

async function dispatchRaw(method: string, a: Record<string, unknown>): Promise<unknown> {
  switch (method) {
    case 'pickFolder':
      return FileAccess.pickFolder()
    case 'showInFolder':
      return FileAccess.showInFolder({ path: String(a.path ?? '') })
    case 'openExternal':
      return FileAccess.openExternal({ url: String(a.url ?? '') })
    case 'imageResize':
      return ImageResize.resize({
        srcPath: String(a.srcPath ?? ''),
        destPath: String(a.destPath ?? ''),
        width: Number(a.width ?? 0),
        height: Number(a.height ?? 0),
        quality: Number(a.quality ?? 80)
      })
    case 'imageProbe':
      return ImageResize.probe({ srcPath: String(a.srcPath ?? '') })
    case 'audioDecode':
      return AudioDecode.decode({
        srcPath: String(a.srcPath ?? ''),
        destPath: String(a.destPath ?? ''),
        offsetSec: Number(a.offsetSec ?? 0),
        durationSec: Number(a.durationSec ?? 12)
      })
    case 'getSecureKey':
      return SecureStore.getKey()
    case 'ytdlpRun':
      return YtDlp.run({
        args: (a.args as string[]) ?? [],
        processId: a.processId as string | undefined
      })
    case 'ytdlpCancel':
      return YtDlp.cancel({ processId: String(a.processId ?? '') })
    case 'migrationServiceStart':
      return MigrationService.start({ total: Number(a.total ?? 0) })
    case 'migrationServiceProgress':
      return MigrationService.progress({ done: Number(a.done ?? 0), total: Number(a.total ?? 0) })
    case 'migrationServiceStop':
      return MigrationService.stop()
    case 'downloadServiceStart':
      return DownloadNotification.start({
        title: String(a.title ?? ''),
        total: Number(a.total ?? 0)
      })
    case 'downloadServiceProgress':
      return DownloadNotification.progress({
        title: String(a.title ?? ''),
        active: Number(a.active ?? 0),
        total: Number(a.total ?? 0),
        percent: Number(a.percent ?? 0)
      })
    case 'downloadServiceStop':
      return DownloadNotification.stop()
    case 'saveFileViaSaf':
      return FileAccess.saveFileViaSaf({
        originalPath: String(a.originalPath ?? ''),
        tempPath: String(a.tempPath ?? '')
      })
    case 'importFileViaSaf':
      return FileAccess.importFileViaSaf({
        folderPath: String(a.folderPath ?? ''),
        fileName: String(a.fileName ?? ''),
        tempPath: String(a.tempPath ?? '')
      })
    case 'deleteFile':
      return FileAccess.deleteFile({ path: String(a.path ?? '') })
    case 'getNetworkType':
      return FileAccess.getNetworkType()
    case 'transferServiceStart':
      return TransferService.start()
    case 'transferServiceProgress':
      return TransferService.progress({
        done: Number(a.done ?? 0),
        total: Number(a.total ?? 0)
      })
    case 'transferServiceStop':
      return TransferService.stop()
    case 'transferDeviceName':
      return TransferService.deviceName()
    case 'lanRegisterService':
      return LanDiscoveryRegister.registerService({
        port: Number(a.port ?? 0),
        deviceId: String(a.deviceId ?? ''),
        deviceName: String(a.deviceName ?? '')
      })
    case 'lanUnregisterService':
      return LanDiscoveryRegister.unregisterService()
    default:
      throw new Error(`unknown native method: ${method}`)
  }
}
