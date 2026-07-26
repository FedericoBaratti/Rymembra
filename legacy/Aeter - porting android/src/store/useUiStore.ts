import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface UiState {
  searchOpen: boolean
  queueOpen: boolean
  eqOpen: boolean
  fullscreenViz: boolean
  /** Mobile-only: full-screen synced-lyrics surface over the Now Playing sheet. */
  lyricsOpen: boolean
  sleepMenuOpen: boolean
  editTrackId: number | null
  batchEditTrackIds: number[] | null
  lyricsEditTrackId: number | null
  sidebarPinned: boolean
  /** Mobile-only: expanded "now playing" sheet over the compact player bar. */
  nowPlayingOpen: boolean

  setSearchOpen: (v: boolean) => void
  setQueueOpen: (v: boolean) => void
  toggleQueue: () => void
  setEqOpen: (v: boolean) => void
  setFullscreenViz: (v: boolean) => void
  setLyricsOpen: (v: boolean) => void
  setSleepMenuOpen: (v: boolean) => void
  setEditTrackId: (id: number | null) => void
  setBatchEditTrackIds: (ids: number[] | null) => void
  setLyricsEditTrackId: (id: number | null) => void
  setSidebarPinned: (v: boolean) => void
  setNowPlayingOpen: (v: boolean) => void
}

export const useUiStore = create<UiState>()(
  persist(
    (set, get) => ({
      searchOpen: false,
      queueOpen: false,
      eqOpen: false,
      fullscreenViz: false,
      lyricsOpen: false,
      sleepMenuOpen: false,
      editTrackId: null,
      batchEditTrackIds: null,
      lyricsEditTrackId: null,
      sidebarPinned: false,
      nowPlayingOpen: false,

      setSearchOpen: (v) => set({ searchOpen: v }),
      setQueueOpen: (v) => set({ queueOpen: v }),
      toggleQueue: () => set({ queueOpen: !get().queueOpen }),
      setEqOpen: (v) => set({ eqOpen: v }),
      setFullscreenViz: (v) => set({ fullscreenViz: v }),
      setLyricsOpen: (v) => set({ lyricsOpen: v }),
      setSleepMenuOpen: (v) => set({ sleepMenuOpen: v }),
      setEditTrackId: (id) => set({ editTrackId: id }),
      setBatchEditTrackIds: (ids) => set({ batchEditTrackIds: ids }),
      setLyricsEditTrackId: (id) => set({ lyricsEditTrackId: id }),
      setSidebarPinned: (v) => set({ sidebarPinned: v }),
      setNowPlayingOpen: (v) => set({ nowPlayingOpen: v })
    }),
    {
      name: 'aether-ui',
      partialize: (s) => ({ sidebarPinned: s.sidebarPinned })
    }
  )
)
