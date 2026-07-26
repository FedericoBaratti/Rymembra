import { registerPlugin } from '@capacitor/core'
import type { AetherAPI, AetherEventName } from '@shared/types'
import { INVOKE_METHODS } from '@shared/ipcMethods'
import i18n from '@/i18n'
import { setMediaBase } from '@/lib/format'
import { nativeDispatch } from '@/lib/nativeRpc'
import { createBridgeWatchdog } from '@/lib/bridgeWatchdog'
import { toast } from '@/store/useToastStore'

/**
 * Renderer-side replacement for the Electron preload bridge (electron/preload.ts).
 *
 * On desktop, `window.aether.<method>(...)` was `ipcRenderer.invoke(method, ...)`.
 * On Android we keep the exact same `window.aether` surface, but each call is
 * serialized to JSON and sent to the nodejs-mobile backend; the matching reply
 * (correlated by `id`) resolves/rejects the Promise. Backend → renderer events
 * (`aether:event`) are delivered as push messages and dispatched to `on(...)`.
 *
 * The native transport is pluggable, so either nodejs-mobile integration works:
 *   1. nodejs-mobile-cordova  → `window.nodejs.channel` (recommended for Capacitor)
 *   2. a custom `NodeBackend` Capacitor plugin (raw nodejs-mobile AAR)
 * The node side speaks the same JSON protocol regardless (see node-backend/main.ts).
 *
 * Binary payloads (audio, cover art) do NOT go through this bridge — they are
 * served by the local HTTP server (see src/lib/format.ts mediaUrl/coverUrl).
 */

interface Transport {
  /** Start the node runtime if not already running. */
  start(): Promise<void>
  /** Post a JSON string to the node process. */
  send(message: string): Promise<void>
  /** Register the single handler for JSON strings pushed from node. */
  onMessage(cb: (message: string) => void): Promise<void>
}

// --- Transport 1: nodejs-mobile-cordova (window.nodejs.channel) ----------

interface CordovaNodeChannel {
  on(event: 'message', cb: (msg: string) => void): void
  send(msg: string): void
}
interface CordovaNodeJs {
  start(script: string, cb?: (err?: unknown) => void): void
  startWithScript?(script: string, cb?: (err?: unknown) => void): void
  channel: CordovaNodeChannel
}

function cordovaTransport(): Transport | null {
  const nodejs = (window as unknown as { nodejs?: CordovaNodeJs }).nodejs
  if (!nodejs?.channel) return null
  return {
    start: () =>
      new Promise<void>((resolve, reject) => {
        try {
          nodejs.start('main.js', (err) => {
            if (!err) return resolve()
            // nodejs-mobile keeps a PROCESS-WIDE "engine started" flag and cannot
            // restart the engine in-process (a second startNodeWithArguments aborts
            // with `Check failed: !platform_`). After the WebView page is reloaded or
            // the Activity is recreated while the process survives (e.g.
            // RecoveringWebViewClient.onRenderProcessGone → recreate(), or Android
            // reloading the page when the background renderer was evicted), the fresh
            // page re-runs start() but the engine is already live. The native side
            // replies "Engine already started" — that is a successful RECONNECT, not a
            // failure: the node engine is alive and the message channel is re-registered
            // on page load (nodejs_apis.js setAllChannelsListener). Treating it as fatal
            // rejects every window.aether.* call → the whole app is dead on reopen.
            const msg =
              typeof err === 'string'
                ? err
                : String((err as { message?: string })?.message ?? err)
            if (/already started/i.test(msg)) return resolve()
            reject(err instanceof Error ? err : new Error(msg))
          })
        } catch (err) {
          reject(err as Error)
        }
      }),
    send: async (message) => nodejs.channel.send(message),
    onMessage: async (cb) => nodejs.channel.on('message', cb)
  }
}

// --- Transport 2: custom NodeBackend Capacitor plugin (raw AAR) ----------

interface NodeBackendPlugin {
  start(): Promise<void>
  send(options: { message: string }): Promise<void>
  addListener(
    eventName: 'message',
    listener: (data: { message: string }) => void
  ): Promise<{ remove: () => Promise<void> }>
}

function pluginTransport(): Transport {
  const NodeBackend = registerPlugin<NodeBackendPlugin>('NodeBackend')
  return {
    start: () => NodeBackend.start(),
    send: async (message) => void (await NodeBackend.send({ message })),
    onMessage: async (cb) => void (await NodeBackend.addListener('message', (d) => cb(d.message)))
  }
}

function getTransport(): Transport {
  return cordovaTransport() ?? pluginTransport()
}

const transport = getTransport()

type ReqMessage = { t: 'req'; id: number; channel: string; args: unknown[] }
type ResMessage = { t: 'res'; id: number; ok: true; result: unknown }
type ErrMessage = { t: 'res'; id: number; ok: false; error: string }
type EventMessage = { t: 'event'; name: AetherEventName; payload: unknown }
type NativeRpcMessage = { t: 'nrpc'; nid: number; method: string; args: unknown }
type IncomingMessage = ResMessage | ErrMessage | EventMessage | NativeRpcMessage

// INVOKE_METHODS now lives in @shared/ipcMethods (single source of truth shared
// with electron/preload.ts) — no longer duplicated by hand.

let seq = 0
const pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>()
const listeners = new Map<AetherEventName, Set<(payload: unknown) => void>>()

// Liveness watchdog: if requests are pending and the backend has been silent
// past the stall window, a cheap probe is fired; if even that goes unanswered
// the backend is dead → reject EVERYTHING pending instead of hanging spinners
// forever. Slow-but-alive calls (yt-dlp preview, rescans) are never killed:
// any inbound message settles the probe.

// ---- backend health (persistent-death detection) --------------------------
// One missed probe is only a *transient* verdict (GC pause, heavy migration).
// After each death the bridge keeps re-probing on its own; if the backend has
// stayed dead past BACKEND_DOWN_AFTER_MS the app is de-facto bricked (the node
// engine cannot be restarted in-process) — surface that as a persistent
// "backend down" state so the UI can offer the only real fix: restart the app
// process (BackendDownBanner → FileAccessNative.restartApp()).
const BACKEND_DOWN_AFTER_MS = 60_000
const REPROBE_DELAY_MS = 10_000
let deadSince: number | null = null
let backendDown = false
let reprobeTimer: ReturnType<typeof setTimeout> | null = null
const healthListeners = new Set<(down: boolean) => void>()

/** True when the backend has been unreachable long enough to be considered gone. */
export function isBackendDown(): boolean {
  return backendDown
}

/** Subscribe to persistent backend-down transitions (both directions). */
export function onBackendHealthChange(cb: (down: boolean) => void): () => void {
  healthListeners.add(cb)
  return () => healthListeners.delete(cb)
}

function setBackendDown(down: boolean): void {
  if (backendDown === down) return
  backendDown = down
  for (const cb of healthListeners) cb(down)
}

let deadToastAt = 0
function rejectAllPending(): void {
  const entries = [...pending.values()]
  pending.clear()
  watchdog.noteIdle()
  const err = new Error('BACKEND_UNREACHABLE')
  for (const e of entries) e.reject(err)
  const now = Date.now()
  // Suppress the repeating toast once the persistent banner has taken over.
  if (entries.length > 0 && !backendDown && now - deadToastAt > 30_000) {
    deadToastAt = now
    toast.error(i18n.t('errors.backend_unreachable'))
  }
  if (deadSince == null) deadSince = now
  if (now - deadSince >= BACKEND_DOWN_AFTER_MS) setBackendDown(true)
  // Keep the death re-checked: this probe re-arms the watchdog, so a still-dead
  // backend lands here again in ~20s, while ANY inbound message (a late reply,
  // an event) resets the verdict in handleIncoming.
  if (reprobeTimer == null) {
    reprobeTimer = setTimeout(() => {
      reprobeTimer = null
      void invoke('__mediaBase', []).catch(() => {})
    }, REPROBE_DELAY_MS)
  }
}

const watchdog = createBridgeWatchdog({
  // Existing cheap handler; the reply (or ANY other message) proves liveness.
  probe: () => void invoke('__mediaBase', []).catch(() => {}),
  onDead: rejectAllPending
})

function handleIncoming(raw: string): void {
  let msg: IncomingMessage
  try {
    msg = JSON.parse(raw) as IncomingMessage
  } catch {
    return
  }
  watchdog.noteMessage()
  // Any inbound message proves the backend is alive: clear the death verdict.
  deadSince = null
  if (reprobeTimer != null) {
    clearTimeout(reprobeTimer)
    reprobeTimer = null
  }
  setBackendDown(false)
  if (msg.t === 'event') {
    const set = listeners.get(msg.name)
    if (set) for (const cb of set) cb(msg.payload)
    return
  }
  if (msg.t === 'nrpc') {
    // Backend → native reverse-RPC: invoke the Capacitor plugin and reply.
    const { nid, method, args } = msg
    nativeDispatch(method, args)
      .then((result) => transport.send(JSON.stringify({ t: 'nres', nid, ok: true, result })))
      .catch((err) =>
        transport.send(
          JSON.stringify({ t: 'nres', nid, ok: false, error: err instanceof Error ? err.message : String(err) })
        )
      )
    return
  }
  const entry = pending.get(msg.id)
  if (!entry) return
  pending.delete(msg.id)
  if (pending.size === 0) watchdog.noteIdle()
  if (msg.ok) entry.resolve(msg.result)
  else entry.reject(new Error(msg.error))
}

let started: Promise<void> | null = null
function ensureStarted(): Promise<void> {
  if (!started) {
    // `started` gates ONLY the transport being up (listener attached + engine
    // started). It must NOT await any invoke(): invoke() waits on `started`
    // before sending, so awaiting a request here would deadlock — `started`
    // would never resolve, and no req (getSettings, refreshAll, …) would ever
    // be sent. Media-base discovery therefore runs in ensureReady(), AFTER this.
    started = (async () => {
      await transport.onMessage(handleIncoming)
      await transport.start()
    })()
  }
  return started
}

let ready: Promise<void> | null = null
/**
 * Resolves once the transport is up AND the media-server origin has been
 * discovered, so mediaUrl()/coverUrl() resolve to http://127.0.0.1:<port>/…
 * instead of the desktop `aether://` default. The renderer must wait on this
 * before the first paint: a track played (or a cover shown) with a stale
 * `aether://` URL silently fails in the WebView (no such scheme on mobile) —
 * the same "infinite spinner / no cover" symptom as a blocked request.
 *
 * The `__mediaBase` invoke() only waits on `started` (transport), NOT on
 * `ready`, so awaiting it here cannot reintroduce the deadlock fixed above.
 */
function ensureReady(): Promise<void> {
  if (!ready) {
    ready = (async () => {
      await ensureStarted()
      try {
        const base = (await invoke('__mediaBase', [])) as string
        if (base) setMediaBase(base)
      } catch {
        /* server not ready yet; events/retries will refresh URLs */
      }
    })()
  }
  return ready
}

/** Resolves once the backend is up and the media base is configured. */
export function bridgeReady(): Promise<void> {
  return ensureReady()
}

function invoke(channel: string, args: unknown[]): Promise<unknown> {
  const id = ++seq
  const req: ReqMessage = { t: 'req', id, channel, args }
  return new Promise<unknown>((resolve, reject) => {
    pending.set(id, { resolve, reject })
    ensureStarted()
      .then(() => transport.send(JSON.stringify(req)))
      .then(() => {
        // Arm the watchdog only once the request is actually on the wire —
        // engine startup time must not count as backend silence.
        if (pending.has(id)) watchdog.noteSent()
      })
      .catch((err) => {
        pending.delete(id)
        if (pending.size === 0) watchdog.noteIdle()
        reject(err instanceof Error ? err : new Error(String(err)))
      })
  })
}

/** Build the `window.aether` API object backed by the nodejs-mobile bridge. */
export function createAetherBridge(): AetherAPI {
  const api: Record<string, unknown> = {}
  for (const method of INVOKE_METHODS) {
    api[method] = (...args: unknown[]) => invoke(method, args)
  }
  api['on'] = (event: AetherEventName, cb: (payload: unknown) => void) => {
    let set = listeners.get(event)
    if (!set) {
      set = new Set()
      listeners.set(event, set)
    }
    set.add(cb)
    return () => set!.delete(cb)
  }
  return api as unknown as AetherAPI
}

/** Install the bridge on window.aether. Idempotent. */
export function installAetherBridge(): void {
  if ((window as unknown as { aether?: AetherAPI }).aether) return
  ;(window as unknown as { aether: AetherAPI }).aether = createAetherBridge()
  // Kick off the node runtime AND the media-base discovery eagerly so the first
  // IPC call is fast and mediaUrl()/coverUrl() resolve before the first paint.
  void ensureReady()
}
