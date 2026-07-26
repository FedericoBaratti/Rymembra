import { registerPlugin } from '@capacitor/core'
import type { AetherAPI, AetherEventName, AppSettings, TrackQuery } from '@shared/types'
import { INVOKE_METHODS, type InvokeMethod } from '@shared/ipcMethods'
import { setMediaBase, setMediaToken } from '@/lib/format'
import { SecureStoreNative } from '@/lib/nativeRpc'

/**
 * Thin-client transport: window.aether backed directly by the desktop's LAN
 * server (electron/modules/lan/ in the Aeter repo) instead of the on-device
 * nodejs-mobile backend (src/lib/bridge.ts). Built alongside bridge.ts, not
 * replacing it — main.mobile.tsx picks this transport only once a desktop has
 * actually been paired AND answers a boot probe (see installLanBridge());
 * otherwise — nothing paired, or the PC is off/unreachable — the legacy local
 * bridge still boots exactly as before, so the phone remains a standalone
 * player.
 *
 * Only a fixed subset of INVOKE_METHODS is backed by real REST calls (the
 * play/browse/search surface the LAN server exposes) — every other method
 * (download*, enrichTrack, setSettings, spotifyMigration*, ...) rejects. That
 * rejection list IS the "phone can only play/browse/search" enforcement: the
 * trimmed mobile UI (BottomNav) never calls them in LAN mode.
 */

const PREF_HOST = 'lan.host'
const PREF_PORT = 'lan.port'
const PREF_TOKEN = 'lan.deviceToken'
const PREF_DEVICE_ID = 'lan.deviceId'

export interface PairingInfo {
  host: string
  port: number
  deviceToken: string
  deviceId: string
}

/** The raw payload encoded in the desktop's pairing QR code. */
export interface PairingQrPayload {
  v: 1
  host: string
  port: number
  pairingToken: string
}

async function readPref(key: string): Promise<string | null> {
  const { value } = await SecureStoreNative.getValue({ key })
  return value
}

export async function getStoredPairing(): Promise<PairingInfo | null> {
  const [host, port, deviceToken, deviceId] = await Promise.all([
    readPref(PREF_HOST),
    readPref(PREF_PORT),
    readPref(PREF_TOKEN),
    readPref(PREF_DEVICE_ID)
  ])
  if (!host || !port || !deviceToken) return null
  return { host, port: Number(port), deviceToken, deviceId: deviceId ?? '' }
}

export async function isPaired(): Promise<boolean> {
  return (await getStoredPairing()) !== null
}

async function savePairing(info: PairingInfo): Promise<void> {
  await Promise.all([
    SecureStoreNative.setValue({ key: PREF_HOST, value: info.host }),
    SecureStoreNative.setValue({ key: PREF_PORT, value: String(info.port) }),
    SecureStoreNative.setValue({ key: PREF_TOKEN, value: info.deviceToken }),
    SecureStoreNative.setValue({ key: PREF_DEVICE_ID, value: info.deviceId })
  ])
}

export async function clearPairing(): Promise<void> {
  await Promise.all([
    SecureStoreNative.deleteValue({ key: PREF_HOST }),
    SecureStoreNative.deleteValue({ key: PREF_PORT }),
    SecureStoreNative.deleteValue({ key: PREF_TOKEN }),
    SecureStoreNative.deleteValue({ key: PREF_DEVICE_ID })
  ])
}

/** Claims a pairing ticket scanned from the desktop's QR code. */
export async function claimPairing(payload: PairingQrPayload, deviceName: string): Promise<void> {
  const res = await fetch(`http://${payload.host}:${payload.port}/api/pair`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pairingToken: payload.pairingToken, deviceName })
  })
  if (!res.ok) throw new Error(`PAIRING_FAILED_${res.status}`)
  const { deviceId, deviceToken } = (await res.json()) as { deviceId: string; deviceToken: string }
  await savePairing({ host: payload.host, port: payload.port, deviceToken, deviceId })
}

// ---- REST calls against the paired desktop ----

function baseUrl(pairing: PairingInfo): string {
  return `http://${pairing.host}:${pairing.port}`
}

async function request(pairing: PairingInfo, path: string, init?: RequestInit): Promise<unknown> {
  const res = await fetch(`${baseUrl(pairing)}${path}`, {
    ...init,
    headers: { Authorization: `Bearer ${pairing.deviceToken}`, ...(init?.headers ?? {}) }
  })
  if (!res.ok) throw new Error(`LAN_HTTP_${res.status}`)
  if (res.status === 204) return undefined
  const text = await res.text()
  return text ? (JSON.parse(text) as unknown) : undefined
}

const getJson = (p: PairingInfo, path: string): Promise<unknown> => request(p, path)
const postJson = (p: PairingInfo, path: string, body: unknown): Promise<unknown> =>
  request(p, path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })
const putJson = (p: PairingInfo, path: string, body: unknown): Promise<unknown> =>
  request(p, path, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })

function queryString(params: Record<string, string | number | undefined>): string {
  const sp = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined) sp.set(k, String(v))
  const s = sp.toString()
  return s ? `?${s}` : ''
}

type Handler = (pairing: PairingInfo, args: unknown[]) => Promise<unknown>

const DEFAULT_SETTINGS: AppSettings = {
  watchFolders: [],
  downloadFolder: '',
  downloadQuality: 'mp3-320',
  downloadConcurrency: 1,
  autoFixYoutubeMetadata: false,
  crossfadeSeconds: 0,
  audioOffloadEnabled: false,
  replayGainEnabled: false,
  replayGainTargetDb: -18,
  eqEnabled: false,
  eqGains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  eqCustomPresets: [],
  theme: 'dark',
  skin: 'plain',
  language: 'it',
  volume: 0.8,
  muted: false,
  notificationsOnTrackChange: false,
  globalMediaKeys: true,
  hasSeenOnboarding: true,
  spotifyClientId: '',
  spotifyClientSecret: '',
  lastfmApiKey: '',
  lastfmApiSecret: '',
  lastfmSessionKey: '',
  lastfmUsername: '',
  scrobblingEnabled: false,
  acoustidApiKey: '',
  autoEnrichEnabled: false,
  enrichFingerprint: false,
  dedupeAutoRemove: false,
  dedupeKeep: 'higher',
  googleClientId: '',
  googleClientSecret: '',
  driveSyncEnabled: false,
  driveSyncLastAt: null,
  googleDriveEmail: '',
  syncDeviceId: '',
  driveFileId: null,
  driveLastLocalHash: null,
  driveLastRemoteMd5: null,
  googleRefreshToken: '',
  autoFetchMissing: false,
  autoFetchNetwork: 'wifi',
  transferServerEnabled: false
}

let localSettings: AppSettings = { ...DEFAULT_SETTINGS }

const HANDLERS: Partial<Record<InvokeMethod, Handler>> = {
  getTracks: (p, [query]) => {
    const q = (query ?? {}) as TrackQuery
    return getJson(
      p,
      `/api/tracks${queryString({
        sortBy: q.sortBy,
        sortDir: q.sortDir,
        albumId: q.albumId,
        artistName: q.artistName,
        limit: q.limit,
        offset: q.offset
      })}`
    )
  },
  getTrackCount: (p) => getJson(p, '/api/tracks/count'),
  getTrackById: (p, [id]) => getJson(p, `/api/tracks/${id}`),
  getTracksByIds: (p, [ids]) => postJson(p, '/api/tracks/byIds', { ids }),
  getAlbums: (p) => getJson(p, '/api/albums'),
  getAlbumTracks: (p, [albumId]) => getJson(p, `/api/albums/${albumId}/tracks`),
  getArtists: (p) => getJson(p, '/api/artists'),
  getArtistAlbums: (p, [name]) => getJson(p, `/api/artists/${encodeURIComponent(String(name))}/albums`),
  getLibraryStats: (p) => getJson(p, '/api/stats'),
  search: (p, [term]) => getJson(p, `/api/search?q=${encodeURIComponent(String(term))}`),
  getPlaylists: (p) => getJson(p, '/api/playlists'),
  getPlaylistTracks: (p, [id]) => getJson(p, `/api/playlists/${id}/tracks`),
  getLikedTracks: (p) => getJson(p, '/api/tracks/liked'),
  // Served by the desktop, which also fetches+caches missing lyrics remotely —
  // the phone gets the same lyrics the desktop shows, with zero local logic.
  getLyrics: (p, [id]) => getJson(p, `/api/tracks/${id}/lyrics`),
  recordPlay: (p, [id, msPlayed]) => postJson(p, `/api/tracks/${id}/play`, { msPlayed }),
  setRating: (p, [id, rating]) => putJson(p, `/api/tracks/${id}/rating`, { rating }),
  setLiked: (p, [id, liked]) => putJson(p, `/api/tracks/${id}/liked`, { liked }),
  // getSettings/setSettings are NOT proxied to the desktop: its getSettings()
  // overlays decrypted secrets (API keys, OAuth tokens) for its own renderer,
  // and none of that belongs on a paired phone. Instead these serve a local,
  // in-memory-only AppSettings — session-scoped UI prefs (theme, volume, EQ),
  // reset on relaunch — purely so useSettingsStore resolves instead of hanging
  // forever (its 5-retry load() would otherwise leave `settings` null and a
  // large part of the UI stuck on skeletons).
  getSettings: () => Promise.resolve({ ...localSettings }),
  setSettings: (_p, [patch]) => {
    localSettings = { ...localSettings, ...(patch as Partial<AppSettings>) }
    return Promise.resolve({ ...localSettings })
  },
  // No LAN route persists waveform peaks (electron/modules/lan/ never exposed
  // one — a nice-to-have cache, not core). getWaveform must resolve (not
  // reject) with null so useWaveform.ts's local Web Audio decode-and-cache
  // fallback still runs; saveWaveform is a silent no-op (the in-memory cache
  // in useWaveform.ts already covers the session).
  getWaveform: () => Promise.resolve(null),
  saveWaveform: () => Promise.resolve(undefined),
  // Called unconditionally from useAppBootstrap.ts on every visibility change
  // (foreground/background) and app quit — harmless no-ops here (nothing
  // local to flush; no backend to notify) so they don't spam unhandled-
  // rejection noise on every app switch. getDownloads() backs a queue refresh
  // in the same hot path; LAN mode has no downloader, so an empty list is the
  // correct answer, not a downgrade.
  flushNow: () => Promise.resolve(undefined),
  setAppState: () => Promise.resolve(undefined),
  getDownloads: () => Promise.resolve([])
}

// ---- live events over the LAN server's /ws ----

/** Health of the link to the paired desktop, driven by the /ws socket (same
    server as the REST/media routes, so it's an honest proxy for all three).
    Read by LanOfflineBanner to surface "PC not reachable" instead of letting
    every page fail silently with skeletons. */
export type LanConnectionState = 'connecting' | 'connected' | 'offline'

const listeners = new Map<AetherEventName, Set<(payload: unknown) => void>>()
let socket: WebSocket | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let reconnectAttempts = 0
let connectionState: LanConnectionState = 'connecting'
const connectionListeners = new Set<(state: LanConnectionState) => void>()

const RECONNECT_BASE_MS = 3000
const RECONNECT_MAX_MS = 30_000
/** From this many consecutive failures on, assume the stored host may be stale
    (DHCP renewal, desktop moved network) and re-verify/re-discover before the
    next dial instead of retrying the same IP forever. */
const REDISCOVER_AFTER_ATTEMPTS = 3

export function getLanConnectionState(): LanConnectionState {
  return connectionState
}

export function onLanConnectionChange(cb: (state: LanConnectionState) => void): () => void {
  connectionListeners.add(cb)
  return () => connectionListeners.delete(cb)
}

function setConnectionState(state: LanConnectionState): void {
  if (connectionState === state) return
  connectionState = state
  for (const cb of connectionListeners) cb(state)
}

/** Re-runs host reconciliation mid-session, mutating `pairing` in place so the
    closures created by createLanBridge/connectWs see the new host too. */
async function rediscoverHost(pairing: PairingInfo): Promise<void> {
  const { pairing: updated } = await reconcileHost(pairing)
  if (updated.host === pairing.host && updated.port === pairing.port) return
  pairing.host = updated.host
  pairing.port = updated.port
  setMediaBase(`${baseUrl(pairing)}/`)
}

function connectWs(pairing: PairingInfo): void {
  if (socket) return
  setConnectionState('connecting')
  const url = `ws://${pairing.host}:${pairing.port}/ws?token=${encodeURIComponent(pairing.deviceToken)}`
  const ws = new WebSocket(url)
  ws.onopen = () => {
    reconnectAttempts = 0
    setConnectionState('connected')
  }
  ws.onmessage = (ev) => {
    try {
      const { event, payload } = JSON.parse(String(ev.data)) as { event: AetherEventName; payload: unknown }
      const set = listeners.get(event)
      if (set) for (const cb of set) cb(payload)
    } catch {
      /* ignore malformed frame */
    }
  }
  const scheduleReconnect = (): void => {
    if (socket === ws) socket = null
    setConnectionState('offline')
    if (reconnectTimer) return
    reconnectAttempts++
    // Capped exponential backoff: 3s, 6s, 12s, 24s, then 30s flat.
    const delay = Math.min(RECONNECT_BASE_MS * 2 ** (reconnectAttempts - 1), RECONNECT_MAX_MS)
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null
      void (async () => {
        if (reconnectAttempts >= REDISCOVER_AFTER_ATTEMPTS) await rediscoverHost(pairing)
        connectWs(pairing)
      })()
    }, delay)
  }
  ws.onclose = scheduleReconnect
  ws.onerror = scheduleReconnect
  socket = ws
}

/** Immediate reconnect for the offline banner's retry button: skips whatever
    backoff is pending and re-discovers the host before dialing. */
export async function retryLanConnection(): Promise<void> {
  if (!activePairing || socket) return
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
  setConnectionState('connecting')
  await rediscoverHost(activePairing)
  connectWs(activePairing)
}

export function createLanBridge(pairing: PairingInfo): AetherAPI {
  const api: Record<string, unknown> = {}
  for (const method of INVOKE_METHODS) {
    const handler = HANDLERS[method]
    api[method] = handler
      ? (...args: unknown[]) => handler(pairing, args)
      : () => Promise.reject(new Error(`LAN_METHOD_NOT_SUPPORTED: ${method}`))
  }
  api['on'] = (event: AetherEventName, cb: (payload: unknown) => void) => {
    let set = listeners.get(event)
    if (!set) listeners.set(event, (set = new Set()))
    set.add(cb)
    return () => set!.delete(cb)
  }
  return api as unknown as AetherAPI
}

// ---- mDNS fallback: the desktop's IP can change between sessions (DHCP lease
// renewal, hotspot reconnect, ...). Pairing already gives an initial host/port
// directly from the QR; this runs at boot and again from the WS reconnect loop
// (rediscoverHost) once enough consecutive dials have failed. ----

interface LanDiscoveryPlugin {
  startDiscovery(): Promise<void>
  stopDiscovery(): Promise<void>
  addListener(
    eventName: 'serviceFound',
    listener: (data: { name: string; host: string; port: number }) => void
  ): Promise<{ remove: () => void }>
}

const LanDiscovery = registerPlugin<LanDiscoveryPlugin>('LanDiscovery')

async function isReachable(pairing: PairingInfo): Promise<boolean> {
  try {
    const res = await fetch(`${baseUrl(pairing)}/health`, { signal: AbortSignal.timeout(2000) })
    return res.ok
  } catch {
    return false
  }
}

/** True only if `host:port` is an Aether server that accepts OUR device token —
    /health alone would happily adopt someone else's desktop when two Aether
    instances advertise on the same LAN. */
async function verifyDiscoveredHost(pairing: PairingInfo, host: string, port: number): Promise<boolean> {
  try {
    const res = await fetch(`http://${host}:${port}/api/tracks/count`, {
      headers: { Authorization: `Bearer ${pairing.deviceToken}` },
      signal: AbortSignal.timeout(2000)
    })
    return res.ok
  } catch {
    return false
  }
}

interface ReconcileResult {
  pairing: PairingInfo
  /** True when the desktop actually answered: /health OK on the stored
      address, or an mDNS hit that accepted our device token. */
  reachable: boolean
}

/** Returns `pairing` unchanged if still reachable; otherwise browses mDNS for
    up to `timeoutMs` and, on a match that accepts our device token, persists +
    returns the updated host/port (same deviceToken/deviceId). Never throws;
    `reachable: false` means both the stored host and the browse came up empty. */
async function reconcileHost(pairing: PairingInfo, timeoutMs = 4000): Promise<ReconcileResult> {
  if (await isReachable(pairing)) return { pairing, reachable: true }
  return new Promise<ReconcileResult>((resolve) => {
    let settled = false
    let removeListener: (() => void) | null = null
    const finish = (result: ReconcileResult): void => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      removeListener?.()
      void LanDiscovery.stopDiscovery().catch(() => {})
      resolve(result)
    }
    const timer = setTimeout(() => finish({ pairing, reachable: false }), timeoutMs)
    void LanDiscovery.addListener('serviceFound', (data) => {
      void (async () => {
        if (settled) return
        // Unverifiable services are skipped, keeping the browse alive for the
        // right desktop until the timeout.
        if (!(await verifyDiscoveredHost(pairing, data.host, data.port))) return
        const updated: PairingInfo = { ...pairing, host: data.host, port: data.port }
        await savePairing(updated).catch(() => {})
        finish({ pairing: updated, reachable: true })
      })()
    }).then((handle) => (removeListener = handle.remove))
    void LanDiscovery.startDiscovery().catch(() => finish({ pairing, reachable: false }))
  })
}

let lanModeActive = false

/** The live pairing installLanBridge() is running on. Mutated in place by
    rediscoverHost() when the desktop's IP changes mid-session, so every
    closure holding it (bridge handlers, the WS reconnect loop) follows. */
let activePairing: PairingInfo | null = null

/** True once installLanBridge() has actually taken over window.aether — read
    by BottomNav.tsx/Settings.tsx to hide nav items and sections that need
    desktop-only features (downloading, metadata editing, settings admin)
    unsupported by the LAN allow-list. */
export function isLanModeActive(): boolean {
  return lanModeActive
}

/** True when a desktop IS paired but didn't answer the boot probe, so this
    session runs on the local nodejs-mobile backend instead (pairing kept).
    Read by LanOfflineBanner to explain the situation and offer a reconnect. */
let lanFallbackLocal = false

export function isLanFallbackLocal(): boolean {
  return lanFallbackLocal
}

/** Fallback-mode reconnect: probes the stored pairing (with mDNS rediscovery)
    and reloads into LAN mode when the desktop answers. Resolves false when it
    is still unreachable — the caller surfaces that instead of reloading into
    the same dead end. */
export async function retryLanFromFallback(): Promise<boolean> {
  const stored = await getStoredPairing()
  if (!stored) return false
  const { reachable } = await reconcileHost(stored)
  if (!reachable) return false
  window.location.reload()
  return true
}

/** Installs window.aether backed by the LAN server. Returns false (no-op) if
    no desktop has been paired yet OR the paired desktop doesn't answer the
    boot probe — the caller falls back to the legacy nodejs-mobile bridge in
    both cases, so the phone still works as a standalone local player when the
    PC is off. The pairing itself is kept for a later reconnect. */
export async function installLanBridge(): Promise<boolean> {
  const stored = await getStoredPairing()
  if (!stored) return false
  // Short browse at boot (vs the reconnect loop's 4s): with the PC off this
  // path gates first render, and health probe (2s) + browse already cost
  // seconds the user spends staring at a blank screen.
  const { pairing, reachable } = await reconcileHost(stored, 2000)
  if (!reachable) {
    lanFallbackLocal = true
    console.info('[lan] paired desktop unreachable at boot — using local backend')
    return false
  }
  activePairing = pairing
  ;(window as unknown as { aether: AetherAPI }).aether = createLanBridge(pairing)
  setMediaBase(`${baseUrl(pairing)}/`)
  setMediaToken(pairing.deviceToken)
  connectWs(pairing)
  lanModeActive = true
  return true
}
