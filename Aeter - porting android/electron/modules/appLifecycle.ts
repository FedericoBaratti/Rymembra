/**
 * App foreground/background state, pushed by the renderer on Android
 * (visibilitychange → window.aether.setAppState). Consumers gate periodic work
 * on it so the node process — kept alive by the media/download foreground
 * services — doesn't burn battery while the screen is off.
 *
 * Desktop never calls setAppState, so the state is pinned to 'foreground' and
 * every consumer behaves exactly as before.
 */

export type AppState = 'foreground' | 'background'

type Listener = (state: AppState) => void

let state: AppState = 'foreground'
const listeners = new Set<Listener>()

export function getAppState(): AppState {
  return state
}

export function isBackground(): boolean {
  return state === 'background'
}

export function setAppState(next: AppState): void {
  if (next !== 'foreground' && next !== 'background') return
  if (next === state) return
  state = next
  for (const cb of listeners) {
    try {
      cb(next)
    } catch {
      /* one bad listener must not break the rest */
    }
  }
}

/** Returns an unsubscribe function. */
export function onAppStateChange(cb: Listener): () => void {
  listeners.add(cb)
  return () => listeners.delete(cb)
}
