import { app } from 'electron'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { PersistedQueue } from '@shared/types'
import { createDebouncedJsonFile } from './jsonFile'

function queuePath(): string {
  return join(app.getPath('userData'), 'queue.json')
}

const queueFile = createDebouncedJsonFile<PersistedQueue>(queuePath, 1000)

export function readQueueState(): PersistedQueue | null {
  try {
    const raw = readFileSync(queuePath(), 'utf-8')
    const q = JSON.parse(raw) as PersistedQueue
    if (!Array.isArray(q.trackIds) || !Array.isArray(q.order)) return null
    return q
  } catch {
    return null
  }
}

export function writeQueueState(state: PersistedQueue): void {
  queueFile.write(state)
}

/** Persists any pending queue write synchronously (call on quit). */
export function flushQueueStateSync(): void {
  queueFile.flushSync()
}
