import { create } from 'zustand'
import type { RadioSeed } from '@shared/types'
import { usePlayerStore } from './usePlayerStore'
import { toast } from './useToastStore'
import i18n from '@/i18n'

// Radio / autoplay state. When a radio is active, useAutoRadio keeps the queue
// topped up with tracks similar to whatever is currently playing, so playback
// never runs dry — the defining Spotify behaviour.

interface DiscoveryState {
  radioActive: boolean
  /** Human label for the active radio (track/artist/genre name). */
  radioLabel: string | null
  startRadio: (seed: RadioSeed, label: string) => Promise<void>
  stopRadio: () => void
}

export const useDiscoveryStore = create<DiscoveryState>((set) => ({
  radioActive: false,
  radioLabel: null,

  startRadio: async (seed, label) => {
    try {
      const res = await window.aether.getRadioSeedTracks(seed, 60)
      if (res.inLibrary.length === 0) {
        toast.error(i18n.t('radio.empty'))
        return
      }
      usePlayerStore.getState().playTracks(res.inLibrary, 0)
      set({ radioActive: true, radioLabel: label })
      toast.success(i18n.t('radio.started', { name: label }))
    } catch {
      toast.error(i18n.t('radio.error'))
    }
  },

  stopRadio: () => set({ radioActive: false, radioLabel: null })
}))
