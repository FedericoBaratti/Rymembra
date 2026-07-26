import { create } from 'zustand'
import type { PhoneRepairItem, PhoneSyncState, PhoneTrackPlanned } from '@shared/types'

/**
 * Renderer state for the phone repair page. `state` mirrors 'phone:state'
 * events (pairing/connection); `tracks` is the last phoneListTracks() result
 * with per-row repair jobs patched in place by 'phoneRepair:updated' (same
 * O(1)-per-event pattern as useDownloadsStore).
 */
interface PhoneSyncStore {
  state: PhoneSyncState | null
  tracks: PhoneTrackPlanned[]
  loadingTracks: boolean
  tracksError: string | null
  refreshState: () => Promise<void>
  setState: (state: PhoneSyncState) => void
  refreshTracks: () => Promise<void>
  applyRepairUpdate: (item: PhoneRepairItem) => void
}

export const usePhoneSyncStore = create<PhoneSyncStore>((set, get) => ({
  state: null,
  tracks: [],
  loadingTracks: false,
  tracksError: null,

  refreshState: async () => {
    set({ state: await window.aether.phoneGetState() })
  },

  setState: (state) => set({ state }),

  refreshTracks: async () => {
    set({ loadingTracks: true, tracksError: null })
    try {
      set({ tracks: await window.aether.phoneListTracks(), loadingTracks: false })
    } catch (err) {
      set({
        loadingTracks: false,
        tracksError: err instanceof Error ? err.message : String(err)
      })
    }
  },

  applyRepairUpdate: (item) => {
    const tracks = get().tracks
    const idx = tracks.findIndex((t) => t.info.id === item.phoneTrackId)
    if (idx < 0) return
    const next = [...tracks]
    next[idx] = { ...next[idx], repair: item }
    set({ tracks: next })
  }
}))
