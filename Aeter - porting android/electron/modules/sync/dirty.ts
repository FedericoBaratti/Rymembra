/**
 * A minimal, dependency-free signal that the local library changed in a way the
 * Drive sync should eventually push (rating/liked edits, playlist mutations).
 * Mutation handlers call `markLibraryDirty()` — a no-op until the sync service
 * registers a listener via `onLibraryDirty()` — so those handlers never have to
 * import the heavy sync/Drive/OAuth stack (and stay safe under unit tests).
 *
 * Scan/import/watcher/deleteTracks already emit `broadcast('library:changed')`;
 * the sync service also subscribes to that, so this covers the mutations that do
 * not broadcast.
 */
let listener: (() => void) | null = null

export function onLibraryDirty(fn: () => void): void {
  listener = fn
}

export function markLibraryDirty(): void {
  try {
    listener?.()
  } catch {
    // never let a sync trigger break a library mutation
  }
}
