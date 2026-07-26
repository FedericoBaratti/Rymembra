import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface UiState {
  searchOpen: boolean
  queueOpen: boolean
  eqOpen: boolean
  fullscreenViz: boolean
  sleepMenuOpen: boolean
  editTrackId: number | null
  batchEditTrackIds: number[] | null
  lyricsEditTrackId: number | null
  sidebarPinned: boolean

  setSearchOpen: (v: boolean) => void
  setQueueOpen: (v: boolean) => void
  toggleQueue: () => void
  setEqOpen: (v: boolean) => void
  setFullscreenViz: (v: boolean) => void
  setSleepMenuOpen: (v: boolean) => void
  setEditTrackId: (id: number | null) => void
  setBatchEditTrackIds: (ids: number[] | null) => void
  setLyricsEditTrackId: (id: number | null) => void
  setSidebarPinned: (v: boolean) => void
}

export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      searchOpen: false,
      queueOpen: false,
      eqOpen: false,
      fullscreenViz: false,
      sleepMenuOpen: false,
      editTrackId: null,
      batchEditTrackIds: null,
      lyricsEditTrackId: null,
      sidebarPinned: false,

      setSearchOpen: (v) => set({ searchOpen: v }),
      setQueueOpen: (v) => set({ queueOpen: v }),
      toggleQueue: () => set({ queueOpen: !get().queueOpen }),
      setEqOpen: (v) => set({ eqOpen: v }),
      setFullscreenViz: (v) => set({ fullscreenViz: v }),
      setSleepMenuOpen: (v) => set({ sleepMenuOpen: v }),
      setEditTrackId: (id) => set({ editTrackId: id }),
      setBatchEditTrackIds: (ids) => set({ batchEditTrackIds: ids }),
      setLyricsEditTrackId: (id) => set({ lyricsEditTrackId: id }),
      setSidebarPinned: (v) => set({ sidebarPinned: v })
    }),
    {
      name: 'aether-ui',
      partialize: (s) => ({ sidebarPinned: s.sidebarPinned })
    }
  )
)
