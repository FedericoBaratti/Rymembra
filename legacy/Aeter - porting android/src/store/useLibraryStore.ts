import { create } from 'zustand'
import type { Track, Album, Artist, Playlist, ScanProgress, TrackQuery } from '@shared/types'

// Trailing debounce for refreshAll: a burst of library:changed events (scan,
// repair) coalesces into a single full reload instead of N reloads of the whole
// track list. Module-level so it survives re-renders.
let refreshAllTimer: ReturnType<typeof setTimeout> | null = null

// Patch buffer for track:updated: enrichment fires one event per track, and
// rebuilding a 10k-row array per event is O(N²) across a batch (plus one
// re-render each). Buffer the patches and fold them in ONE pass + ONE set per
// flush window instead.
const PATCH_FLUSH_MS = 50
let pendingPatches = new Map<number, Track>()
let patchTimer: ReturnType<typeof setTimeout> | null = null

interface LibraryState {
  tracks: Track[]
  trackQuery: Required<Pick<TrackQuery, 'sortBy' | 'sortDir'>>
  albums: Album[]
  artists: Artist[]
  playlists: Playlist[]
  scanProgress: ScanProgress | null
  loaded: boolean

  refreshTracks: () => Promise<void>
  refreshAll: () => Promise<void>
  refreshAllDebounced: () => void
  patchTrack: (track: Track) => void
  setSort: (sortBy: NonNullable<TrackQuery['sortBy']>) => void
  setScanProgress: (p: ScanProgress | null) => void
  refreshPlaylists: () => Promise<void>
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  tracks: [],
  trackQuery: { sortBy: 'artist', sortDir: 'asc' },
  albums: [],
  artists: [],
  playlists: [],
  scanProgress: null,
  loaded: false,

  refreshTracks: async () => {
    const { trackQuery } = get()
    try {
      const tracks = await window.aether.getTracks(trackQuery)
      set({ tracks, loaded: true })
    } catch (err) {
      // Don't strand the UI on the loading skeletons if the backend rejects
      // (e.g. it is still booting or an IPC channel isn't ready yet). Mark as
      // loaded so an empty/error state shows; a later library:changed event
      // (fired after the startup scan) triggers a fresh refresh.
      console.error('[library] refreshTracks failed', err)
      set({ loaded: true })
    }
  },

  refreshAll: async () => {
    const { trackQuery } = get()
    try {
      const [tracks, albums, artists, playlists] = await Promise.all([
        window.aether.getTracks(trackQuery),
        window.aether.getAlbums(),
        window.aether.getArtists(),
        window.aether.getPlaylists()
      ])
      set({ tracks, albums, artists, playlists, loaded: true })
    } catch (err) {
      // See refreshTracks: never leave `loaded` false forever on a backend error.
      console.error('[library] refreshAll failed', err)
      set({ loaded: true })
    }
  },

  refreshAllDebounced: () => {
    if (refreshAllTimer) clearTimeout(refreshAllTimer)
    refreshAllTimer = setTimeout(() => {
      refreshAllTimer = null
      void get().refreshAll()
    }, 500)
  },

  // Replace a single track in place from a track:updated event payload, instead
  // of re-fetching the entire library. Enrichment emits one event per track, so
  // a full getTracks() per event was O(N²) on large libraries. Album/artist
  // aggregates re-align on the next library:changed (end of scan).
  patchTrack: (track) => {
    pendingPatches.set(track.id, track)
    if (patchTimer) return
    patchTimer = setTimeout(() => {
      patchTimer = null
      const patches = pendingPatches
      pendingPatches = new Map()
      set((s) => {
        let changed = false
        const tracks = s.tracks.map((t) => {
          const p = patches.get(t.id)
          if (p) changed = true
          return p ?? t
        })
        // Unknown ids (e.g. filtered-out tracks) are a no-op: keep the array
        // identity so subscribers don't re-render for nothing.
        return changed ? { tracks } : s
      })
    }, PATCH_FLUSH_MS)
  },

  setSort: (sortBy) => {
    const { trackQuery } = get()
    const sortDir =
      trackQuery.sortBy === sortBy && trackQuery.sortDir === 'asc' ? 'desc' : 'asc'
    set({ trackQuery: { sortBy, sortDir } })
    void get().refreshTracks()
  },

  setScanProgress: (p) => set({ scanProgress: p }),

  refreshPlaylists: async () => {
    try {
      set({ playlists: await window.aether.getPlaylists() })
    } catch (err) {
      // Passive refresh (post save/delete): keep the previous list on failure.
      console.error('[library] refreshPlaylists failed', err)
    }
  }
}))
