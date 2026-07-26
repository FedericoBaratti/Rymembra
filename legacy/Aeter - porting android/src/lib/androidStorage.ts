/**
 * Android storage auto-setup (runs once per app launch from useAppBootstrap).
 *
 * Goal: make `<external>/Download/Music` the music library with zero manual
 * steps. On every launch we idempotently ensure:
 *   1. All Files Access (MANAGE_EXTERNAL_STORAGE) — required to CREATE an empty,
 *      persistent folder in a public collection (scoped storage / SAF can't).
 *   2. the `Download/Music` folder exists (created if missing);
 *   3. that folder is in `watchFolders` (so it gets scanned) and is the
 *      `downloadFolder` (so yt-dlp/Spotify downloads land there).
 *
 * The permission REQUEST opens a system Settings screen, so we only auto-open it
 * the first time (a localStorage flag) — afterwards we stay silent if denied,
 * but still persist the path so the folder is set up the moment access is later
 * granted. The folder path is saved regardless of permission/emptiness, which
 * satisfies "keep the path even if the folder is empty or can't be created yet".
 *
 * Native side: FileAccessPlugin.{hasAllFilesAccess,requestAllFilesAccess,
 * ensureDownloadMusicFolder} in android/app/src/main/java/com/aether/player/.
 */
import type { AppSettings } from '@shared/types'
import { FileAccessNative } from './nativeRpc'
import { useSettingsStore } from '@/store/useSettingsStore'

const MANAGE_ASKED_KEY = 'aether.manageStorageAsked'

function alreadyAsked(): boolean {
  try {
    return localStorage.getItem(MANAGE_ASKED_KEY) === '1'
  } catch {
    return false
  }
}

function markAsked(): void {
  try {
    localStorage.setItem(MANAGE_ASKED_KEY, '1')
  } catch {
    /* private mode / storage disabled — re-asking next launch is acceptable */
  }
}

/**
 * Ensure the Download/Music library folder exists and is wired into settings.
 * Best-effort: any native failure is logged and swallowed so it never blocks
 * the rest of app boot.
 */
export async function ensureAndroidMusicStorage(current: AppSettings): Promise<void> {
  try {
    // 1. All Files Access — request once (opens system Settings), then re-check.
    let granted = (await FileAccessNative.hasAllFilesAccess()).granted
    if (!granted && !alreadyAsked()) {
      markAsked()
      granted = (await FileAccessNative.requestAllFilesAccess()).granted
    }

    // 2. Ensure the folder. Returns the path even if it couldn't be created yet
    //    (no access) — we still persist it for a later, granted launch.
    const { path } = await FileAccessNative.ensureDownloadMusicFolder()
    if (!path) return

    // 3. Persist into settings only when something actually changes, to avoid a
    //    redundant setSettings() (which would trigger a needless rescan).
    const inWatch = current.watchFolders.includes(path)
    const isDownloadDir = current.downloadFolder === path
    if (inWatch && isDownloadDir) return

    const watchFolders = inWatch ? current.watchFolders : [...current.watchFolders, path]
    await useSettingsStore.getState().update({ watchFolders, downloadFolder: path })
  } catch (err) {
    console.warn('[androidStorage] auto-setup failed', err)
  }
}
