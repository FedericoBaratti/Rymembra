import { create } from 'zustand'
import type { DownloadItem } from '@shared/types'

interface DownloadsState {
  items: DownloadItem[]
  refresh: () => Promise<void>
  upsert: (item: DownloadItem) => void
}

export const useDownloadsStore = create<DownloadsState>((set, get) => ({
  items: [],

  refresh: async () => {
    // Best-effort: a rejected IPC (backend not ready on early boot) keeps the
    // previous items rather than surfacing an unhandled rejection.
    try {
      set({ items: await window.aether.getDownloads() })
    } catch {
      /* keep current items */
    }
  },

  upsert: (item) => {
    const items = get().items
    const idx = items.findIndex((i) => i.id === item.id)
    if (idx >= 0) {
      const next = [...items]
      next[idx] = item
      set({ items: next })
    } else {
      set({ items: [item, ...items] })
    }
  }
}))
