/**
 * Orchestrates the Google Drive library sync: single-flight `syncNow`, the
 * connect/disconnect flows, and the triggers (startup, 15-min interval, a
 * 2-min-throttled "library dirty" signal, and manual). All merge/DB logic lives
 * in the pure modules; this file only sequences I/O and reports status.
 */
import { createHash } from 'node:crypto'
import type { SyncStatus } from '@shared/types'
import { getDb } from '../db'
import { getSettings, setSettings } from '../settings'
import { broadcast, onBroadcast } from '../events'
import { logWarn } from '../logger'
import { HttpError } from '../net/errors'
import {
  ensureAccessToken,
  clearAccessTokenCache,
  runLoopbackAuth,
  revokeToken
} from './googleAuth'
import { isOAuthConfigured } from './googleOAuthConfig'
import { findFile, downloadJson, uploadJson, renameFile, DriveCorruptError } from './driveClient'
import { buildSnapshot, applyWriteback } from './snapshot'
import { mergeSync } from './merge'
import { parseSyncFile, type SyncFile } from './schema'
import { onLibraryDirty } from './dirty'
import { isBackground, onAppStateChange } from '../appLifecycle'
import { syncMissingSet, startMissingFetchWorker, stopMissingFetchWorker } from './fetchMissing'

const SYNC_INTERVAL_MS = 15 * 60_000
const DIRTY_THROTTLE_MS = 2 * 60_000
const STARTUP_DELAY_MS = 10_000

let syncing = false
let lastError: string | null = null
let dirtyTimer: ReturnType<typeof setTimeout> | null = null
let intervalTimer: ReturnType<typeof setInterval> | null = null
let startupTimer: ReturnType<typeof setTimeout> | null = null
let unsubBroadcast: (() => void) | null = null
// An automatic sync (interval/dirty) suppressed while backgrounded; runs on the
// next return to foreground instead. Manual driveSyncNow is never gated.
let pendingAutoSync = false

// ---- status ----------------------------------------------------------------

export function getSyncStatus(): SyncStatus {
  const s = getSettings()
  return {
    configured: isOAuthConfigured(),
    connected: !!s.googleRefreshToken,
    email: s.googleDriveEmail,
    enabled: s.driveSyncEnabled,
    syncing,
    lastSyncAt: s.driveSyncLastAt,
    lastError
  }
}

function emitStatus(): void {
  broadcast('sync:status', getSyncStatus())
}

function errMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

// ---- content hashing (order-independent, ignores volatile header fields) ----

function stableStringify(value: unknown): string {
  if (value === null || typeof value !== 'object') return JSON.stringify(value) ?? 'null'
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(',')}]`
  const obj = value as Record<string, unknown>
  const keys = Object.keys(obj).sort()
  return `{${keys.map((k) => `${JSON.stringify(k)}:${stableStringify(obj[k])}`).join(',')}}`
}

/** Hash of the meaningful content only — generatedAt/generatedBy/playback are
 *  volatile and excluded so identical libraries hash equal across devices. */
function contentHash(file: SyncFile): string {
  const payload = { tracks: file.tracks, playlists: file.playlists, tombstones: file.tombstones }
  return createHash('sha256').update(stableStringify(payload)).digest('hex')
}

// ---- the sync itself --------------------------------------------------------

async function doSync(token: string): Promise<void> {
  const before = getSettings()
  const local = buildSnapshot(getDb(), before.syncDeviceId)
  const localHash = contentHash(local)

  const meta = await findFile(token)

  // Fast path: nothing changed on either side since the last successful sync.
  if (
    meta &&
    meta.md5Checksum &&
    meta.md5Checksum === before.driveLastRemoteMd5 &&
    localHash === before.driveLastLocalHash
  ) {
    setSettings({ driveFileId: meta.id, driveSyncLastAt: Date.now() })
    return
  }

  let existingId: string | null = meta?.id ?? null
  let remote: SyncFile | null = null
  if (meta) {
    try {
      remote = parseSyncFile(await downloadJson(token, meta.id))
      if (!remote) throw new DriveCorruptError('unrecognized sync file shape')
    } catch (err) {
      if (err instanceof DriveCorruptError) {
        // Move the bad file aside (never silently overwrite) and recreate fresh.
        const stamp = new Date().toISOString().replace(/[:.]/g, '-')
        await renameFile(token, meta.id, `aether-library.corrupt-${stamp}.json.gz`)
        remote = null
        existingId = null
      } else {
        throw err
      }
    }
  }

  const { merged, writeback, changed } = mergeSync(local, remote)
  if (changed) {
    applyWriteback(getDb(), writeback)
    // reason 'sync' so our own broadcast doesn't re-trigger markDirty
    broadcast('library:changed', { reason: 'sync' })
  }

  let fileId = existingId
  let remoteMd5 = meta && existingId ? meta.md5Checksum ?? null : null
  if (!remote || contentHash(merged) !== contentHash(remote)) {
    const up = await uploadJson(token, existingId, merged)
    fileId = up.id
    remoteMd5 = up.md5Checksum
  }

  setSettings({
    driveFileId: fileId,
    driveLastLocalHash: localHash,
    driveLastRemoteMd5: remoteMd5,
    driveSyncLastAt: Date.now()
  })

  // Record any track present in the merged library but absent on this device so
  // the background worker can re-download its audio from the download sources.
  syncMissingSet(local, merged)
}

async function runSyncOnce(retriedAuth: boolean): Promise<void> {
  const token = await ensureAccessToken()
  try {
    await doSync(token)
  } catch (err) {
    // A 401 mid-flight → refresh the token once and retry the whole pass.
    if (!retriedAuth && err instanceof HttpError && err.status === 401) {
      clearAccessTokenCache()
      await runSyncOnce(true)
      return
    }
    throw err
  }
}

/** Run a sync now (single-flight). No-op when not connected/configured. */
export async function syncNow(): Promise<SyncStatus> {
  if (syncing) return getSyncStatus()
  const s = getSettings()
  if (!isOAuthConfigured() || !s.googleRefreshToken) return getSyncStatus()
  syncing = true
  emitStatus()
  try {
    await runSyncOnce(false)
    lastError = null
  } catch (err) {
    lastError = errMessage(err)
    logWarn('sync', 'sincronizzazione fallita', err)
  } finally {
    syncing = false
    emitStatus()
  }
  return getSyncStatus()
}

/** Auto-sync trigger: throttled so a burst of edits coalesces into one sync. */
function markDirty(): void {
  const s = getSettings()
  if (!s.driveSyncEnabled || !s.googleRefreshToken) return
  if (dirtyTimer) return
  dirtyTimer = setTimeout(() => {
    dirtyTimer = null
    // Defer automatic syncs while backgrounded (no snapshot+hash of the whole
    // library with the screen off); caught up on the return to foreground.
    if (isBackground()) {
      pendingAutoSync = true
      return
    }
    void syncNow()
  }, DIRTY_THROTTLE_MS)
  dirtyTimer.unref?.()
}

// ---- connect / disconnect ---------------------------------------------------

export async function connectDrive(): Promise<SyncStatus> {
  const { refreshToken, email } = await runLoopbackAuth()
  clearAccessTokenCache()
  setSettings({ googleRefreshToken: refreshToken, googleDriveEmail: email, driveSyncEnabled: true })
  lastError = null
  // Credentials may have been entered this session, after startSyncService()
  // already bailed for lack of a client — (re)wire the triggers now. Idempotent.
  startSyncService()
  emitStatus()
  void syncNow() // first sync right after connecting
  return getSyncStatus()
}

export async function disconnectDrive(): Promise<SyncStatus> {
  const token = getSettings().googleRefreshToken
  if (token) await revokeToken(token)
  clearAccessTokenCache()
  if (dirtyTimer) {
    clearTimeout(dirtyTimer)
    dirtyTimer = null
  }
  setSettings({
    googleRefreshToken: '',
    googleDriveEmail: '',
    driveSyncEnabled: false,
    driveFileId: null,
    driveLastLocalHash: null,
    driveLastRemoteMd5: null
  })
  lastError = null
  emitStatus()
  return getSyncStatus()
}

// ---- lifecycle --------------------------------------------------------------

export function startSyncService(): void {
  if (!isOAuthConfigured()) return // no bundled client → sync is inert
  if (!unsubBroadcast) {
    unsubBroadcast = onBroadcast((event, payload) => {
      if (event !== 'library:changed') return
      const reason = (payload as { reason?: string } | null | undefined)?.reason
      if (reason !== 'sync') markDirty()
    })
  }
  onLibraryDirty(markDirty)

  if (!intervalTimer) {
    intervalTimer = setInterval(() => {
      const s = getSettings()
      if (!s.driveSyncEnabled || !s.googleRefreshToken) return
      if (isBackground()) {
        pendingAutoSync = true
        return
      }
      void syncNow()
    }, SYNC_INTERVAL_MS)
    intervalTimer.unref?.()
  }

  startupTimer = setTimeout(() => {
    startupTimer = null
    const s = getSettings()
    if (s.driveSyncEnabled && s.googleRefreshToken) void syncNow()
  }, STARTUP_DELAY_MS)
  startupTimer.unref?.()

  // Background re-download of tracks missing locally (populated by each sync).
  startMissingFetchWorker()
}

onAppStateChange((state) => {
  if (state !== 'foreground' || !intervalTimer) return
  const s = getSettings()
  if (!s.driveSyncEnabled || !s.googleRefreshToken) return
  // Catch up on syncs suppressed while backgrounded, or simply overdue because
  // the process was frozen (Doze) past the normal 15-min cadence.
  const overdue = Date.now() - (s.driveSyncLastAt ?? 0) >= SYNC_INTERVAL_MS
  if (pendingAutoSync || overdue) {
    pendingAutoSync = false
    void syncNow()
  }
})

export function stopSyncService(): void {
  stopMissingFetchWorker()
  unsubBroadcast?.()
  unsubBroadcast = null
  onLibraryDirty(() => {})
  if (intervalTimer) {
    clearInterval(intervalTimer)
    intervalTimer = null
  }
  if (startupTimer) {
    clearTimeout(startupTimer)
    startupTimer = null
  }
  if (dirtyTimer) {
    clearTimeout(dirtyTimer)
    dirtyTimer = null
  }
}
