import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'

/**
 * Filesystem location of the cover-art store.
 *
 * Cover images live as files (one per content hash) OUTSIDE the SQLite DB. The
 * sql.js (WASM) engine used on Android persists by serializing the ENTIRE
 * database on every write, so keeping multi-MB WebP BLOBs inside it made every
 * `recordPlay`/rating/enrichment update rewrite tens of MB — slow, battery-hungry
 * and a corruption risk on a background kill. Holding only the hash in the DB
 * keeps `export()` tiny.
 *
 * `app.getPath('userData')` resolves to the Electron userData dir on desktop and
 * to the Android app-private filesDir via the electron shim (resolveAppPath), so
 * the covers dir is `<userData|filesDir>/covers` on both platforms. It is
 * app-private, so no storage permission is needed (unlike the music files).
 */
let dir: string | null = null

export function coversDir(): string {
  if (!dir) {
    dir = join(app.getPath('userData'), 'covers')
    mkdirSync(dir, { recursive: true })
  }
  return dir
}

/** Absolute path of a cover file. `thumb` selects the 64px variant. */
export function coverPath(hash: string, thumb = false): string {
  return join(coversDir(), thumb ? `${hash}.t.webp` : `${hash}.webp`)
}
