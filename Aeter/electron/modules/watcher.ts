import chokidar, { type FSWatcher } from 'chokidar'
import { existsSync } from 'node:fs'
import { extname } from 'node:path'
import { SUPPORTED_EXTS, TRASH_DIR_NAME, upsertTrackFromFile, removeTrackByPath, rebuildAggregates, isScanning } from './library'

let watcher: FSWatcher | null = null
let rebuildTimer: NodeJS.Timeout | null = null

function scheduleRebuild(notify: () => void): void {
  if (rebuildTimer) clearTimeout(rebuildTimer)
  rebuildTimer = setTimeout(() => {
    rebuildAggregates()
    notify()
  }, 1500)
}

/** (Re)start watching the given folders. Calls notify() after the library changes. */
export function watchFolders(folders: string[], notify: () => void): void {
  void watcher?.close()
  watcher = null
  if (folders.length === 0) return

  watcher = chokidar.watch(folders, {
    ignoreInitial: true,
    awaitWriteFinish: { stabilityThreshold: 1500, pollInterval: 200 },
    depth: 32,
    // dedupe trash: moving a victim there must not re-add it to the library
    ignored: (p: string) => p.split(/[\\/]/).includes(TRASH_DIR_NAME)
  })

  const isAudio = (p: string): boolean => SUPPORTED_EXTS.has(extname(p).toLowerCase())

  watcher.on('add', async (p) => {
    if (!isAudio(p) || isScanning()) return
    await upsertTrackFromFile(p)
    scheduleRebuild(notify)
  })
  watcher.on('change', async (p) => {
    if (!isAudio(p) || isScanning()) return
    await upsertTrackFromFile(p)
    scheduleRebuild(notify)
  })
  watcher.on('unlink', (p) => {
    if (!isAudio(p) || isScanning()) return
    // Spurious unlink events happen on network/FUSE mounts during heavy IO;
    // only a file that is really gone may lose its row.
    if (existsSync(p)) return
    removeTrackByPath(p)
    scheduleRebuild(notify)
  })
}

export function stopWatching(): void {
  void watcher?.close()
  watcher = null
}
