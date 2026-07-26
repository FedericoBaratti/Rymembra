import { create } from 'zustand'
import type { SpotifyMigrationState } from '@shared/types'

interface SpotifyMigrationStore {
  state: SpotifyMigrationState | null
  set: (s: SpotifyMigrationState | null) => void
  refresh: () => Promise<void>
}

// Mirrors the backend migration state, kept live by the 'spotify:migration'
// event (wired in useAppBootstrap) so the flow survives navigation/remount.
export const useSpotifyMigrationStore = create<SpotifyMigrationStore>((set) => ({
  state: null,
  set: (s) => set({ state: s }),
  // Best-effort: an early IPC reject (backend not ready) keeps the current state
  // instead of surfacing an unhandled rejection.
  refresh: async () => {
    try {
      set({ state: await window.aether.getSpotifyMigration() })
    } catch {
      /* keep current state */
    }
  }
}))
