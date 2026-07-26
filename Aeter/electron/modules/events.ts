import type { BrowserWindow } from 'electron'
import type { AetherEventName, AetherEvents } from '@shared/types'

let win: BrowserWindow | null = null

export function setMainWindow(w: BrowserWindow | null): void {
  win = w
}

// Internal in-process listeners fired alongside the renderer broadcast. Lets a
// backend module (e.g. the library sync service) react to library changes
// without touching every mutation site. Nothing registers unless a feature
// opts in, so there is no cost otherwise.
type BroadcastListener = (event: AetherEventName, payload: unknown) => void
const listeners: BroadcastListener[] = []

export function onBroadcast(fn: BroadcastListener): () => void {
  listeners.push(fn)
  return () => {
    const i = listeners.indexOf(fn)
    if (i >= 0) listeners.splice(i, 1)
  }
}

export function broadcast<E extends AetherEventName>(event: E, payload: AetherEvents[E]): void {
  if (win && !win.isDestroyed()) {
    win.webContents.send('aether:event', event, payload)
  }
  for (const fn of listeners) {
    try {
      fn(event, payload)
    } catch (err) {
      console.warn('[events] broadcast listener failed', err)
    }
  }
}
