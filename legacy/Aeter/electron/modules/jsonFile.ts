import { writeFileSync, mkdirSync, renameSync } from 'node:fs'
import { writeFile, rename, mkdir } from 'node:fs/promises'
import { dirname } from 'node:path'
import { logWarn } from './logger'

export interface DebouncedJsonFile<T> {
  /** Schedules a debounced async write of `data`. */
  write(data: T): void
  /** Cancels any pending write and persists the latest data synchronously. */
  flushSync(): void
}

/**
 * Debounced JSON persistence: rapid successive writes collapse into one
 * async tmp+rename flush. `flushSync` (for app quit) writes synchronously.
 */
export function createDebouncedJsonFile<T>(
  getPath: () => string,
  debounceMs = 400
): DebouncedJsonFile<T> {
  let pending: T | null = null
  let timer: NodeJS.Timeout | null = null

  const flushAsync = async (): Promise<void> => {
    if (pending === null) return
    const data = pending
    pending = null
    const path = getPath()
    try {
      await mkdir(dirname(path), { recursive: true })
      const tmp = `${path}.tmp`
      await writeFile(tmp, JSON.stringify(data, null, 2), 'utf-8')
      await rename(tmp, path)
    } catch (err) {
      logWarn('jsonfile', `Scrittura fallita: ${path}`, err)
    }
  }

  return {
    write(data: T): void {
      pending = data
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        timer = null
        void flushAsync()
      }, debounceMs)
    },
    flushSync(): void {
      if (timer) {
        clearTimeout(timer)
        timer = null
      }
      if (pending === null) return
      const data = pending
      pending = null
      const path = getPath()
      try {
        mkdirSync(dirname(path), { recursive: true })
        const tmp = `${path}.tmp`
        writeFileSync(tmp, JSON.stringify(data, null, 2), 'utf-8')
        renameSync(tmp, path)
      } catch (err) {
        logWarn('jsonfile', `Scrittura sincrona fallita: ${path}`, err)
      }
    }
  }
}
