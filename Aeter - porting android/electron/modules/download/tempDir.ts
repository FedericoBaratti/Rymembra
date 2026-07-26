import { mkdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

/**
 * Working dir for yt-dlp intermediates (-P temp:, see ytPathArgs). On Android
 * TMPDIR is the app's private cacheDir (set by nodejs-mobile-cordova), on
 * desktop the OS tmp — either way OUTSIDE the watched library folders, so the
 * scan/watcher never see a half-written download. Stale leftovers are swept by
 * cleanupPartialFiles (downloader.ts).
 */
export function ytdlpTempDir(): string {
  const dir = join(tmpdir(), 'aether-ytdlp')
  try {
    mkdirSync(dir, { recursive: true })
  } catch {
    // already exists or unwritable — yt-dlp surfaces a real error if unusable
  }
  return dir
}
