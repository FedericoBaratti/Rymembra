/**
 * Shared runtime wiring for the nodejs-mobile backend.
 *
 * Holds the values the Android host injects at startup (app directories,
 * native library dir for binaries) and the transport callbacks used to talk
 * to the renderer over the nodejs-mobile bridge. The electron shim and the
 * backend entry both read/write this module so the reused electron/modules
 * code keeps working unchanged.
 */

export interface HostConfig {
  /** Per-app private writable dir (≈ Electron userData). */
  dataDir: string
  /** App files dir (settings, db, queue state). */
  filesDir: string
  /** Cache dir. */
  cacheDir: string
  /** Music output dir for downloads. */
  musicDir: string
  /** nativeLibraryDir — where lib*.so ARM binaries live (executable). */
  nativeLibraryDir: string
}

let host: HostConfig = {
  dataDir: '',
  filesDir: '',
  cacheDir: '',
  musicDir: '',
  nativeLibraryDir: ''
}

export function setHostConfig(cfg: HostConfig): void {
  host = cfg
}

export function getHostConfig(): HostConfig {
  return host
}

/** Maps Electron getPath() names onto Android dirs. */
export function resolveAppPath(name: string): string {
  switch (name) {
    case 'userData':
    case 'appData':
      return host.dataDir
    case 'temp':
    case 'cache':
      return host.cacheDir
    case 'music':
      return host.musicDir
    case 'logs':
      return host.filesDir
    default:
      return host.filesDir
  }
}

// --- Transport: backend → renderer push (events) -------------------------

type Emit = (raw: string) => void
let emitFn: Emit = () => {}

export function setEmit(fn: Emit): void {
  emitFn = fn
}

/** Push an `aether:event` to the renderer (used by the BrowserWindow shim). */
export function emitEvent(name: string, payload: unknown): void {
  emitFn(JSON.stringify({ t: 'event', name, payload }))
}

/** Push a request reply to the renderer. */
export function emitReply(id: number, ok: boolean, body: unknown): void {
  emitFn(
    ok
      ? JSON.stringify({ t: 'res', id, ok: true, result: body })
      : JSON.stringify({ t: 'res', id, ok: false, error: String(body) })
  )
}

// --- Reverse-RPC: backend → native (routed through the renderer/WebView) -
//
// The node-backend can only reach Android-native code via the WebView that
// hosts the Capacitor plugins. callNative() emits an `nrpc` message; the
// renderer (src/lib/bridge.ts) invokes the matching Capacitor plugin and posts
// back an `nres` reply, which resolveNative() correlates by id.

let nativeSeq = 0
const nativePending = new Map<
  number,
  { resolve: (v: unknown) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }
>()

/** Call an Android-native plugin method and await its result. */
export function callNative(method: string, args?: unknown, timeoutMs = 30000): Promise<unknown> {
  const nid = ++nativeSeq
  return new Promise<unknown>((resolve, reject) => {
    const timer = setTimeout(() => {
      nativePending.delete(nid)
      reject(new Error(`native call timed out: ${method}`))
    }, timeoutMs)
    nativePending.set(nid, { resolve, reject, timer })
    emitFn(JSON.stringify({ t: 'nrpc', nid, method, args: args ?? null }))
  })
}

/** Resolve a pending native call (called from the backend message loop). */
export function resolveNative(nid: number, ok: boolean, body: unknown): void {
  const entry = nativePending.get(nid)
  if (!entry) return
  nativePending.delete(nid)
  clearTimeout(entry.timer)
  if (ok) entry.resolve(body)
  else entry.reject(new Error(String(body)))
}

// --- IPC dispatch registry (filled by the ipcMain shim) ------------------

export type IpcHandler = (event: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

export function registerHandler(channel: string, fn: IpcHandler): void {
  handlers.set(channel, fn)
}

export function getHandler(channel: string): IpcHandler | undefined {
  return handlers.get(channel)
}
