/**
 * Drop-in replacement for the `electron` module under nodejs-mobile.
 *
 * The mobile node-backend Vite build aliases `electron` → this file, so every
 * `import { app, ipcMain, ... } from 'electron'` in electron/modules and
 * electron/ipc resolves here. Only the surface actually used by the backend
 * is implemented (see grep of `from 'electron'`): app, ipcMain, BrowserWindow,
 * net, safeStorage, protocol, globalShortcut, dialog, shell, nativeTheme.
 *
 * The desktop Electron build is unaffected — it keeps using the real module.
 */
import { resolveAppPath, registerHandler, emitEvent, callNative } from './runtime'

// --- app -----------------------------------------------------------------

type Listener = (...args: unknown[]) => void
const appEvents = new Map<string, Listener[]>()

interface AppShim {
  isPackaged: boolean
  getPath(name: string): string
  getAppPath(): string
  getName(): string
  whenReady(): Promise<void>
  on(event: string, cb: Listener): AppShim
  once(event: string, cb: Listener): AppShim
  removeAllListeners(): AppShim
  requestSingleInstanceLock(): boolean
  quit(): void
  exit(): void
}

export const app: AppShim = {
  isPackaged: true,
  getPath(name: string): string {
    return resolveAppPath(name)
  },
  getAppPath(): string {
    return resolveAppPath('userData')
  },
  getName(): string {
    return 'Aether'
  },
  whenReady(): Promise<void> {
    return Promise.resolve()
  },
  on(event: string, cb: Listener): AppShim {
    const list = appEvents.get(event) ?? []
    list.push(cb)
    appEvents.set(event, list)
    return app
  },
  once(event: string, cb: Listener): AppShim {
    return app.on(event, cb)
  },
  removeAllListeners(): AppShim {
    appEvents.clear()
    return app
  },
  // Single-instance is meaningless on Android; always "primary".
  requestSingleInstanceLock(): boolean {
    return true
  },
  quit(): void {
    /* lifecycle handled by Android Activity */
  },
  exit(): void {
    /* no-op */
  }
}

/** Invoke registered app event listeners (used by the backend entry). */
export function fireAppEvent(event: string, ...args: unknown[]): void {
  for (const cb of appEvents.get(event) ?? []) cb(...args)
}

// --- ipcMain -------------------------------------------------------------

export interface IpcMainInvokeEvent {
  sender: unknown
}

export const ipcMain = {
  handle(channel: string, fn: (event: IpcMainInvokeEvent, ...args: never[]) => unknown): void {
    registerHandler(channel, fn as (event: unknown, ...args: unknown[]) => unknown)
  },
  on(): void {
    /* push-style ipc.on is unused by the backend */
  },
  removeHandler(): void {
    /* no-op */
  }
}

// --- BrowserWindow (only webContents.send is used, via events.ts) --------

export class BrowserWindow {
  webContents = {
    send: (channel: string, name: string, payload: unknown): void => {
      // electron/modules/events.ts calls send('aether:event', name, payload)
      if (channel === 'aether:event') emitEvent(name, payload)
    }
  }

  isDestroyed(): boolean {
    return false
  }

  static getAllWindows(): BrowserWindow[] {
    return [sharedWindow]
  }

  // pickFolder() resolves a window from the invoke event sender; the single
  // shared window is fine since there is only one WebView.
  static fromWebContents(_sender: unknown): BrowserWindow {
    return sharedWindow
  }
}

/** A single shared "window" the backend uses as the broadcast sink. */
export const sharedWindow = new BrowserWindow()

// --- net -----------------------------------------------------------------

export const net = {
  // Node 18+ (and nodejs-mobile) provides a global fetch.
  fetch: (input: Parameters<typeof fetch>[0], init?: Parameters<typeof fetch>[1]): Promise<Response> =>
    fetch(input, init)
}

// --- safeStorage ---------------------------------------------------------
// Real at-rest encryption backed by a 32-byte key kept in the Android Keystore
// (EncryptedSharedPreferences). The key is fetched once at boot via reverse-RPC
// (setSecretKey) so encrypt/decrypt can stay synchronous, matching the
// SecretCodec seam in electron/modules/secrets.ts. If the key is unavailable
// (plugin missing / fetch failed) this degrades to an identity transform with
// encryption reported off — the secrets file is still app-private/sandboxed.
import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto'

let secretKey: Buffer | null = null

/** Version marker prepended to AES-GCM ciphertext to tell it from plaintext blobs. */
const ENC_VERSION = 0x01

/** Install the Keystore-backed AES key fetched from SecureStorePlugin. */
export function setSecretKey(keyB64: string): void {
  const key = Buffer.from(keyB64, 'base64')
  secretKey = key.length === 32 ? key : null
}

export const safeStorage = {
  isEncryptionAvailable(): boolean {
    return secretKey != null
  },
  encryptString(plain: string): Buffer {
    if (!secretKey) return Buffer.from(plain, 'utf8')
    const iv = randomBytes(12)
    const cipher = createCipheriv('aes-256-gcm', secretKey, iv)
    const enc = Buffer.concat([cipher.update(plain, 'utf8'), cipher.final()])
    // Layout: version(1)=0x01 | iv(12) | authTag(16) | ciphertext. The version
    // byte distinguishes real ciphertext from values written as plaintext while
    // the key was still loading (see decryptString). 0x01 is a control byte that
    // can't begin a real UTF-8 secret, so plaintext is never mistaken for it.
    return Buffer.concat([Buffer.from([ENC_VERSION]), iv, cipher.getAuthTag(), enc])
  },
  decryptString(buf: Buffer): string {
    // No key, or an untagged blob → it was stored as plaintext (key unavailable
    // at write time). Return it verbatim instead of GCM-decrypting garbage.
    if (!secretKey || buf[0] !== ENC_VERSION) return Buffer.from(buf).toString('utf8')
    const iv = buf.subarray(1, 13)
    const tag = buf.subarray(13, 29)
    const data = buf.subarray(29)
    const decipher = createDecipheriv('aes-256-gcm', secretKey, iv)
    decipher.setAuthTag(tag)
    return Buffer.concat([decipher.update(data), decipher.final()]).toString('utf8')
  }
}

// --- protocol (replaced by the local HTTP server) -----------------------

export const protocol = {
  registerSchemesAsPrivileged(): void {
    /* no custom scheme on mobile; see node-backend/server.ts */
  },
  handle(): void {
    /* no-op */
  }
}

// --- globalShortcut (no global media keys on Android; MediaSession plugin) -

export const globalShortcut = {
  register(_accelerator?: string, _callback?: () => void): boolean {
    return false
  },
  registerAll(_accelerators?: string[], _callback?: () => void): void {
    /* no-op */
  },
  unregister(_accelerator?: string): void {
    /* no-op */
  },
  unregisterAll(): void {
    /* no-op */
  },
  isRegistered(_accelerator?: string): boolean {
    return false
  }
}

// --- dialog (folder picker → SAF plugin, wired in M4) --------------------

export const dialog = {
  async showOpenDialog(
    _window?: unknown,
    _options?: unknown
  ): Promise<{ canceled: boolean; filePaths: string[] }> {
    // Routed to the SAF FileAccess plugin (ACTION_OPEN_DOCUMENT_TREE) via
    // reverse-RPC; the native side resolves a real filesystem path.
    try {
      const res = (await callNative('pickFolder')) as { canceled: boolean; path?: string }
      if (res.canceled || !res.path) return { canceled: true, filePaths: [] }
      return { canceled: false, filePaths: [res.path] }
    } catch {
      return { canceled: true, filePaths: [] }
    }
  }
}

// --- shell (reveal in folder / open external → Android Intents) ----------

export const shell = {
  showItemInFolder(path: string): void {
    void callNative('showInFolder', { path }).catch(() => {})
  },
  async openPath(_path: string): Promise<string> {
    return ''
  },
  async trashItem(path: string): Promise<void> {
    // Android has no Recycle Bin: delete the file for real via SAF (or a direct
    // File.delete when All-Files-Access is granted). Rejects when no grant
    // covers the path (removable storage) — the caller logs and moves on.
    // Throwing keeps trashItem's contract (desktop shell.trashItem also throws
    // on failure), so callers already wrap it in try/catch.
    await callNative('deleteFile', { path }, 15000)
  },
  async openExternal(url: string): Promise<void> {
    // Last.fm auth etc. — opened via Custom Tabs from the native side.
    try {
      await callNative('openExternal', { url })
    } catch {
      // Fall back to a renderer-handled open if the plugin is unavailable.
      emitEvent('open-external', url)
    }
  }
}

// --- nativeTheme ---------------------------------------------------------

export const nativeTheme = {
  themeSource: 'system' as 'system' | 'light' | 'dark',
  shouldUseDarkColors: true,
  on(): void {
    /* no-op */
  }
}

export default {
  app,
  ipcMain,
  BrowserWindow,
  net,
  safeStorage,
  protocol,
  globalShortcut,
  dialog,
  shell,
  nativeTheme
}
