import { writeFileSync, mkdirSync, renameSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'
import { getDb } from '../db'
import { getSettings } from '../settings'
import { buildAutoCatalog } from './autoCatalogBuild'

export type { AutoCatalog, AutoNode, AutoTrackEntry } from './autoCatalogBuild'
export { buildAutoCatalog } from './autoCatalogBuild'

/**
 * Android Auto browse-catalog snapshot (runtime writer/scheduler).
 *
 * Android Auto starts the app's MediaBrowserService WITHOUT the WebView/Node
 * backend necessarily running (the car can connect to a cold app), so the native
 * AetherMediaBrowserService cannot query the SQLite DB (which lives inside
 * nodejs-mobile) at browse time. While the backend IS running we write a compact
 * JSON snapshot of the browse tree to an app-private file the native service
 * reads directly — no Node, no WebView, no media-server loopback. Playback is
 * node-free too: every playable leaf carries the track's real file path.
 *
 * Gated by enableAutoCatalog() so it is a no-op on desktop (which has no Auto and
 * never registers the trigger). The heavy/pure work lives in autoCatalogBuild.ts.
 */

let enabled = false
let refreshTimer: ReturnType<typeof setTimeout> | null = null

/** Enable snapshot generation. Called from node-backend boot; desktop never
 *  calls it, so all writes below are no-ops there. */
export function enableAutoCatalog(): void {
  enabled = true
}

function autoDir(): string {
  const dir = join(app.getPath('userData'), 'auto')
  mkdirSync(dir, { recursive: true })
  return dir
}

export function autoCatalogPath(): string {
  return join(autoDir(), 'catalog.json')
}

/** Build + atomically write the snapshot now. No-op unless enabled. */
export function writeAutoCatalog(): void {
  if (!enabled) return
  const settings = getSettings()
  const catalog = buildAutoCatalog(getDb(), {
    rgEnabled: settings.replayGainEnabled,
    rgTargetDb: settings.replayGainTargetDb
  })
  const path = autoCatalogPath()
  const tmp = `${path}.tmp`
  writeFileSync(tmp, JSON.stringify(catalog), 'utf-8')
  renameSync(tmp, path)
}

/** Debounced refresh — coalesces bursts of library:changed / like toggles. */
export function scheduleAutoCatalogRefresh(): void {
  if (!enabled) return
  if (refreshTimer) clearTimeout(refreshTimer)
  refreshTimer = setTimeout(() => {
    refreshTimer = null
    try {
      writeAutoCatalog()
    } catch (err) {
      console.warn('[autoCatalog] refresh failed', err)
    }
  }, 3000)
}
